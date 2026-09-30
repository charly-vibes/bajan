//! Purpose: property + scenario tests for `qt_contradiction_query`
//! (bajan-0zo) — contradict-pair traversal plus staged invalidation
//! proposals over the persisted claim graph.
//! Responsibilities: red-first derivation of specs/query-tools.md
//! `p_contradiction_query` — graphs with contradict pairs, staged
//! proposals, and unconnected claims.
//! Rationale: the contradiction query is a traversal of `contradicts`
//! edges (published vocabulary, `gm_relation_typing`) plus the
//! invalidation proposals staged against the queried claims
//! (`ex_mutation_proposal`) — the graph says what conflicts, the human
//! says what's true: the query reports structure only, never
//! adjudicates (qt_contradiction_query). Every read is bounded and
//! honest (qt_bounded_traversal), lineage-traceable (qt_lineage_traceable)
//! and read-only (qt_readonly). Edges and proposals persist as plain
//! rows and survive dump-recreate (gm_embedded_store).

use bajan::ingest::{EpisodeRecord, Locator, SourceMeta};
use bajan::query::{self, BudgetStatus};
use bajan::store::sqlite::SqliteStore;
use bajan::store::{ClaimNode, ClaimStatus, EdgeLabel, Evidence, Lineage};
use proptest::prelude::*;

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

// qt_contradiction_query: the pairs are exactly the contradicts edges
// joined over the queried claims — nothing adjudicated, nothing dropped.
#[test]
fn pairs_are_exactly_the_contradicts_edges() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, "ep-a", "alpha one", ClaimStatus::Active);
    let b = seed(&db, "ep-b", "alpha two", ClaimStatus::Staged);
    let c = seed(&db, "ep-c", "alpha three", ClaimStatus::Active);
    db.insert_edge(a, EdgeLabel::Contradicts, b).expect("edge");
    db.insert_edge(c, EdgeLabel::Contradicts, b).expect("edge");

    let result = query::contradictions(&db, &[a], 16).expect("contradictions");
    assert!(matches!(result.status, BudgetStatus::Complete));
    assert_eq!(result.pairs.len(), 1, "only edges over the queried claims");
    assert_eq!(result.pairs[0].from.claim_key, a);
    assert_eq!(result.pairs[0].to.claim_key, b);
    // Graph structure only: both sides carried verbatim, no verdict field.
    assert_eq!(result.pairs[0].from.text, "alpha one");
    assert_eq!(result.pairs[0].to.status, ClaimStatus::Staged);
}

// Edges pointing *to* a queried claim join a pair too — the edge joins
// two claims regardless of direction.
#[test]
fn incoming_contradicts_edges_join_pairs() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, "ep-a", "alpha one", ClaimStatus::Active);
    let b = seed(&db, "ep-b", "alpha two", ClaimStatus::Active);
    db.insert_edge(b, EdgeLabel::Contradicts, a).expect("edge");

    let result = query::contradictions(&db, &[a], 16).expect("contradictions");
    assert_eq!(result.pairs.len(), 1);
    assert_eq!(result.pairs[0].from.claim_key, b, "edge stored from b");
    assert_eq!(result.pairs[0].to.claim_key, a);
}

// qt_lineage_traceable: a claim whose lineage reaches no persisted
// episode is invisible — a pair over such a claim returns nothing.
#[test]
fn lineage_broken_side_makes_the_pair_invisible() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, "ep-a", "alpha one", ClaimStatus::Active);
    let ep = episode_with("ep-ghost");
    let (node, lineage) = claim("alpha two", "ep-ghost", ClaimStatus::Active);
    let b = db
        .insert_claim(&node, &[lineage], &ep.text)
        .expect("insert without persisted episode");
    db.insert_edge(a, EdgeLabel::Contradicts, b).expect("edge");

    let result = query::contradictions(&db, &[a], 16).expect("contradictions");
    assert!(
        result.pairs.is_empty(),
        "lineage-broken side hides the pair (qt_lineage_traceable)"
    );
}

// ex_mutation_proposal: staged invalidation proposals against the
// queried claims ride along with their lineage and status — the query
// reports them, never resolves them.
#[test]
fn staged_proposals_against_queried_claims_are_reported() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, "ep-a", "alpha one", ClaimStatus::Active);
    let b = seed(&db, "ep-b", "alpha two", ClaimStatus::Active);
    db.stage_invalidation_proposal(a, "ep-b").expect("proposal");

    let result = query::contradictions(&db, &[a], 16).expect("contradictions");
    assert_eq!(
        result.pairs.len(),
        0,
        "a proposal is not a contradicts edge"
    );
    assert_eq!(result.proposals.len(), 1);
    let proposal = &result.proposals[0];
    assert_eq!(proposal.claim_key, a);
    assert_eq!(proposal.causing_episode_id, "ep-b");
    assert_eq!(proposal.status, ClaimStatus::Active, "status verbatim");
    assert_eq!(proposal.episodes, vec!["ep-a".to_string()]);
    // A proposal against an unqueried claim is not reported.
    let other = query::contradictions(&db, &[b], 16).expect("contradictions");
    assert!(other.proposals.is_empty());
}

// Unconnected claims: a claim with no contradicts edges and no proposals
// produces no hits — the empty answer is honest (complete status).
#[test]
fn unconnected_claims_report_honest_empty() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, "ep-a", "alpha one", ClaimStatus::Active);
    let b = seed(&db, "ep-b", "alpha two", ClaimStatus::Active);
    db.insert_edge(a, EdgeLabel::Contradicts, b).expect("edge");

    let lonely = seed(&db, "ep-l", "alpha lone", ClaimStatus::Active);
    let result = query::contradictions(&db, &[lonely], 16).expect("contradictions");
    assert!(matches!(result.status, BudgetStatus::Complete));
    assert!(result.pairs.is_empty());
    assert!(result.proposals.is_empty());
}

// qt_bounded_traversal: the edge walk is bounded and reports
// budget-exhausted honestly when edges remain unwalked.
#[test]
fn bounded_edge_walk_reports_exhaustion() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, "ep-a", "alpha one", ClaimStatus::Active);
    let b = seed(&db, "ep-b", "alpha two", ClaimStatus::Active);
    let c = seed(&db, "ep-c", "alpha three", ClaimStatus::Active);
    db.insert_edge(a, EdgeLabel::Contradicts, b).expect("edge");
    db.insert_edge(a, EdgeLabel::Contradicts, c).expect("edge");

    let result = query::contradictions(&db, &[a], 1).expect("contradictions");
    assert!(matches!(result.status, BudgetStatus::BudgetExhausted));
    assert_eq!(
        result.pairs.len(),
        1,
        "stopped at the bound, deterministically"
    );
    let result = query::contradictions(&db, &[a], 2).expect("contradictions");
    assert!(matches!(result.status, BudgetStatus::Complete));
    assert_eq!(result.pairs.len(), 2);
}

// gm_relation_typing: the edge label vocabulary is exactly the published
// relation set; other typed edges exist in the graph but only
// contradicts edges join contradiction pairs.
#[test]
fn edge_labels_stay_within_the_published_vocabulary() {
    assert_eq!(
        serde_json::to_value(EdgeLabel::Mentions).unwrap(),
        serde_json::json!("mentions")
    );
    assert_eq!(
        serde_json::to_value(EdgeLabel::Contradicts).unwrap(),
        serde_json::json!("contradicts")
    );
    assert_eq!(
        serde_json::to_value(EdgeLabel::Supports).unwrap(),
        serde_json::json!("supports")
    );
    assert_eq!(
        serde_json::to_value(EdgeLabel::DerivedFrom).unwrap(),
        serde_json::json!("derived_from")
    );
    assert_eq!(
        serde_json::to_value(EdgeLabel::PossibleDuplicateOf).unwrap(),
        serde_json::json!("possible_duplicate_of")
    );
}

// Unknown endpoints are refused structurally — an edge always joins two
// persisted claims.
#[test]
fn edges_to_unknown_claims_are_refused() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, "ep-a", "alpha one", ClaimStatus::Active);
    assert!(db.insert_edge(a, EdgeLabel::Contradicts, 999).is_err());
    assert!(db.insert_edge(999, EdgeLabel::Contradicts, a).is_err());
    assert!(db.stage_invalidation_proposal(999, "ep-a").is_err());
}

// gm_embedded_store: edges and proposals persist as plain rows and
// survive dump-recreate.
#[test]
fn edges_and_proposals_survive_dump_recreate() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, "ep-a", "alpha one", ClaimStatus::Active);
    let b = seed(&db, "ep-b", "alpha two", ClaimStatus::Active);
    db.insert_edge(a, EdgeLabel::Contradicts, b).expect("edge");
    db.insert_edge(b, EdgeLabel::PossibleDuplicateOf, a)
        .expect("edge");
    db.stage_invalidation_proposal(b, "ep-a").expect("proposal");

    let dump = db.dump_extraction_output().expect("dump");
    let fresh = SqliteStore::open_in_memory().expect("fresh");
    for id in ["ep-a", "ep-b"] {
        let ep = episode_with(id);
        bajan::ingest::persist(&fresh, &ep).expect("persist episode");
    }
    fresh.restore_extraction_output(&dump).expect("restore");

    let redumped = fresh.dump_extraction_output().expect("fresh dump");
    assert_eq!(
        redumped, dump,
        "dump-recreate reproduces edges and proposals"
    );

    let result = query::contradictions(&fresh, &[a], 16).expect("contradictions");
    assert_eq!(result.pairs.len(), 1, "contradicts edge survived");
}

// qt_readonly: contradiction reads never mutate the store.
#[test]
fn contradiction_read_stays_readonly() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, "ep-a", "alpha one", ClaimStatus::Active);
    let b = seed(&db, "ep-b", "alpha two", ClaimStatus::Active);
    db.insert_edge(a, EdgeLabel::Contradicts, b).expect("edge");
    db.stage_invalidation_proposal(a, "ep-b").expect("proposal");
    let before = db.dump_extraction_output().expect("dump read");
    let _ = query::contradictions(&db, &[a], 16).expect("contradictions");
    assert_eq!(
        db.dump_extraction_output().expect("dump read"),
        before,
        "contradiction query must not write (qt_readonly)"
    );
}

proptest! {
    // p_contradiction_query: graphs with contradict pairs, staged
    // invalidation proposals, and unconnected claims — returned pairs are
    // exactly the contradicts edges over the queried claims plus their
    // staged proposals; no pair is dropped or adjudicated.
    #[test]
    fn pairs_are_exactly_edges_over_queried_claims(
        // (from-index, to-index) contradict pairs over a 5-claim graph.
        pairs in proptest::collection::vec((0usize..5, 0usize..5), 0..6),
        // Staged proposals: (claim-index, causing-episode index).
        proposals in proptest::collection::vec((0usize..5, 0usize..5), 0..4),
        // Which claims the read queries.
        queried in proptest::collection::hash_set(0usize..5, 1..5),
    ) {
        let db = SqliteStore::open_in_memory().expect("open");
        let mut keys = [0usize; 5];
        for (i, key) in keys.iter_mut().enumerate() {
            let text = format!("alpha claim {i}");
            *key = seed(&db, &format!("ep-{i}"), &text, ClaimStatus::Active);
        }

        let mut want_pairs: Vec<(usize, usize)> = Vec::new();
        // Edges are a set relation (bajan-2hp): an identical
        // (from, label, to) triple is refused, so the generated
        // duplicates insert once and count once.
        let mut seen_edges = std::collections::HashSet::new();
        for (from, to) in pairs.iter() {
            let (f, t) = (keys[*from], keys[*to]);
            if !seen_edges.insert((f, t)) {
                continue;
            }
            db.insert_edge(f, EdgeLabel::Contradicts, t).expect("edge");
            if queried.contains(from) || queried.contains(to) {
                want_pairs.push((f, t));
            }
        }
        let mut want_proposals: Vec<usize> = Vec::new();
        // Proposals are a set relation (bajan-2hp): an identical
        // (claim, causing-episode) pair is refused — dedupe the
        // generated repeats the same way.
        let mut seen_proposals = std::collections::HashSet::new();
        for (claim_i, cause_i) in proposals.iter() {
            let causing = format!("ep-{cause_i}");
            if !seen_proposals.insert((*claim_i, causing.clone())) {
                continue;
            }
            db.stage_invalidation_proposal(keys[*claim_i], &causing)
                .expect("proposal");
            if queried.contains(claim_i) {
                want_proposals.push(keys[*claim_i]);
            }
        }
        want_pairs.sort();
        want_proposals.sort();

        let q: Vec<usize> = queried.iter().map(|i| keys[*i]).collect();
        let result = query::contradictions(&db, &q, 64).expect("contradictions");
        let mut got_pairs: Vec<(usize, usize)> = result
            .pairs
            .iter()
            .map(|p| (p.from.claim_key, p.to.claim_key))
            .collect();
        got_pairs.sort();
        prop_assert_eq!(got_pairs, want_pairs, "exactly the contradicts edges over the queried claims");
        let mut got_proposals: Vec<usize> = result
            .proposals
            .iter()
            .map(|p| p.claim_key)
            .collect();
        got_proposals.sort();
        prop_assert_eq!(got_proposals, want_proposals, "exactly the proposals against the queried claims");

        // Graph structure only: no side ever disappears, both statuses
        // are carried verbatim; the query never adjudicates.
        for pair in &result.pairs {
            prop_assert!(!pair.from.text.is_empty());
            prop_assert!(!pair.to.text.is_empty());
            prop_assert!(!pair.from.episodes.is_empty());
            prop_assert!(!pair.to.episodes.is_empty());
        }
    }
}
