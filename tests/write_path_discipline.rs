//! Purpose: red-first tests for bajan-2hp — write-path discipline on the
//! graph-edge and invalidation-proposal producers.
//! Responsibilities: uniqueness per relation — `insert_edge` refuses an
//! identical (from, label, to) triple (`gm_relation_typing`),
//! `stage_invalidation_proposal` refuses an identical
//! (claim, causing-episode) pair and stages against active claims only
//! (`ex_mutation_proposal`); dump-recreate stays faithful — rows the
//! pipeline would refuse are restored verbatim (`gm_embedded_store`);
//! the contradiction read bounds BOTH walks under one budget
//! (`qt_bounded_traversal`).
//! Rationale: a doubled contradicts edge doubles the reported pairs and a
//! doubled possible_duplicate_of entry would double er_queue rows — the
//! write path is the place where the graph's set semantics are enforced;
//! rebuild semantics stay raw so an honest dump of a legacy store is
//! never silently rewritten on recreate.

use bajan::ingest::{EpisodeRecord, Locator, SourceMeta};
use bajan::query::{self, BudgetStatus};
use bajan::store::sqlite::{DumpedEdge, ExtractionDump, SqliteStore};
use bajan::store::{
    ClaimNode, ClaimStatus, EdgeLabel, Evidence, InvalidationProposal, Lineage, StoreError,
};

fn episode_with(id: &str) -> EpisodeRecord {
    EpisodeRecord {
        id: id.into(),
        text: "alpha beta gamma delta epsilon zeta.".into(),
        locator: Locator::Span("heading:Notes".into()),
        source: SourceMeta {
            source_type: "episode".into(),
            data_cutoff: None,
            authority_tier: 1,
            tags: vec!["workspace:dev".into()],
        },
    }
}

fn claim(text: &str, episode_id: &str, status: ClaimStatus) -> (ClaimNode, Lineage) {
    (
        ClaimNode {
            text: text.into(),
            valid_at: None,
            invalid_at: None,
            data_cutoff: None,
            status,
            scope: "workspace:dev".into(),
            source_type: "episode".into(),
            evidence: Evidence::Span {
                text: text.into(),
                locator: "heading:Notes".into(),
            },
        },
        Lineage {
            episode_id: episode_id.into(),
            extractor_version: "0.1.0".into(),
        },
    )
}

/// Seed an episode and a claim backed by it; returns the claim key.
fn seed(db: &SqliteStore, id: &str, text: &str, status: ClaimStatus) -> usize {
    let ep = episode_with(id);
    bajan::ingest::persist(db, &ep).expect("persist");
    let (node, lineage) = claim(text, id, status);
    db.insert_claim(&node, &[lineage], &ep.text)
        .expect("insert")
}

// gm_relation_typing: edges are a set relation — inserting the identical
// (from, label, to) triple twice is refused; the reversed direction and
// a different label between the same pair remain distinct relations.
#[test]
fn duplicate_edge_refuses_identical_triple_only() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, "ep-a", "alpha one", ClaimStatus::Active);
    let b = seed(&db, "ep-b", "alpha two", ClaimStatus::Active);
    db.insert_edge(a, EdgeLabel::Contradicts, b)
        .expect("first insert");

    let dup = db.insert_edge(a, EdgeLabel::Contradicts, b).unwrap_err();
    assert!(
        matches!(dup, StoreError::DuplicateEdge { .. }),
        "duplicate triple refused with a typed error, got: {dup:?}"
    );
    assert_eq!(
        db.edges().expect("read").len(),
        1,
        "the refused insert wrote no row"
    );

    // Distinct relations stay distinct: reversed direction, other labels.
    db.insert_edge(b, EdgeLabel::Contradicts, a)
        .expect("reversed direction is a different triple");
    db.insert_edge(a, EdgeLabel::Mentions, b)
        .expect("a different label is a different relation");
    assert_eq!(db.edges().expect("read").len(), 3);
}

// ex_mutation_proposal: proposals are a set relation — staging the
// identical (claim, causing-episode) pair twice is refused.
#[test]
fn duplicate_proposal_refuses_identical_pair_only() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, "ep-a", "alpha one", ClaimStatus::Active);
    db.stage_invalidation_proposal(a, "ep-b")
        .expect("first staging");

    let dup = db.stage_invalidation_proposal(a, "ep-b").unwrap_err();
    assert!(
        matches!(dup, StoreError::DuplicateProposal { .. }),
        "duplicate pair refused with a typed error, got: {dup:?}"
    );
    assert_eq!(
        db.invalidations().expect("read").len(),
        1,
        "the refused staging wrote no row"
    );

    // A different causing episode is a different proposal.
    db.stage_invalidation_proposal(a, "ep-c")
        .expect("new evidence, new proposal");
    assert_eq!(db.invalidations().expect("read").len(), 2);
}

// ex_mutation_proposal: proposals stage against ACTIVE claims — a
// staged claim is not yet settled, a rejected claim is tombstoned;
// neither may be targeted by an invalidation proposal.
#[test]
fn proposal_stages_against_active_claims_only() {
    let db = SqliteStore::open_in_memory().expect("open");
    let staged = seed(&db, "ep-a", "alpha staged text", ClaimStatus::Staged);
    let rejected = seed(&db, "ep-b", "alpha rejected text", ClaimStatus::Rejected);
    let active = seed(&db, "ep-c", "alpha active text", ClaimStatus::Active);

    let refused = db.stage_invalidation_proposal(staged, "ep-d").unwrap_err();
    assert!(
        matches!(
            refused,
            StoreError::ProposalRefused {
                current: ClaimStatus::Staged,
                ..
            }
        ),
        "staged target refused with a typed error, got: {refused:?}"
    );
    let refused = db
        .stage_invalidation_proposal(rejected, "ep-d")
        .unwrap_err();
    assert!(
        matches!(
            refused,
            StoreError::ProposalRefused {
                current: ClaimStatus::Rejected,
                ..
            }
        ),
        "rejected target refused with a typed error, got: {refused:?}"
    );
    assert!(db.invalidations().expect("read").is_empty());

    db.stage_invalidation_proposal(active, "ep-d")
        .expect("active target stages");
    assert_eq!(db.invalidations().expect("read").len(), 1);
}

// gm_embedded_store: dump-recreate is raw — rows the pipeline would
// refuse (duplicates, proposals against non-active claims from a legacy
// store) are restored verbatim; the guards are pipeline semantics, not
// rebuild semantics.
#[test]
fn dump_recreate_keeps_rows_the_pipeline_refuses() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, "ep-a", "alpha one", ClaimStatus::Active);
    let b = seed(&db, "ep-b", "alpha two", ClaimStatus::Staged);
    db.insert_edge(a, EdgeLabel::Contradicts, b)
        .expect("first insert");

    let mut dump: ExtractionDump = db.dump_extraction_output().expect("dump read");
    // A legacy store's duplicates and a proposal against a staged claim.
    dump.edges.push(DumpedEdge {
        from_claim: a,
        label: EdgeLabel::Contradicts,
        to_claim: b,
        // A legacy row predating the provenance column: absent.
        provenance: None,
    });
    dump.invalidations.push(InvalidationProposal {
        claim_key: b,
        causing_episode_id: "ep-x".into(),
    });

    let rebuilt = SqliteStore::open_in_memory().expect("rebuild open");
    rebuilt.restore_extraction_output(&dump).expect("restore");
    assert_eq!(
        rebuilt.edges().expect("read").len(),
        2,
        "duplicate edge row restored verbatim"
    );
    assert_eq!(
        rebuilt.invalidations().expect("read").len(),
        1,
        "proposal against a non-active claim restored verbatim"
    );
}

// qt_bounded_traversal: the budget bounds BOTH contradiction walks —
// edges first, then proposals, one shared counter. A proposal never
// walked is a proposal never reported, and the status says so.
#[test]
fn budget_bounds_the_proposal_walk() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, "ep-a", "alpha one", ClaimStatus::Active);
    let b = seed(&db, "ep-b", "alpha two", ClaimStatus::Active);
    db.insert_edge(a, EdgeLabel::Contradicts, b).expect("edge");
    db.stage_invalidation_proposal(b, "ep-c").expect("proposal");
    db.stage_invalidation_proposal(b, "ep-d").expect("proposal");

    // 1 edge row + 2 proposal rows = 3 walked rows in total.
    let result = query::contradictions(&db, &[a, b], 3).expect("contradictions");
    assert!(matches!(result.status, BudgetStatus::Complete));
    assert_eq!(result.pairs.len(), 1);
    assert_eq!(result.proposals.len(), 2);

    // Budget covers the edge and one proposal; the second proposal is
    // unseen — budget-exhausted, one proposal reported.
    let result = query::contradictions(&db, &[a, b], 2).expect("contradictions");
    assert!(matches!(result.status, BudgetStatus::BudgetExhausted));
    assert_eq!(result.pairs.len(), 1);
    assert_eq!(
        result.proposals.len(),
        1,
        "the walked proposal is reported, the unwalked one is not"
    );

    // Budget exhausted during the edge walk: the proposal walk never
    // ran — no proposals are reported and the status says why.
    let result = query::contradictions(&db, &[a, b], 1).expect("contradictions");
    assert!(matches!(result.status, BudgetStatus::BudgetExhausted));
    assert_eq!(result.pairs.len(), 1);
    assert!(
        result.proposals.is_empty(),
        "proposals behind an exhausted edge walk are never reported"
    );
}

// qt_bounded_traversal: an exhausted budget with nothing left to walk is
// still honest-complete — the shared counter only reports exhaustion
// when rows remained unseen.
#[test]
fn budget_at_least_the_walk_is_complete() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, "ep-a", "alpha one", ClaimStatus::Active);
    let b = seed(&db, "ep-b", "alpha two", ClaimStatus::Active);
    db.insert_edge(a, EdgeLabel::Contradicts, b).expect("edge");

    let result = query::contradictions(&db, &[a, b], 2).expect("contradictions");
    assert!(
        matches!(result.status, BudgetStatus::Complete),
        "one edge row + zero proposal rows fit a budget of 2"
    );
    assert_eq!(result.pairs.len(), 1);
    assert!(result.proposals.is_empty());
}
