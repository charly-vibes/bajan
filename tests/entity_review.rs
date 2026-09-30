//! Purpose: property + scenario tests for the entity-review HITL queue
//! (bajan-6j1) — deterministic-ER candidate proposals, human-only
//! resolution, lineage-union merges, edge-only rejections, the repropose
//! loop guard, and queue transparency.
//! Responsibilities: red-first derivation of specs/entity-review.md on the
//! wired SQLite path (`er_queue_entry`, `er_human_resolution`,
//! `er_merge_preserves`, `er_reject_drops_edge`, `er_repropose_guard`,
//! `er_queue_transparency`) plus the dump-recreate durability of the
//! queue state (`gm_embedded_store`) and the CLI surface.
//! Rationale: the queue is the only HITL surface in an otherwise AFK
//! pipeline — every test here guards the boundary between the machine's
//! uncertainty (proposals) and the human's decision (resolutions).

use bajan::ingest::{EpisodeRecord, Locator, SourceMeta};
use bajan::query;
use bajan::store::sqlite::SqliteStore;
use bajan::store::{
    ClaimNode, ClaimStatus, Evidence, Lineage, ReviewDecision, ReviewStatus, StoreError,
};
use bajan::{ingest, review};
use proptest::prelude::*;

fn episode(id: &str, text: &str, tier: u8, cutoff: Option<&str>) -> EpisodeRecord {
    EpisodeRecord {
        id: id.into(),
        text: text.into(),
        locator: Locator::Span("heading:Notes".into()),
        source: SourceMeta {
            source_type: "episode".into(),
            data_cutoff: cutoff.map(|c| c.to_string()),
            authority_tier: tier,
            tags: vec!["workspace:dev".into()],
        },
    }
}

/// Seed an episode and one claim backed by the given lineage rows.
fn seed(db: &SqliteStore, ep: &EpisodeRecord, text: &str, lineage: &[Lineage]) -> usize {
    ingest::persist(db, ep).expect("persist");
    let node = ClaimNode {
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
    };
    db.insert_claim(&node, lineage, &ep.text).expect("insert")
}

/// Seed one claim backed by two persisted episodes (multi-lineage).
fn seed_two(db: &SqliteStore, ep1: &EpisodeRecord, ep2: &EpisodeRecord, text: &str) -> usize {
    ingest::persist(db, ep1).expect("persist");
    ingest::persist(db, ep2).expect("persist");
    let node = ClaimNode {
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
    };
    db.insert_claim(
        &node,
        &[
            Lineage {
                episode_id: ep1.id.clone(),
                extractor_version: "0.1.0".into(),
            },
            Lineage {
                episode_id: ep2.id.clone(),
                extractor_version: "0.1.0".into(),
            },
        ],
        &ep1.text,
    )
    .expect("insert")
}

fn lineage_of(db: &SqliteStore, key: usize) -> Vec<Lineage> {
    db.claims_with_lineage()
        .expect("read")
        .into_iter()
        .find(|(k, _, _)| *k == key)
        .expect("claim exists")
        .2
}

// er_queue_entry: candidates enter only as possible_duplicate_of edges
// written by the deterministic pass; the queue equals the open proposal
// edges; duplicate open proposals and unknown endpoints are refused.
proptest! {
    #[test]
    fn p_queue_entry(
        text_a in "[a-z ]{5,40}",
        text_b in "[a-z ]{5,40}",
    ) {
        let db = SqliteStore::open_in_memory().expect("open");
        let ep1 = episode("ep1", "alpha body", 1, None);
        let a = seed(&db, &ep1, &text_a, &[Lineage { episode_id: "ep1".into(), extractor_version: "0.1.0".into() }]);
        let ep2 = episode("ep2", "beta body", 1, None);
        let b = seed(&db, &ep2, &text_b, &[Lineage { episode_id: "ep2".into(), extractor_version: "0.1.0".into() }]);
        // A deliberate duplicate of a — the deterministic pass must find it.
        let ep3 = episode("ep3", "gamma body", 1, None);
        let dup = seed(&db, &ep3, &text_a, &[Lineage { episode_id: "ep3".into(), extractor_version: "0.1.0".into() }]);

        let report = review::run_er_pass(&db, 256, 1_000_000).expect("pass");
        let queue = db.review_queue().expect("queue");

        // Queue contents equal the open possible_duplicate_of edges.
        let edges: Vec<(usize, usize)> = db
            .edges()
            .expect("edges")
            .into_iter()
            .filter(|(_, l, _)| *l == bajan::store::EdgeLabel::PossibleDuplicateOf)
            .map(|(f, _, t)| (f, t))
            .collect();
        prop_assert_eq!(queue.len(), edges.len(), "queue = open proposal edges");
        for (from, to) in &edges {
            prop_assert!(queue.iter().any(|r| r.from_claim == *from && r.to_claim == *to),
                "every edge has a queue row");
        }
        for r in &queue {
            prop_assert!(edges.contains(&(r.from_claim, r.to_claim)),
                "every queue row traces to an edge");
            prop_assert_ne!(r.from_claim, r.to_claim, "never a self-pair");
            prop_assert!(!r.evidence.is_empty(), "evidence recorded");
        }
        // The duplicate pair was found (unordered).
        prop_assert!(queue.iter().any(|r| {
            (r.from_claim == a && r.to_claim == dup) || (r.from_claim == dup && r.to_claim == a)
        }), "duplicate pair proposed");
        // Two non-duplicate claims are not proposed together (unless the
        // random texts happen to be equivalence/containment related).
        let norm_b = review::normalize(&text_b);
        let equivalent = review::normalize(&text_a) == norm_b
            || review::normalize(&text_a).contains(&norm_b)
            || norm_b.contains(&review::normalize(&text_a));
        if !equivalent {
            prop_assert!(
                !queue
                    .iter()
                    .any(|r| {
                        r.from_claim == std::cmp::min(a, b)
                            && r.to_claim == std::cmp::max(a, b)
                    }),
                "non-equivalent pair not proposed"
            );
        }
        prop_assert!(report.proposed >= 1, "the duplicate was proposed");

        // A second pass over the unchanged store proposes nothing new.
        let again = review::run_er_pass(&db, 256, 1_000_001).expect("pass");
        prop_assert_eq!(again.proposed, 0, "idempotent over unchanged inputs");

        // enqueue refuses an already-open pair.
        let outcome = db
            .enqueue_candidate(a, dup, "normalization-equal", "{}", 1_000_002)
            .expect("call");
        prop_assert!(matches!(outcome, review::EnqueueOutcome::AlreadyProposed));

        // Unknown endpoints are refused.
        let err = db
            .enqueue_candidate(a, 999, "normalization-equal", "{}", 1_000_002)
            .expect_err("unknown endpoint refused");
        let is_claim_not_found = matches!(err, StoreError::ClaimNotFound { .. });
        prop_assert!(is_claim_not_found);
        // A self-pair is refused.
        let err = db
            .enqueue_candidate(a, a, "normalization-equal", "{}", 1_000_002)
            .expect_err("self-pair refused");
        let is_invalid_pair = matches!(err, StoreError::InvalidReviewPair { .. });
        prop_assert!(is_invalid_pair);
    }
}

// er_human_resolution: only explicit human commands with actor identity
// resolve a candidate; resolutions without actor are refused; a resolved
// candidate cannot resolve again.
proptest! {
    #[test]
    fn p_human_resolution(
        text in "[a-z ]{5,40}",
        actor in "[a-z]{3,10}",
    ) {
        let db = SqliteStore::open_in_memory().expect("open");
        let ep1 = episode("ep1", "alpha body", 1, None);
        let a = seed(&db, &ep1, &text, &[Lineage { episode_id: "ep1".into(), extractor_version: "0.1.0".into() }]);
        let ep2 = episode("ep2", "beta body", 1, None);
        let b = seed(&db, &ep2, &text, &[Lineage { episode_id: "ep2".into(), extractor_version: "0.1.0".into() }]);
        review::run_er_pass(&db, 256, 1_000_000).expect("pass");
        prop_assert_eq!(db.review_queue().expect("queue").len(), 1);

        // Without actor identity: refused.
        let err = db
            .resolve_review(a, b, ReviewDecision::Approved, "", 1_000_005)
            .expect_err("actor required");
        let actor_required = matches!(err, StoreError::ReviewActorRequired { .. });
        prop_assert!(actor_required);

        // With actor: resolves; the queue drains.
        db.resolve_review(a, b, ReviewDecision::Approved, &actor, 1_000_005)
            .expect("resolve");
        prop_assert!(db.review_queue().expect("queue").is_empty());

        // Already-resolved: refused.
        let err = db
            .resolve_review(a, b, ReviewDecision::Rejected, &actor, 1_000_006)
            .expect_err("already resolved");
        let not_proposed = matches!(err, StoreError::ReviewNotProposed { .. });
        prop_assert!(not_proposed);
    }
}

// er_merge_preserves: an approved merge preserves the union of both
// sides' lineage on the merged entity and writes exactly one audit
// record — actor, timestamp, both entity ids, decision.
proptest! {
    #[test]
    fn p_merge_preserves(actor in "[a-z]{3,10}") {
        let db = SqliteStore::open_in_memory().expect("open");
        // Two supporting episodes for a, one for b, one unrelated claim.
        let ep1 = episode("ep1", "alpha body", 1, None);
        let ep2 = episode("ep2", "alpha body", 2, None);
        let a = seed_two(&db, &ep1, &ep2, "the same content");
        let ep3 = episode("ep3", "the same content", 1, None);
        let b = seed(&db, &ep3, "the same content", &[Lineage { episode_id: "ep3".into(), extractor_version: "0.1.0".into() }]);
        let ep4 = episode("ep4", "unrelated body", 1, None);
        let _c = seed(&db, &ep4, "unrelated body", &[Lineage { episode_id: "ep4".into(), extractor_version: "0.1.0".into() }]);

        review::run_er_pass(&db, 256, 1_000_000).expect("pass");
        db.resolve_review(a, b, ReviewDecision::Approved, &actor, 1_000_005)
            .expect("approve");

        // The merged entity's lineage is the union of both sides' pre-merge
        // lineage: every episode either side derived from survives on it.
        let survivor = lineage_of(&db, a);
        let episodes: std::collections::BTreeSet<&str> =
            survivor.iter().map(|l| l.episode_id.as_str()).collect();
        prop_assert!(episodes.contains("ep1"), "survivor's own lineage survives");
        prop_assert!(episodes.contains("ep2"), "survivor's second lineage survives");
        prop_assert!(episodes.contains("ep3"), "absorbed side's lineage survives");
        prop_assert_eq!(episodes.len(), 3, "union, not overwrite");

        // Exactly one audit record: actor, timestamp, both ids, decision.
        let audits = db.review_audits().expect("audits");
        prop_assert_eq!(audits.len(), 1);
        prop_assert_eq!(audits[0].from_claim, a);
        prop_assert_eq!(audits[0].to_claim, b);
        prop_assert_eq!(audits[0].decision, ReviewDecision::Approved);
        prop_assert_eq!(audits[0].actor.clone(), actor);
        prop_assert_eq!(audits[0].resolved_at, 1_000_005);
    }
}

// er_reject_drops_edge: after rejection the possible_duplicate_of edge is
// gone and everything else — entities, lineage, statuses — is unchanged;
// the audit trail gains one rejection record.
proptest! {
    #[test]
    fn p_reject_drops_edge(actor in "[a-z]{3,10}") {
        let db = SqliteStore::open_in_memory().expect("open");
        let ep1 = episode("ep1", "alpha body", 1, None);
        let a = seed(&db, &ep1, "identical text", &[Lineage { episode_id: "ep1".into(), extractor_version: "0.1.0".into() }]);
        let ep2 = episode("ep2", "beta body", 1, None);
        let b = seed(&db, &ep2, "identical text", &[Lineage { episode_id: "ep2".into(), extractor_version: "0.1.0".into() }]);

        review::run_er_pass(&db, 256, 1_000_000).expect("pass");
        let before_claims = db.claims_with_lineage().expect("claims");
        let before_lineage_a = lineage_of(&db, a);
        let before_lineage_b = lineage_of(&db, b);
        prop_assert!(db
            .edges()
            .expect("edges")
            .iter()
            .any(|(_, l, _)| *l == bajan::store::EdgeLabel::PossibleDuplicateOf));

        db.resolve_review(a, b, ReviewDecision::Rejected, &actor, 1_000_005)
            .expect("reject");

        // The proposal edge is gone.
        prop_assert!(!db
            .edges()
            .expect("edges")
            .iter()
            .any(|(_, l, _)| *l == bajan::store::EdgeLabel::PossibleDuplicateOf));
        // Entities, lineage, and statuses are untouched.
        let after_claims = db.claims_with_lineage().expect("claims");
        prop_assert_eq!(before_claims, after_claims);
        prop_assert_eq!(before_lineage_a, lineage_of(&db, a));
        prop_assert_eq!(before_lineage_b, lineage_of(&db, b));
        // One rejection audit record.
        let audits = db.review_audits().expect("audits");
        prop_assert_eq!(audits.len(), 1);
        prop_assert_eq!(audits[0].decision, ReviewDecision::Rejected);
        prop_assert_eq!(audits[0].actor.clone(), actor);
        prop_assert_eq!(audits[0].resolved_at, 1_000_005);
    }
}

// er_repropose_guard: a rejected pair re-enters only with new evidence —
// unchanged-input re-runs never re-queue it, changed evidence re-queues
// it, and the prior rejection record is never erased.
#[test]
fn repropose_guard_requires_new_evidence() {
    let db = SqliteStore::open_in_memory().expect("open");
    let ep1 = episode("ep1", "alpha body", 1, None);
    let a = seed(
        &db,
        &ep1,
        "identical text",
        &[Lineage {
            episode_id: "ep1".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    let ep2 = episode("ep2", "beta body", 1, None);
    let b = seed(
        &db,
        &ep2,
        "identical text",
        &[Lineage {
            episode_id: "ep2".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    review::run_er_pass(&db, 256, 1_000_000).expect("pass");
    db.resolve_review(a, b, ReviewDecision::Rejected, "alice", 1_000_005)
        .expect("reject");
    assert!(db.review_queue().expect("queue").is_empty());

    // Re-running the deterministic pass on unchanged inputs: refused.
    let report = review::run_er_pass(&db, 256, 1_000_010).expect("pass");
    assert_eq!(report.refused, 1, "unchanged re-run refused");
    assert!(db.review_queue().expect("queue").is_empty());
    assert_eq!(
        db.review_audits().expect("audits").len(),
        1,
        "rejection intact"
    );

    // With new evidence (changed fingerprint): the pair re-enters the
    // queue as a fresh proposal; the prior rejection record stays.
    let new_fp = r#"{"texts":["identical text","identical text"],"episodes":["ep1","ep2","ep9"]}"#
        .to_string();
    let outcome = db
        .enqueue_candidate(a, b, "new-mention", &new_fp, 1_000_020)
        .expect("call");
    assert!(matches!(outcome, review::EnqueueOutcome::Queued));
    assert_eq!(db.review_queue().expect("queue").len(), 1);
    let audits = db.review_audits().expect("audits");
    assert_eq!(audits.len(), 1, "prior rejection record not erased");
    let all = db.reviews().expect("reviews");
    assert_eq!(all.len(), 2, "old rejected row + new proposed row");
    assert!(
        all.iter()
            .any(|r| r.status == ReviewStatus::ResolvedRejected)
    );
    assert!(all.iter().any(|r| r.status == ReviewStatus::Proposed));
}

// er_queue_transparency: the list returns every open candidate with
// evidence, episode counts, and queue age, ordered oldest-first;
// resolved candidates never reappear.
#[test]
fn queue_list_is_transparent_and_oldest_first() {
    let db = SqliteStore::open_in_memory().expect("open");
    let ep1 = episode("ep1", "alpha body", 1, None);
    let a = seed(
        &db,
        &ep1,
        "first duplicate text",
        &[Lineage {
            episode_id: "ep1".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    let ep2 = episode("ep2", "beta body", 1, None);
    let b = seed(
        &db,
        &ep2,
        "first duplicate text",
        &[Lineage {
            episode_id: "ep2".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    let ep3 = episode("ep3", "gamma body", 1, None);
    let _c = seed(
        &db,
        &ep3,
        "second duplicate text",
        &[Lineage {
            episode_id: "ep3".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    let ep4 = episode("ep4", "delta body", 1, None);
    let _d = seed(
        &db,
        &ep4,
        "second duplicate text",
        &[Lineage {
            episode_id: "ep4".into(),
            extractor_version: "0.1.0".into(),
        }],
    );

    // Two passes at different times: the first pair queues first.
    review::run_er_pass(&db, 256, 1_000_000).expect("pass");
    db.resolve_review(a, b, ReviewDecision::Rejected, "alice", 1_000_005)
        .expect("reject");
    let fp = r#"{"texts":["first duplicate text","first duplicate text"],"episodes":["ep1","ep2","ep9"]}"#.to_string();
    db.enqueue_candidate(a, b, "new-mention", &fp, 1_000_020)
        .expect("repropose");
    review::run_er_pass(&db, 256, 1_000_030).expect("pass");

    let queue = db.review_queue().expect("queue");
    assert_eq!(queue.len(), 2, "both open candidates listed");
    assert!(
        queue[0].queued_at <= queue[1].queued_at,
        "oldest-first ordering"
    );
    for r in &queue {
        assert!(!r.evidence.is_empty(), "evidence visible");
        assert_eq!(r.status, ReviewStatus::Proposed);
    }
    // Episode counts per side are reported (claim counts for the pair).
    let counts = db.review_episode_counts(&queue).expect("counts");
    for (from_n, to_n) in counts {
        assert!(
            from_n >= 1 && to_n >= 1,
            "each side has supporting episodes"
        );
    }
    // Resolved candidates never reappear in the queue.
    assert!(
        queue
            .iter()
            .all(|r| (r.from_claim, r.to_claim) != (a, b) || r.queued_at == 1_000_020)
    );
}

// gm_embedded_store: queue rows and audit records survive dump-recreate.
#[test]
fn review_state_survives_dump_recreate() {
    let db = SqliteStore::open_in_memory().expect("open");
    let ep1 = episode("ep1", "alpha body", 1, None);
    let a = seed(
        &db,
        &ep1,
        "identical text",
        &[Lineage {
            episode_id: "ep1".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    let ep2 = episode("ep2", "beta body", 1, None);
    let b = seed(
        &db,
        &ep2,
        "identical text",
        &[Lineage {
            episode_id: "ep2".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    review::run_er_pass(&db, 256, 1_000_000).expect("pass");
    db.resolve_review(a, b, ReviewDecision::Rejected, "alice", 1_000_005)
        .expect("reject");
    let fp = r#"{"texts":["identical text","identical text"],"episodes":["ep1","ep2","ep9"]}"#
        .to_string();
    db.enqueue_candidate(a, b, "new-mention", &fp, 1_000_020)
        .expect("repropose");

    let dump = db.dump_extraction_output().expect("dump");
    let rebuilt = SqliteStore::open_in_memory().expect("open");
    rebuilt.restore_extraction_output(&dump).expect("restore");

    let before = (
        db.reviews().expect("reviews"),
        db.review_audits().expect("audits"),
    );
    let after = (
        rebuilt.reviews().expect("reviews"),
        rebuilt.review_audits().expect("audits"),
    );
    assert_eq!(before.0, after.0, "review rows reproduced");
    assert_eq!(before.1, after.1, "audit rows reproduced");
    assert_eq!(
        db.review_queue().expect("queue"),
        rebuilt.review_queue().expect("queue")
    );
}

// Wiring: the pass proposes only pairs whose normalized texts are equal
// or containment-related (deterministic scoring, no LLM/embedding).
#[test]
fn pass_uses_deterministic_normalization_only() {
    let db = SqliteStore::open_in_memory().expect("open");
    let ep1 = episode("ep1", "alpha body", 1, None);
    let _a = seed(
        &db,
        &ep1,
        "Alpha   Body",
        &[Lineage {
            episode_id: "ep1".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    let ep2 = episode("ep2", "beta body", 1, None);
    let _b = seed(
        &db,
        &ep2,
        "alpha body",
        &[Lineage {
            episode_id: "ep2".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    let ep3 = episode("ep3", "gamma body", 1, None);
    let _c = seed(
        &db,
        &ep3,
        "completely different",
        &[Lineage {
            episode_id: "ep3".into(),
            extractor_version: "0.1.0".into(),
        }],
    );

    let report = review::run_er_pass(&db, 256, 1_000_000).expect("pass");
    assert_eq!(report.proposed, 1, "whitespace/case normalization equal");
    let queue = db.review_queue().expect("queue");
    assert_eq!(queue[0].evidence, "normalization-equal");
}

// Bounded traversal: the pair walk is budget-bounded with honest
// exhaustion (qt_bounded_traversal discipline applied to the pass).
#[test]
fn pass_budget_is_honest() {
    let db = SqliteStore::open_in_memory().expect("open");
    let ep1 = episode("ep1", "alpha body", 1, None);
    let _a = seed(
        &db,
        &ep1,
        "text one",
        &[Lineage {
            episode_id: "ep1".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    let ep2 = episode("ep2", "beta body", 1, None);
    let _b = seed(
        &db,
        &ep2,
        "text two",
        &[Lineage {
            episode_id: "ep2".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    let ep3 = episode("ep3", "gamma body", 1, None);
    let _c = seed(
        &db,
        &ep3,
        "text three",
        &[Lineage {
            episode_id: "ep3".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    let ep4 = episode("ep4", "delta body", 1, None);
    let _d = seed(
        &db,
        &ep4,
        "text four",
        &[Lineage {
            episode_id: "ep4".into(),
            extractor_version: "0.1.0".into(),
        }],
    );

    // Budget 1: at most one pair examined — the walk must stop honestly.
    let report = review::run_er_pass(&db, 1, 1_000_000).expect("pass");
    assert_eq!(
        report.status,
        query::BudgetStatus::BudgetExhausted,
        "honest exhaustion, never a silent complete"
    );
}
// ---- CLI wiring (binary-level, global flags before the subcommand) ----

fn bajan(args: &[&str]) -> (i32, String) {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_bajan"))
        .args(args)
        .output()
        .expect("binary built by cargo test");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

/// Build a seeded store file with one duplicate pair and one unrelated
/// claim; returns (dir, db_path). The dir name is unique per call — tests
/// share one process, so the pid alone collides.
fn seeded_cli_db() -> (std::path::PathBuf, String) {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("bajan-6j1-cli-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let db_path = dir.join("review.db");
    let db_str = db_path.to_str().expect("utf8").to_string();
    let db = SqliteStore::open(&db_str).expect("open");
    let ep1 = episode("ep1", "alpha body", 1, None);
    let _a = seed(
        &db,
        &ep1,
        "identical text",
        &[Lineage {
            episode_id: "ep1".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    let ep2 = episode("ep2", "beta body", 1, None);
    let _b = seed(
        &db,
        &ep2,
        "identical text",
        &[Lineage {
            episode_id: "ep2".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    let ep3 = episode("ep3", "gamma body", 1, None);
    let _c = seed(
        &db,
        &ep3,
        "different text entirely",
        &[Lineage {
            episode_id: "ep3".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    (dir, db_str)
}

// er_queue_entry via CLI: the pass proposes the duplicate; the queue
// lists it with evidence, episode counts, and age; a second pass is
// honest-refused (idempotent).
#[test]
fn er_pass_and_review_list_cli_end_to_end() {
    let (dir, db) = seeded_cli_db();
    let (code, stdout) = bajan(&["--db", &db, "er", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope");
    assert_eq!(v["ok"].as_bool(), Some(true));
    assert_eq!(v["data"]["proposed"].as_u64(), Some(1));

    let (code, stdout) = bajan(&["--db", &db, "review", "list", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope");
    let queue = v["data"]["queue"].as_array().expect("queue array");
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0]["evidence"].as_str(), Some("normalization-equal"));
    assert!(queue[0]["from_episodes"].as_u64().unwrap() >= 1);
    assert!(queue[0]["age_seconds"].is_u64(), "queue age visible");

    // Idempotent: the unchanged re-run proposes nothing new.
    let (_, stdout) = bajan(&["--db", &db, "er", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope");
    assert_eq!(v["data"]["proposed"].as_u64(), Some(0));

    let _ = std::fs::remove_dir_all(&dir);
}

// er_human_resolution via CLI: reject drops the proposal edge, claims and
// lineage untouched, one audit record; the queue drains.
#[test]
fn review_reject_cli_drops_only_the_edge() {
    let (dir, db) = seeded_cli_db();
    let _ = bajan(&["--db", &db, "er", "--json"]);
    let (code, stdout) = bajan(&["--db", &db, "review", "reject", "0", "1", "--json"]);
    assert_eq!(code, 0, "resolution with actor identity succeeds");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope");
    assert_eq!(v["data"]["decision"].as_str(), Some("rejected"));
    assert_eq!(
        v["data"]["actor"].as_str(),
        std::env::var("USER").ok().as_deref()
    );

    // Queue drains; edge gone; claims untouched.
    let (_, stdout) = bajan(&["--db", &db, "review", "list", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope");
    assert_eq!(v["data"]["queue"].as_array().map(|a| a.len()), Some(0));
    let store = SqliteStore::open(&db).expect("reopen");
    assert!(
        !store
            .edges()
            .expect("edges")
            .iter()
            .any(|(_, l, _)| *l == bajan::store::EdgeLabel::PossibleDuplicateOf)
    );
    assert_eq!(store.claims_with_lineage().expect("claims").len(), 3);

    let _ = std::fs::remove_dir_all(&dir);
}

// er_merge_preserves via CLI: approval merges lineage (union on the
// survivor) and writes one audit record.
#[test]
fn review_approve_cli_merges_lineage() {
    let (dir, db) = seeded_cli_db();
    let _ = bajan(&["--db", &db, "er", "--json"]);
    let (code, stdout) = bajan(&["--db", &db, "review", "approve", "0", "1", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope");
    assert_eq!(v["data"]["decision"].as_str(), Some("approved"));

    let store = SqliteStore::open(&db).expect("reopen");
    let lineage = store.claims_with_lineage().expect("claims");
    let survivor = lineage.iter().find(|(k, _, _)| *k == 0).expect("survivor");
    let episodes: std::collections::BTreeSet<&str> =
        survivor.2.iter().map(|l| l.episode_id.as_str()).collect();
    assert!(
        episodes.contains("ep1") && episodes.contains("ep2"),
        "union"
    );
    assert_eq!(store.review_audits().expect("audits").len(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

// er_queue_entry anti-goal: review commands never create candidates —
// there is no propose/create review subcommand; the attempt is an
// argument error envelope.
#[test]
fn review_commands_cannot_create_candidates() {
    let (code, stdout) = bajan(&["review", "propose", "0", "1"]);
    assert_ne!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope");
    assert_eq!(v["ok"].as_bool(), Some(false));
    assert_eq!(v["data"]["code"].as_str(), Some("argument_error"));
}

// bajan-9pm scale regression: at book scale (2k claims) the pass must
// COMPLETE within a modest budget — the old all-pairs walk (n*(n-1)/2
// comparisons, each charged to the budget) exhausted with zero pairs at
// any realistic budget. Candidate pre-filtering (normalization-equal
// groups + sound containment blocking) makes the pass usable; honest
// exhaustion semantics are unchanged (candidates remained unseen =
// exhausted, no candidates = complete).
#[test]
fn er_pass_completes_at_book_scale() {
    let db = SqliteStore::open_in_memory().expect("open");
    let n = 2_000usize;
    for i in 0..n {
        let ep = episode(&format!("ep{i}"), &format!("episode body {i}"), 1, None);
        // Distinct claim texts sharing only common filler words — the
        // unique numbered token keeps containment candidates empty.
        // Zero-padded: the 5-digit token yields a 4-gram unique to this
        // claim, so its rarest-gram posting is a singleton (distinctive
        // content is exactly what makes the containment blocking cheap —
        // real claims carry content words; unpadded small numbers would
        // be the degenerate all-filler case).
        let text = format!("claim {:05} concerns mechanism {:05} in the body", i, i);
        seed(
            &db,
            &ep,
            &text,
            &[Lineage {
                episode_id: format!("ep{i}"),
                extractor_version: "0.1.0".into(),
            }],
        );
    }
    // Three normalization-equal pairs among the noise.
    for i in 0..3 {
        let ep = episode(&format!("dup{i}"), &format!("duplicate body {i}"), 1, None);
        seed(
            &db,
            &ep,
            &format!("exact duplicate text {i}"),
            &[Lineage {
                episode_id: format!("dup{i}"),
                extractor_version: "0.1.0".into(),
            }],
        );
        let ep2 = episode(&format!("dup{i}b"), &format!("duplicate body {i}"), 1, None);
        let k = seed(
            &db,
            &ep2,
            &format!("  EXACT   duplicate text {i} "),
            &[Lineage {
                episode_id: format!("dup{i}b"),
                extractor_version: "0.1.0".into(),
            }],
        );
        assert!(k > 0);
    }
    // One containment pair: the short claim's text is contained in the
    // long claim's text.
    let ep = episode("epshort", "short body", 1, None);
    let short = seed(
        &db,
        &ep,
        "alpha beta",
        &[Lineage {
            episode_id: "epshort".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    let ep2 = episode("eplong", "long body", 1, None);
    let _long = seed(
        &db,
        &ep2,
        "alpha beta gamma detail 777",
        &[Lineage {
            episode_id: "eplong".into(),
            extractor_version: "0.1.0".into(),
        }],
    );

    // Modest budget: covers the ranked-candidate walk (2006 claims) plus
    // the handful of candidate comparisons. The old all-pairs walk would
    // have needed ~2M comparisons and reported budget-exhausted.
    let report = review::run_er_pass(&db, 2_100, 1_000_000).expect("pass");
    assert_eq!(
        report.status,
        query::BudgetStatus::Complete,
        "book-scale pass completes within a modest budget"
    );
    assert_eq!(report.proposed, 4, "3 equal pairs + 1 contained pair");
    assert_eq!(report.refused, 0);
    let queue = db.review_queue().expect("queue");
    assert_eq!(queue.len(), 4);
    assert!(
        queue.iter().any(|r| r.evidence == "normalization-contains"
            && (r.from_claim == short || r.to_claim == short)),
        "containment pair proposed with the short claim as suspect"
    );
}

// bajan-9pm soundness lock: the rarest-gram blocking must never miss a
// true containment — even when the short text occurs only MID-TOKEN in
// the longer one ("audit" inside "preaudits"), which a naive
// token-equality posting would miss.
#[test]
fn er_containment_found_when_shorter_text_embeds_mid_token() {
    let db = SqliteStore::open_in_memory().expect("open");
    let ep1 = episode("ep1", "alpha body", 1, None);
    let short = seed(
        &db,
        &ep1,
        "audit",
        &[Lineage {
            episode_id: "ep1".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    let ep2 = episode("ep2", "beta body", 1, None);
    let long = seed(
        &db,
        &ep2,
        "preaudits 191 filings",
        &[Lineage {
            episode_id: "ep2".into(),
            extractor_version: "0.1.0".into(),
        }],
    );
    let report = review::run_er_pass(&db, 64, 1_000_000).expect("pass");
    assert_eq!(report.status, query::BudgetStatus::Complete);
    assert_eq!(report.proposed, 1, "mid-token containment found");
    assert_eq!(report.pairs, vec![(short, long)]);
    assert_eq!(
        db.review_queue().expect("queue")[0].evidence,
        "normalization-contains"
    );
}
