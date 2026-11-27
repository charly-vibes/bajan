//! Property: `bajan/dump-restore-preserves-human-decision-trail` (bajan-llg).
//!
//! Purpose: pin the invariant that `dump_extraction_output` followed by
//! `restore_extraction_output` into a fresh `SqliteStore` preserves the
//! human decision trail — audit rows, restage history, and the
//! evidence-ref/check records — up to ordering.
//! Responsibilities: two ignored red tests (bug bajan-7n3: the
//! `ExtractionDump` struct carries no `audits`, `restages`,
//! `evidence_refs`, or `evidence_checks` tables, so restore drops them by
//! construction) plus one live `#[hegel::test]` control asserting the
//! graph views (claims/edges/invalidations/supersessions) DO survive, to
//! isolate the missing-tables failure from general restore breakage.
//! Rationale: plan doc
//! `antithesis/scratchbook/dump-restore-preserves-human-decision-trail.md`;
//! the dump carries `review_audits`/`contradiction_audits` but not claim
//! audits/restages/evidence, so the rebuild violates "rebuild reproduces
//! the graph, never re-litigates it" for the human trail specifically.

use bajan::ingest::{EpisodeRecord, Locator, SourceMeta};
use bajan::store::sqlite::SqliteStore;
use bajan::store::{AuditRecord, ClaimNode, ClaimStatus, EdgeLabel, Evidence, Lineage};
use hegel::generators as gs;

// ---- fixture helpers (shape follows tests/write_path_discipline.rs) ----

const EPISODE_TEXT: &str = "alpha beta gamma delta epsilon zeta.";
const SPAN: &str = "beta gamma";

fn episode(id: &str, text: &str) -> EpisodeRecord {
    EpisodeRecord {
        id: id.into(),
        text: text.into(),
        locator: Locator::Span("heading:Notes".into()),
        source: SourceMeta {
            source_type: "episode".into(),
            data_cutoff: None,
            authority_tier: 1,
            tags: vec!["workspace:dev".into()],
        },
    }
}

fn node_with(text: &str, status: ClaimStatus) -> ClaimNode {
    ClaimNode {
        text: text.into(),
        valid_at: None,
        invalid_at: None,
        data_cutoff: None,
        status,
        scope: "workspace:dev".into(),
        source_type: "episode".into(),
        evidence: Evidence::Span {
            text: SPAN.into(),
            locator: "heading:Notes".into(),
        },
    }
}

fn lineage_with(episode_id: &str) -> Lineage {
    Lineage {
        episode_id: episode_id.into(),
        extractor_version: "0.1.0".into(),
    }
}

/// Seed one episode plus one span-backed claim on it; returns the key.
fn seed(db: &SqliteStore, id: &str, text: &str, status: ClaimStatus) -> usize {
    db.insert_episode(&episode(id, EPISODE_TEXT))
        .expect("persist episode");
    db.insert_claim(&node_with(text, status), &[lineage_with(id)], EPISODE_TEXT)
        .expect("insert claim")
}

fn audit_set(records: Vec<AuditRecord>) -> Vec<AuditRecord> {
    let mut out = records;
    out.sort_by(|a, b| {
        (&a.claim_key, &a.actor, &a.adopted_at).cmp(&(&b.claim_key, &b.actor, &b.adopted_at))
    });
    out
}

/// The fixed-by-hand history for the red tests: 2 claims, 1 adopt, 1
/// supersede, 1 restage, 2 evidence checks. Returns the claim keys
/// `(restaged, adopted)`.
fn build_history(db: &SqliteStore) -> (usize, usize) {
    let restaged = seed(db, "ep-a", "alpha claim", ClaimStatus::Staged);
    let adopted = seed(db, "ep-b", "beta claim", ClaimStatus::Staged);
    db.adopt(adopted, "sasha", 1_000).expect("adopt");
    db.supersede_prior_versions("ep-a", "0.2.0", 2_000)
        .expect("supersede");
    db.restage(restaged, "sasha", 3_000).expect("restage");
    db.evidence_check(adopted, 4_000).expect("check 1");
    db.evidence_check(adopted, 5_000).expect("check 2");
    (restaged, adopted)
}

// ---- red tests (bajan-7n3: dump/restore drops the decision trail) ----

/// The audit trail survives a dump → rebuild: every adoption audit row is
/// still there, byte-for-byte, up to ordering. RED today: `ExtractionDump`
/// (src/store/sqlite.rs:375-427) has no `audits` field and restore writes
/// none, so the rebuilt store's `audit()` is empty.
///
/// The restage history rides along in the same rebuild: its observable
/// side effects (supersession tombstones, claim statuses) are asserted
/// here too. The `restages` table itself has no public reader, so its
/// rows are silently dropped with no observable API surface — that loss
/// is recorded in the bug ticket rather than as an assertion.
#[test]
#[ignore = "bug bajan-7n3: dump/restore drops audits/restages/evidence tables"]
fn dump_restore_preserves_audit_and_restage_trail() {
    let source = SqliteStore::open_in_memory().expect("open source");
    let (restaged, _adopted) = build_history(&source);

    let dump = source.dump_extraction_output().expect("dump");
    let rebuilt = SqliteStore::open_in_memory().expect("open rebuilt");
    rebuilt.restore_extraction_output(&dump).expect("restore");

    assert_eq!(
        audit_set(rebuilt.audit().expect("rebuilt audit")),
        audit_set(source.audit().expect("source audit")),
        "audit trail must survive the rebuild (bajan-7n3)"
    );

    // Restage side effects: tombstones and final statuses are carried by
    // the dump itself, so these should hold once the fix lands.
    let mut source_supersessions = source.supersessions().expect("source supersessions");
    source_supersessions.sort_by_key(|s| (s.claim_key, s.superseded_at));
    let mut rebuilt_supersessions = rebuilt.supersessions().expect("rebuilt supersessions");
    rebuilt_supersessions.sort_by_key(|s| (s.claim_key, s.superseded_at));
    assert_eq!(rebuilt_supersessions, source_supersessions);
    // The restaged claim's final status (back to `staged`) is carried by
    // the dump's claim node, so the rebuild reproduces it.
    let status_of = |db: &SqliteStore, key: usize| {
        db.claims_with_lineage()
            .expect("read")
            .into_iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, node, _)| node.status)
            .expect("claim exists")
    };
    assert_eq!(
        status_of(&rebuilt, restaged),
        status_of(&source, restaged),
        "restaged claim's final status must survive the rebuild"
    );
}

/// The evidence resolution state survives a dump → rebuild: the
/// reference (including `observed_tier` and scheme version) and the
/// append-only check history per claim. RED today: restore writes neither
/// `evidence_refs` nor `evidence_checks`, so the rebuilt store reports
/// no reference and no checks for a claim that was checked twice.
#[test]
#[ignore = "bug bajan-7n3: dump/restore drops audits/restages/evidence tables"]
fn dump_restore_preserves_evidence_trail() {
    let source = SqliteStore::open_in_memory().expect("open source");
    let (_, adopted) = build_history(&source);

    let dump = source.dump_extraction_output().expect("dump");
    let rebuilt = SqliteStore::open_in_memory().expect("open rebuilt");
    rebuilt.restore_extraction_output(&dump).expect("restore");

    assert_eq!(
        rebuilt.evidence_ref(adopted).expect("rebuilt ref"),
        source.evidence_ref(adopted).expect("source ref"),
        "evidence reference must survive the rebuild (bajan-7n3)"
    );
    assert_eq!(
        rebuilt.evidence_checks(adopted).expect("rebuilt checks"),
        source.evidence_checks(adopted).expect("source checks"),
        "append-only evidence check history must survive the rebuild"
    );
}

// ---- live control (predicted green): the graph itself survives ----

/// Control: for generated 1..=10 mixed staged/active claims, dump →
/// restore into a fresh store preserves claims (with lineage), edges,
/// invalidation proposals, and supersession tombstones up to ordering.
/// This isolates the missing-decision-trail failure (the red tests above)
/// from general restore breakage: if this control goes red, the defect is
/// bigger than the four dropped tables.
#[hegel::test]
fn dump_restore_preserves_the_graph(tc: hegel::TestCase) {
    let count = tc.draw(gs::integers::<i64>().min_value(1).max_value(10));

    let source = SqliteStore::open_in_memory().expect("open source");
    let mut keys: Vec<usize> = Vec::new();
    for i in 0..count {
        // Mixed statuses; claim text bounded so no claim balloons.
        let text = tc.draw(gs::text().max_size(64));
        let status_word = tc.draw(gs::sampled_from(&["staged", "active"][..]));
        let status = if status_word == "active" {
            ClaimStatus::Active
        } else {
            ClaimStatus::Staged
        };
        keys.push(seed(&source, &format!("ep-{i}"), &text, status));
    }
    if keys.len() >= 2 {
        source
            .insert_edge(keys[0], EdgeLabel::Contradicts, keys[1])
            .expect("insert edge");
    }
    let nodes: Vec<(usize, ClaimNode, Vec<Lineage>)> = source.claims_with_lineage().expect("read");
    for &key in &keys {
        let node = nodes
            .iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, node, _)| node)
            .expect("claim exists");
        match node.status {
            ClaimStatus::Active => {
                source
                    .stage_invalidation_proposal(key, "ep-0")
                    .expect("proposal on active claim");
            }
            ClaimStatus::Staged => {
                source
                    .supersede_prior_versions(&node_lineage_episode(&source, key), "0.2.0", 10_000)
                    .expect("supersede staged claim");
            }
            ClaimStatus::Rejected => {}
        }
    }

    let dump = source.dump_extraction_output().expect("dump");
    let rebuilt = SqliteStore::open_in_memory().expect("open rebuilt");
    rebuilt.restore_extraction_output(&dump).expect("restore");

    // Claims come back in key order from both stores.
    assert_eq!(
        rebuilt.claims_with_lineage().expect("rebuilt claims"),
        source.claims_with_lineage().expect("source claims"),
        "claim graph must survive the rebuild"
    );
    let mut source_edges = source.edges().expect("source edges");
    source_edges.sort_by_key(|e| (e.0, label_rank(e.1), e.2));
    let mut rebuilt_edges = rebuilt.edges().expect("rebuilt edges");
    rebuilt_edges.sort_by_key(|e| (e.0, label_rank(e.1), e.2));
    assert_eq!(rebuilt_edges, source_edges, "edges must survive (as a set)");
    let mut source_invalidations = source.invalidations().expect("source proposals");
    source_invalidations.sort_by(|a, b| {
        (&a.claim_key, &a.causing_episode_id).cmp(&(&b.claim_key, &b.causing_episode_id))
    });
    let mut rebuilt_invalidations = rebuilt.invalidations().expect("rebuilt proposals");
    rebuilt_invalidations.sort_by(|a, b| {
        (&a.claim_key, &a.causing_episode_id).cmp(&(&b.claim_key, &b.causing_episode_id))
    });
    assert_eq!(
        rebuilt_invalidations, source_invalidations,
        "invalidation proposals must survive (as a set)"
    );
    let mut source_supersessions = source.supersessions().expect("source tombstones");
    source_supersessions.sort_by_key(|s| (s.claim_key, s.superseded_at));
    let mut rebuilt_supersessions = rebuilt.supersessions().expect("rebuilt tombstones");
    rebuilt_supersessions.sort_by_key(|s| (s.claim_key, s.superseded_at));
    assert_eq!(
        rebuilt_supersessions, source_supersessions,
        "supersession tombstones must survive (as a set)"
    );
}

/// Stable ordering code for `EdgeLabel` (the enum derives no `Ord`; this
/// is a set-comparison aid only, not a vocabulary claim).
fn label_rank(label: EdgeLabel) -> u8 {
    match label {
        EdgeLabel::Mentions => 0,
        EdgeLabel::Contradicts => 1,
        EdgeLabel::Supports => 2,
        EdgeLabel::DerivedFrom => 3,
        EdgeLabel::PossibleDuplicateOf => 4,
    }
}

/// The episode id behind a claim's first lineage edge (the supersession
/// writer targets (episode, version) provenance, so the probe needs it).
fn node_lineage_episode(db: &SqliteStore, claim_key: usize) -> String {
    db.lineage_of(claim_key)
        .expect("lineage")
        .into_iter()
        .next()
        .expect("claim has a lineage edge")
        .episode_id
}
