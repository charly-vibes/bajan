//! Two-store differential state machine (bajan/two-store-implementations-agree).
//!
//! Purpose: drive `ClaimStore` (the pure Vec model) and `SqliteStore`
//! (in-memory SQLite) through identical arguments over the shared
//! four-operation alphabet — `insert`/`insert_claim`,
//! `supersede_prior_versions`, `stage_invalidation_proposal`, `adopt` —
//! and assert the two implementations accept and reject identically
//! (antithesis/scratchbook/two-store-implementations-agree.md).
//! Responsibilities: one `#[hegel::state_machine]` model holding both
//! stores plus per-store live-key tracking (keys can diverge after a
//! defect, so they are recorded separately); rules assert clause 1
//! (Ok/Err verdicts agree), the invariant asserts clauses 2-4 (per-claim
//! status agreement, supersession/invalidation record equality, audit
//! equality with exactly one audit row per adopted claim).
//! Oracle scope (eval report §6): the shared four ops ONLY. Model-only
//! `adopt_batch` and multi-lineage inserts are OUT OF SCOPE — `ClaimStore`
//! keeps exactly one `Lineage` per node, so the machine collapses every
//! drawn lineage set to its first episode id; divergence outside the
//! shared alphabet is invisible by construction.
//! Rationale: predicted GREEN (no confirmed defect on this alphabet);
//! runs live, not `#[ignore]`d. The deliberate out-of-range claim keys in
//! the index rules compare refusal behavior and are never assumed away.

use bajan::store::sqlite::SqliteStore;
use bajan::store::{
    ClaimNode, ClaimStatus, ClaimStore, Evidence, InvalidationProposal, Lineage, StoreError,
    SupersessionRecord,
};
use hegel::TestCase;
use hegel::generators as gs;
use hegel::generators::Generator as _;
use hegel::stateful::machine;

/// Episode-id pool small enough that inserts collide on episodes, so
/// `supersede_prior_versions` reaches same-episode/different-version state.
const EPISODE_IDS: [&str; 3] = ["ep-0", "ep-1", "ep-2"];
/// Extractor-version pool: re-extraction bumps the version, giving the
/// supersession provenance guard something to compare.
const EXTRACTOR_VERSIONS: [&str; 2] = ["v1", "v2"];
const SOURCES: [&str; 2] = ["llm", "human"];
const SCOPES: [&str; 2] = ["global", "session"];
const STATUSES: [ClaimStatus; 3] = [
    ClaimStatus::Staged,
    ClaimStatus::Active,
    ClaimStatus::Rejected,
];
/// Clearly-invalid claim key: far past any key either store can produce.
const INVALID_KEY: usize = usize::MAX;

/// Differential model: one pure-model store, one SQLite store, and the
/// successfully inserted claim keys of each, tracked separately because a
/// defect may make the two key spaces diverge mid-sequence.
struct TwoStoreModel {
    model: ClaimStore,
    sqlite: SqliteStore,
    model_keys: Vec<usize>,
    sqlite_keys: Vec<usize>,
}

impl TwoStoreModel {
    fn new() -> Self {
        TwoStoreModel {
            model: ClaimStore::default(),
            sqlite: SqliteStore::open_in_memory().expect("open in-memory sqlite store"),
            model_keys: Vec::new(),
            sqlite_keys: Vec::new(),
        }
    }

    /// Map a drawn index over the tracked live keys into a per-store key.
    /// In range, each store receives its own tracked key at that index;
    /// out of range (`index > len`, up to `len + 2`), both receive a
    /// clearly-invalid key so refusal behavior is compared, never assumed
    /// away. Uses the longer tracking list so a divergence in key counts
    /// still drives both stores with the same drawn index.
    fn resolve_key(&self, index: i64) -> (usize, usize) {
        let index = index as usize;
        let live = self.model_keys.len().max(self.sqlite_keys.len());
        if index < live {
            (
                self.model_keys.get(index).copied().unwrap_or(INVALID_KEY),
                self.sqlite_keys.get(index).copied().unwrap_or(INVALID_KEY),
            )
        } else {
            (INVALID_KEY, INVALID_KEY)
        }
    }
}

/// Draw the shared claim text: free text, the empty string, or a
/// newline/quote-bearing literal (the projection seams escape text into
/// SQL and JSON, so these shapes matter).
fn draw_text(tc: &TestCase) -> String {
    tc.draw(gs::one_of([
        gs::text().max_size(48).boxed(),
        gs::just(String::new()).boxed(),
        gs::just("line\nbreak \"quoted\"".to_string()).boxed(),
    ]))
}

/// Build the shared node/lineage/episode triple for an insert from drawn
/// parts. `supporting_episode` embeds the span text verbatim, so the
/// hedge-marker guard (`gm_hedge_anchor`) cannot fire for span evidence —
/// its verdicts are compared where it does fire, via drawn span text the
/// episode does not contain is avoided by construction here; the guard's
/// refusal path itself is covered by the dedicated unit tests, not this
/// alphabet.
fn build_insert(
    text: String,
    status: ClaimStatus,
    lineage_ids: Vec<String>,
    extractor_version: String,
    span_text: String,
    span_locator: String,
    spanned: bool,
    scope: String,
    source_type: String,
) -> (ClaimNode, Lineage, String) {
    let evidence = if spanned {
        Evidence::Span {
            text: span_text,
            locator: span_locator,
        }
    } else {
        Evidence::Unknown
    };
    let node = ClaimNode {
        text,
        valid_at: None,
        invalid_at: None,
        data_cutoff: None,
        status,
        scope,
        source_type,
        evidence,
    };
    // Multi-lineage inserts are out of scope: the model keeps exactly one
    // lineage per node, so the drawn id set collapses to its first episode.
    let episode_id = lineage_ids
        .first()
        .cloned()
        .unwrap_or_else(|| EPISODE_IDS[0].to_string());
    let lineage = Lineage {
        episode_id,
        extractor_version,
    };
    let supporting_episode = match &node.evidence {
        Evidence::Span { text, .. } => format!("{text} — supporting episode body"),
        Evidence::Unknown => "supporting episode body".to_string(),
    };
    (node, lineage, supporting_episode)
}

#[hegel::state_machine]
impl TwoStoreModel {
    /// `insert`/`insert_claim` with identical arguments, per-store call
    /// shapes normalized (`ClaimStore::insert(&mut self, node, lineage,
    /// &str) -> Result<(), StoreError>` vs `SqliteStore::insert_claim(&self,
    /// &ClaimNode, &[Lineage], &str) -> Result<usize, StoreError>`). Each
    /// store's returned key is recorded in its own tracking list on Ok.
    #[rule]
    fn insert(&mut self, tc: TestCase) {
        let text = draw_text(&tc);
        let status = tc.draw(gs::sampled_from(STATUSES.to_vec()));
        // Draw the lineage count, then that many episode ids.
        let lineage_len = tc.draw(gs::integers::<i64>().min_value(0).max_value(3));
        let mut lineage_ids = Vec::new();
        for _ in 0..lineage_len {
            lineage_ids.push(tc.draw(gs::sampled_from(EPISODE_IDS.to_vec())).to_string());
        }
        let extractor_version = tc
            .draw(gs::sampled_from(EXTRACTOR_VERSIONS.to_vec()))
            .to_string();
        let spanned = tc.draw(gs::sampled_from(vec![true, false]));
        let span_text = draw_text(&tc);
        let span_locator = tc.draw(gs::text().max_size(24));
        let scope = tc.draw(gs::sampled_from(SCOPES.to_vec())).to_string();
        let source_type = tc.draw(gs::sampled_from(SOURCES.to_vec())).to_string();
        let (node, lineage, supporting_episode) = build_insert(
            text,
            status,
            lineage_ids,
            extractor_version,
            span_text,
            span_locator,
            spanned,
            scope,
            source_type,
        );

        let model_verdict = self
            .model
            .insert(node.clone(), lineage.clone(), &supporting_episode);
        let sqlite_verdict = self
            .sqlite
            .insert_claim(&node, &[lineage], &supporting_episode);
        match (model_verdict, sqlite_verdict) {
            (Ok(()), Ok(key)) => {
                self.model_keys.push(self.model.nodes().len() - 1);
                self.sqlite_keys.push(key);
            }
            (Ok(()), Err(err)) => panic!(
                "two-store divergence: insert accepted by ClaimStore but rejected by SqliteStore: {err:?}"
            ),
            (Err(err), Ok(_)) => panic!(
                "two-store divergence: insert accepted by SqliteStore but rejected by ClaimStore: {err:?}"
            ),
            // Refusals must be for the same reason, not just both refusals.
            (Err(model_err), Err(sqlite_err)) => {
                assert_eq!(
                    std::mem::discriminant(&model_err),
                    std::mem::discriminant(&sqlite_err),
                    "two-store divergence: insert refused with different error kinds: {model_err:?} vs {sqlite_err:?}"
                );
            }
        }
    }

    /// `supersede_prior_versions` with shared (episode, version, at).
    /// The model returns the superseded keys directly; a SQLite read or
    /// write error is a divergence on its own. Note the model is not
    /// refused by this op, so no Ok/Err pair exists — the compared
    /// verdict is the superseded-key list itself.
    #[rule]
    fn supersede(&mut self, tc: TestCase) {
        let episode_id = tc.draw(gs::sampled_from(EPISODE_IDS.to_vec()));
        let extractor_version = tc.draw(gs::sampled_from(EXTRACTOR_VERSIONS.to_vec()));
        let superseded_at = tc.draw(gs::integers::<i64>().min_value(0).max_value(1_000_000)) as u64;
        let model_keys =
            self.model
                .supersede_prior_versions(episode_id, extractor_version, superseded_at);
        let sqlite_keys =
            self.sqlite
                .supersede_prior_versions(episode_id, extractor_version, superseded_at);
        match sqlite_keys {
            Ok(keys) => {
                assert_eq!(
                    model_keys, keys,
                    "two-store divergence: superseded key sets disagree"
                );
            }
            Err(err) => panic!(
                "two-store divergence: supersede_prior_versions errored on SqliteStore: {err:?}"
            ),
        }
    }

    /// `stage_invalidation_proposal` with a shared claim index (live-key
    /// or deliberately invalid) and causing episode.
    #[rule]
    fn stage_invalidation(&mut self, tc: TestCase) {
        let index = tc.draw(
            gs::integers::<i64>()
                .min_value(0)
                .max_value(self.model_keys.len().max(self.sqlite_keys.len()) as i64 + 2),
        );
        let (model_key, sqlite_key) = self.resolve_key(index);
        let causing_episode_id = tc.draw(gs::sampled_from(EPISODE_IDS.to_vec()));
        let model_verdict = self
            .model
            .stage_invalidation_proposal(model_key, causing_episode_id);
        let sqlite_verdict = self
            .sqlite
            .stage_invalidation_proposal(sqlite_key, causing_episode_id);
        assert_verdicts_agree(
            "stage_invalidation_proposal",
            model_key,
            sqlite_key,
            &verdict_kind(model_verdict.map(|_| ())),
            &verdict_kind(sqlite_verdict),
        );
    }

    /// `adopt` with a shared claim index, actor, and timestamp; clause 4's
    /// exactly-one-audit-row check runs inline for the adopted claim in
    /// both stores.
    #[rule]
    fn adopt(&mut self, tc: TestCase) {
        let index = tc.draw(
            gs::integers::<i64>()
                .min_value(0)
                .max_value(self.model_keys.len().max(self.sqlite_keys.len()) as i64 + 2),
        );
        let (model_key, sqlite_key) = self.resolve_key(index);
        let actor = tc.draw(gs::sampled_from(vec!["alice", "bob"])).to_string();
        let adopted_at = tc.draw(gs::integers::<i64>().min_value(0).max_value(1_000_000)) as u64;
        let model_verdict = self.model.adopt(model_key, &actor, adopted_at);
        let sqlite_verdict = self.sqlite.adopt(sqlite_key, &actor, adopted_at);
        let model_kind = model_verdict
            .as_ref()
            .map(|_| ())
            .map_err(|e: &StoreError| std::mem::discriminant(e));
        let sqlite_kind = sqlite_verdict
            .as_ref()
            .map(|_| ())
            .map_err(|e: &StoreError| std::mem::discriminant(e));
        assert_eq!(
            model_kind, sqlite_kind,
            "two-store divergence: adopt verdicts disagree (model key {model_key}, sqlite key {sqlite_key})"
        );
        if model_verdict.is_ok() {
            // Clause 4: the adopted claim has exactly one audit row in both.
            let model_rows = self
                .model
                .audit()
                .iter()
                .filter(|r| r.claim_key == model_key)
                .count();
            let sqlite_rows = self
                .sqlite
                .audit()
                .expect("audit read")
                .into_iter()
                .filter(|r| r.claim_key == sqlite_key)
                .count();
            assert_eq!(
                model_rows, 1,
                "adopted claim must have exactly one model audit row"
            );
            assert_eq!(
                sqlite_rows, 1,
                "adopted claim must have exactly one sqlite audit row"
            );
        }
    }

    /// Clause 2: per-claim status agreement (`ClaimStore::nodes()` vs
    /// `SqliteStore::claims_with_lineage()`), plus key alignment. Clauses
    /// 3 and 4's full-trail equality are checked here too, so a sampled
    /// run still covers them at the machine's initial and final states.
    #[invariant]
    fn stores_agree(&self, tc: TestCase) {
        let sqlite_claims = self
            .sqlite
            .claims_with_lineage()
            .expect("claims_with_lineage read");
        assert_eq!(
            self.model.nodes().len(),
            sqlite_claims.len(),
            "two-store divergence: claim counts disagree"
        );
        for ((key, model_node), (sqlite_key, sqlite_node, _)) in
            self.model.nodes().iter().enumerate().zip(&sqlite_claims)
        {
            assert_eq!(
                key, *sqlite_key,
                "two-store divergence: sqlite claim keys are not the model's indices"
            );
            assert_eq!(
                model_node.status, sqlite_node.status,
                "two-store divergence: per-claim status disagrees"
            );
            assert_eq!(
                model_node.text, sqlite_node.text,
                "two-store divergence: per-claim text disagrees"
            );
        }

        // Clause 3: supersession and invalidation trails, same order,
        // same fields.
        let sqlite_supersessions: Vec<SupersessionRecord> =
            self.sqlite.supersessions().expect("supersessions read");
        assert_eq!(
            self.model.supersessions(),
            sqlite_supersessions.as_slice(),
            "two-store divergence: supersession trails disagree"
        );
        let sqlite_invalidations: Vec<InvalidationProposal> =
            self.sqlite.invalidations().expect("invalidations read");
        assert_eq!(
            self.model.invalidations(),
            sqlite_invalidations.as_slice(),
            "two-store divergence: invalidation trails disagree"
        );

        // Clause 4: audit trails equal.
        let sqlite_audit = self.sqlite.audit().expect("audit read");
        assert_eq!(
            self.model.audit(),
            sqlite_audit.as_slice(),
            "two-store divergence: audit trails disagree"
        );

        // Shrink diagnostics: which store drifted.
        tc.note(&format!(
            "model claims {} / sqlite claims {}",
            self.model.nodes().len(),
            sqlite_claims.len()
        ));
    }
}

/// Compare an Ok/Err verdict pair by discriminant so the failure message
/// names the op, both keys, and both error kinds. Equality of
/// discriminants (not full payload) is the honest bar: the error payloads
/// embed each store's own claim key, which legitimately differ once the
/// key spaces diverge.
fn assert_verdicts_agree(
    op: &str,
    model_key: usize,
    sqlite_key: usize,
    model_kind: &Result<(), std::mem::Discriminant<StoreError>>,
    sqlite_kind: &Result<(), std::mem::Discriminant<StoreError>>,
) {
    assert_eq!(
        model_kind, sqlite_kind,
        "two-store divergence: {op} verdicts disagree (model key {model_key}, sqlite key {sqlite_key})"
    );
}

fn verdict_kind(verdict: Result<(), StoreError>) -> Result<(), std::mem::Discriminant<StoreError>> {
    verdict.map(|_| ()).map_err(|e| std::mem::discriminant(&e))
}

#[hegel::test]
fn two_store_differential(tc: TestCase) {
    machine(TwoStoreModel::new()).steps(40).run(tc);
}
