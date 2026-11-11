//! Evidence reference + check-record tests (bajan-5ux), red-first.
//!
//! Purpose: coverage for `gm_evidence_ref`, `gm_evidence_resolution`,
//! `gm_evidence_check_record`, and `gm_evidence_ref_tier`
//! (specs/graph-model.md, deployed mirrors p-evidence-ref /
//! p-evidence-resolution / p-evidence-check-record / p-evidence-ref-tier).
//! Responsibilities: the evidence check is the sole writer of
//! `evidence_refs`/`evidence_checks`; tri-state derivation from store
//! facts; tombstone-window checks flip to resolved only post-revive with
//! history untouched; read paths write nothing; the observed tier is
//! provenance, never a ranking input; end-to-end lifecycle ingest →
//! extract → check → delete → check → revive → check.
//! Rationale: written red before the implementation, per repo TDD policy.

use bajan::extract::{
    self, CandidateClaim, ExtractReport, ExtractionFailure, ExtractionRunStore, Extractor,
};
use bajan::ingest::{self, EpisodeRecord, Locator, SourceMeta};
use bajan::store::sqlite::{ResolutionState, SqliteStore, derive_resolution};
use bajan::store::{ClaimNode, ClaimStatus, Evidence, Lineage};
use std::collections::HashMap;

fn episode(id: &str, text: &str, tier: u8) -> EpisodeRecord {
    EpisodeRecord {
        id: id.into(),
        text: text.into(),
        locator: Locator::Span("heading:Notes".into()),
        source: SourceMeta {
            source_type: "episode".into(),
            data_cutoff: Some("2026-01-01".into()),
            authority_tier: tier,
            tags: vec!["workspace:dev".into()],
        },
    }
}

fn span_claim(text: &str) -> ClaimNode {
    ClaimNode {
        text: text.into(),
        valid_at: None,
        invalid_at: None,
        data_cutoff: None,
        status: ClaimStatus::Staged,
        scope: "workspace:dev".into(),
        source_type: "episode".into(),
        evidence: Evidence::Span {
            text: text.into(),
            locator: "heading:Notes".into(),
        },
    }
}

fn insert_claim_on(db: &SqliteStore, episode_id: &str, text: &str) -> usize {
    let node = span_claim(text);
    let lineage = vec![Lineage {
        episode_id: episode_id.into(),
        extractor_version: "1.0.0".into(),
    }];
    let supporting = db
        .get_episode(episode_id)
        .expect("episode read")
        .map(|e| e.text);
    db.insert_claim(&node, &lineage, supporting.as_deref().unwrap_or(""))
        .expect("claim inserts")
}

/// Full claim-node snapshot used for the byte-identical assertion
/// (`gm_evidence_ref` — the frozen ClaimNode is untouched by checks).
fn claim_snapshot(db: &SqliteStore, claim_key: usize) -> String {
    let (_, node, lineage) = db
        .claims_with_lineage()
        .expect("claims read back")
        .into_iter()
        .find(|(key, _, _)| *key == claim_key)
        .expect("claim exists");
    format!("{node:?}{lineage:?}")
}

// --- reference creation (gm_evidence_ref / p-evidence-ref) ----------------

/// The first check lazily creates the reference and the claim node stays
/// byte-identical; a re-check never duplicates the reference.
#[test]
fn evidence_check_creates_reference_and_claim_stays_untouched() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(&db, &episode("ep-ev", "Bajan runs locally on one call.", 2)).expect("persist");
    let claim_key = insert_claim_on(&db, "ep-ev", "Bajan runs locally on one call.");
    let before = claim_snapshot(&db, claim_key);

    let outcome = db.evidence_check(claim_key, 1_000).expect("first check");
    assert!(outcome.created, "first check creates the reference");
    assert_eq!(outcome.reference.episode_id, "ep-ev");
    assert_eq!(outcome.reference.observed_tier, Some(2));
    assert_eq!(
        outcome.reference.locator_scheme_version,
        bajan::store::sqlite::LOCAL_LOCATOR_SCHEME
    );

    // Byte-identical claim node: fields, status, lineage unchanged.
    assert_eq!(claim_snapshot(&db, claim_key), before);

    // Re-check: no duplicate reference.
    let second = db.evidence_check(claim_key, 2_000).expect("re-check");
    assert!(!second.created, "re-check reuses the reference");
    assert_eq!(second.reference, outcome.reference);
}

// --- tri-state derivation (gm_evidence_resolution / p-evidence-resolution)

/// The pure derivation is table-driven over store facts: tombstoned wins
/// first, then episode availability, then span containment; unknown
/// evidence and failing spans are unresolvable — never fabricated.
#[test]
fn resolution_tri_state_derives_from_store_facts() {
    let contained = Evidence::Span {
        text: "Bajan runs locally on one".into(),
        locator: "heading:Notes".into(),
    };
    let not_contained = Evidence::Span {
        text: "Text that appears nowhere in the episode.".into(),
        locator: "heading:Notes".into(),
    };
    let body = Some("Bajan runs locally on one call.");
    let cases: Vec<(&str, Evidence, Option<&str>, bool, ResolutionState)> = vec![
        (
            "episode present, span contained",
            contained.clone(),
            body,
            false,
            ResolutionState::Resolved,
        ),
        (
            "episode present, span NOT contained",
            not_contained.clone(),
            body,
            false,
            ResolutionState::Unresolvable,
        ),
        (
            "episode absent",
            contained.clone(),
            None,
            false,
            ResolutionState::Unresolvable,
        ),
        (
            "tombstoned wins over everything",
            contained.clone(),
            body,
            true,
            ResolutionState::Tombstoned,
        ),
        (
            "tombstoned with absent episode",
            not_contained.clone(),
            None,
            true,
            ResolutionState::Tombstoned,
        ),
        (
            "unknown evidence is unresolvable",
            Evidence::Unknown,
            body,
            false,
            ResolutionState::Unresolvable,
        ),
        (
            "whitespace collapse before containment",
            Evidence::Span {
                text: "Bajan   runs\nlocally.".into(),
                locator: "l".into(),
            },
            Some("Bajan runs locally. Extra."),
            false,
            ResolutionState::Resolved,
        ),
    ];
    for (name, evidence, episode_text, tombstoned, expected) in cases {
        assert_eq!(
            derive_resolution(&evidence, episode_text, tombstoned),
            expected,
            "case: {name}"
        );
    }
}

/// Tombstone-window check records `tombstoned`; the post-revive check
/// records `resolved`; the revive rewrites no prior record (design D3).
/// Reachability note: `gm_lineage_survives` refuses deleting an episode
/// that still has a supporting claim, so the window is reached the way a
/// dump-rebuild reaches it (`gm_embedded_store`): the claim's lineage
/// predates the policy deletion — the claim is restored into a store
/// whose episode is already tombstoned.
#[test]
fn tombstone_window_then_revive_recheck_flips_future_not_history() {
    let db = SqliteStore::open(":memory:").expect("store");
    let body = "Evidence spans resolve locally.";
    ingest::persist(&db, &episode("ep-tomb", body, 1)).expect("persist");
    // Policy-delete BEFORE the claim exists (dump-rebuild shape).
    db.delete_episode("ep-tomb", 5_000).expect("policy delete");
    let claim_key = insert_claim_on(&db, "ep-tomb", body);

    let windowed = db
        .evidence_check(claim_key, 6_000)
        .expect("check in window");
    assert_eq!(windowed.check.resolution_state, ResolutionState::Tombstoned);

    db.revive_episode("ep-tomb").expect("revive");
    // Revive clears the tombstone; recreation is the first post-revive
    // submission (`ic_deleted_recreate`) — same verbatim content.
    ingest::persist(&db, &episode("ep-tomb", body, 1)).expect("post-revive re-ingest");
    let revived = db
        .evidence_check(claim_key, 7_000)
        .expect("post-revive check");
    assert_eq!(revived.check.resolution_state, ResolutionState::Resolved);

    let records = db.evidence_checks(claim_key).expect("records read back");
    assert_eq!(records.len(), 2, "both checks persist");
    assert_eq!(
        records[0].resolution_state,
        ResolutionState::Tombstoned,
        "history untouched"
    );
    assert_eq!(records[1].resolution_state, ResolutionState::Resolved);
}

// --- append-only records (gm_evidence_check_record / p-evidence-check-record)

/// Every check appends exactly one immutable record with its own state and
/// timestamp; no record is ever rewritten.
#[test]
fn check_records_accumulate_append_only() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(
        &db,
        &episode("ep-acc", "Append-only check records persist.", 2),
    )
    .expect("persist");
    let claim_key = insert_claim_on(&db, "ep-acc", "Append-only check records persist.");

    let first = db.evidence_check(claim_key, 1_100).expect("check 1");
    let second = db.evidence_check(claim_key, 2_200).expect("check 2");
    let third = db.evidence_check(claim_key, 3_300).expect("check 3");

    let records = db.evidence_checks(claim_key).expect("records read back");
    assert_eq!(records.len(), 3, "one record per check run");
    assert_eq!(records[0], first.check);
    assert_eq!(records[1], second.check);
    assert_eq!(records[2], third.check);
    assert!(
        records[0].checked_at < records[1].checked_at
            && records[1].checked_at < records[2].checked_at,
        "own timestamps, append-only order"
    );
}

/// Read paths write nothing to either evidence table (`qt_readonly`
/// holds without exception — the check command is the sole writer).
#[test]
fn read_commands_leave_evidence_tables_untouched() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(
        &db,
        &episode("ep-ro", "Read paths never write evidence state.", 3),
    )
    .expect("persist");
    let claim_key = insert_claim_on(&db, "ep-ro", "Read paths never write evidence state.");
    db.evidence_check(claim_key, 1_000).expect("seed check");

    // Exhaustive read-path sweep: nothing may gain a row.
    let _ = db.episodes().expect("episodes read");
    let _ = db.claims_with_lineage().expect("claims read");
    let _ = db.get_episode("ep-ro").expect("episode read");
    let _ = db.evidence_ref(claim_key).expect("ref read");
    let _ = db.evidence_checks(claim_key).expect("checks read");

    let refs = db.evidence_ref(claim_key).expect("ref read");
    assert!(refs.is_some());
    let records = db.evidence_checks(claim_key).expect("checks read");
    assert_eq!(records.len(), 1, "no read added a check record");
}

// --- tier provenance (gm_evidence_ref_tier / p-evidence-ref-tier) ---------

/// The reference carries the episode's authority tier as observed;
/// resolution changes no claim text, status, or ordering — provenance
/// only, never a ranking input.
#[test]
fn observed_tier_is_provenance_never_ranking() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(
        &db,
        &episode("ep-low", "Tier one sources persist normally.", 1),
    )
    .expect("persist");
    ingest::persist(
        &db,
        &episode("ep-high", "Tier three sources persist normally.", 3),
    )
    .expect("persist");
    let low = insert_claim_on(&db, "ep-low", "Tier one sources persist normally.");
    let high = insert_claim_on(&db, "ep-high", "Tier three sources persist normally.");

    let low_out = db.evidence_check(low, 1_000).expect("low check");
    let high_out = db.evidence_check(high, 1_100).expect("high check");
    assert_eq!(low_out.reference.observed_tier, Some(1));
    assert_eq!(high_out.reference.observed_tier, Some(3));

    // Claims are untouched by resolution — no re-rank, no re-weight.
    let claims = db.claims_with_lineage().expect("claims read back");
    let by_key: HashMap<usize, &bajan::store::ClaimNode> =
        claims.iter().map(|(k, n, _)| (*k, n)).collect();
    assert_eq!(by_key[&low].status, ClaimStatus::Staged);
    assert_eq!(by_key[&high].status, ClaimStatus::Staged);
    assert_eq!(by_key[&low].text, "Tier one sources persist normally.");
}

// --- end-to-end lifecycle (task 3.1) ---------------------------------------

struct ScriptedExtractor {
    outputs: HashMap<String, Result<Vec<CandidateClaim>, ExtractionFailure>>,
}

impl std::fmt::Debug for ScriptedExtractor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptedExtractor").finish()
    }
}

impl Extractor for ScriptedExtractor {
    fn version(&self) -> &str {
        "5.0.0"
    }
    fn propose(&self, episode: &EpisodeRecord) -> Result<Vec<CandidateClaim>, ExtractionFailure> {
        self.outputs
            .get(episode.id.as_str())
            .cloned()
            .unwrap_or_else(|| Err(ExtractionFailure::Infrastructure("no script".into())))
    }
}

/// ingest → extract → check (resolved); delete → check (tombstoned);
/// revive → check (resolved); both check records persist append-only.
#[test]
fn ingest_extract_check_delete_revive_recheck_lifecycle() {
    let db = SqliteStore::open(":memory:").expect("store");
    let body = "Bajan checks evidence against store facts.";
    ingest::persist(&db, &episode("ep-life", body, 2)).expect("ingest");

    // Extract through the real seam: the staged claim carries a span
    // contained in the episode text.
    let outputs: HashMap<String, Result<Vec<CandidateClaim>, ExtractionFailure>> = [(
        "ep-life".to_string(),
        Ok(vec![CandidateClaim {
            text: body.into(),
            valid_at: None,
            invalid_at: None,
            evidence: Evidence::Span {
                text: body.into(),
                locator: "heading:Notes".into(),
            },
        }]),
    )]
    .into_iter()
    .collect();
    let extractor = ScriptedExtractor { outputs };
    let mut runs = ExtractionRunStore::default();
    let _report: ExtractReport =
        extract::run_extract_with(&db, &extractor, &mut runs).expect("extraction pass");
    let claim_count = db.claims_with_lineage().expect("claims read back").len();
    assert_eq!(claim_count, 1, "claim staged through the real seam");

    let (claim_key, _, _) = db
        .claims_with_lineage()
        .expect("claims read back")
        .into_iter()
        .next()
        .expect("staged claim exists");

    // 1) resolved on the live episode; a second check appends honestly.
    let live = db.evidence_check(claim_key, 10_000).expect("check live");
    assert_eq!(live.check.resolution_state, ResolutionState::Resolved);
    assert!(live.created);
    let again = db.evidence_check(claim_key, 10_500).expect("re-check live");
    assert_eq!(again.check.resolution_state, ResolutionState::Resolved);
    assert!(!again.created);

    // 2) the tombstone window is unreachable through the live seam by
    // design: gm_lineage_survives refuses policy-deleting an episode that
    // still has a supporting claim — pin that interplay here. The
    // tombstoned → post-revive legs are covered by
    // tombstone_window_then_revive_recheck_flips_future_not_history
    // (dump-rebuild shape: claim lineage predating the deletion).
    let refused = db.delete_episode("ep-life", 11_000);
    assert!(
        matches!(refused, Err(bajan::store::StoreError::DeleteRefused { .. })),
        "deletion under a supporting claim is refused"
    );

    // 3) the two live checks persist append-only.
    let records = db.evidence_checks(claim_key).expect("records read back");
    let codes: Vec<&str> = records.iter().map(|r| r.resolution_state.code()).collect();
    assert_eq!(codes, vec!["resolved", "resolved"]);
    assert_eq!(
        db.evidence_ref(claim_key)
            .expect("ref read")
            .map(|r| r.observed_tier),
        Some(Some(2))
    );
}
