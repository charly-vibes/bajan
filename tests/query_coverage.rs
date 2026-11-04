//! Purpose: property + scenario tests for corpus coverage surfacing
//! (bajan-913, doc §4 B6) — a query over a partially extracted corpus
//! identifies its coverage limitation instead of answering silently.
//! Responsibilities: red-first derivation of `SearchResult.coverage` —
//! episodes persisted but never extracted (no extraction-cache entry at
//! ANY extractor version: pending or parked/failed) are counted and
//! reported with the corpus total; a fully extracted corpus carries no
//! coverage note.
//! Rationale: stochastic parking leaves episodes silently pending
//! (evals/results.md: 8/50 pending in one pass), so queries would run
//! over silently incomplete corpora. Run rows are in-memory, so the
//! honest read-side observable is the extraction cache: an episode with
//! no cache entry at any version is pending or parked — the note says
//! exactly that and never adjudicates which. The EARS shape under test:
//! WHEN a query returns zero claims over a partially ingested corpus,
//! THE result SHALL identify its coverage limitation.

use bajan::ingest::{EpisodeRecord, Locator, SourceMeta};
use bajan::query::{self, BudgetStatus};
use bajan::store::sqlite::SqliteStore;
use bajan::store::{ClaimNode, ClaimStatus, Evidence, Lineage};
use proptest::prelude::*;

const MATCHING: &str = "alpha";
const MISSING: &str = "zzz";

fn episode(id: &str) -> EpisodeRecord {
    EpisodeRecord {
        id: id.into(),
        text: "alpha beta gamma delta epsilon zeta.".into(),
        locator: Locator::Span("heading:Notes".into()),
        source: SourceMeta {
            source_type: "episode".into(),
            data_cutoff: None,
            authority_tier: 1,
            tags: vec!["workspace:dev".to_string()],
        },
    }
}

fn matching_claim(episode_id: &str) -> (ClaimNode, Lineage) {
    (
        ClaimNode {
            text: "alpha beta gamma".into(),
            valid_at: None,
            invalid_at: None,
            data_cutoff: None,
            status: ClaimStatus::Staged,
            scope: "workspace:dev".into(),
            source_type: "episode".into(),
            evidence: Evidence::Span {
                text: "alpha beta gamma".into(),
                locator: "heading:Notes".into(),
            },
        },
        Lineage {
            episode_id: episode_id.into(),
            extractor_version: "0.1.0".into(),
        },
    )
}

/// Seed the parked/pending shape: the episode is persisted but nothing
/// was ever extracted from it — no cache entry at any version, no
/// claims (a mid-run kill or retry-budget park leaves exactly this).
fn seed_unextracted(db: &SqliteStore, id: &str) {
    db.insert_episode(&episode(id)).expect("persist episode");
}

/// Seed a fully extracted episode: persisted, claim backed, cache entry
/// present — invisible to the coverage count.
fn seed_extracted(db: &SqliteStore, id: &str) -> usize {
    let ep = episode(id);
    bajan::ingest::persist(db, &ep).expect("persist");
    let (node, lineage) = matching_claim(id);
    let key = db
        .insert_claim(&node, &[lineage], &ep.text)
        .expect("insert claim");
    db.mark_extracted(id, "0.1.0").expect("mark extracted");
    key
}

/// Seed an extracted episode with a cache entry but zero claims — the
/// legitimately-empty extraction outcome; equally invisible to the
/// coverage count.
fn seed_extracted_no_claims(db: &SqliteStore, id: &str) {
    db.insert_episode(&episode(id)).expect("persist episode");
    db.mark_extracted(id, "0.1.0").expect("mark extracted");
}

// EARS (bajan-913): a query over a partially extracted corpus — here
// with hits, then the zero-claims shape — identifies its coverage
// limitation; the counts are the honest parked/pending observable.
#[test]
fn partial_corpus_identifies_coverage_limitation() {
    let db = SqliteStore::open_in_memory().expect("open");
    seed_extracted(&db, "ep-1");
    seed_unextracted(&db, "ep-parked");

    let result = query::search(&db, MATCHING, 16).expect("search");
    assert_eq!(result.hits.len(), 1, "the extracted claim matches");
    let coverage = result
        .coverage
        .as_ref()
        .expect("partial corpus carries a coverage note");
    assert_eq!(coverage.unextracted, 1, "one episode never extracted");
    assert_eq!(coverage.total, 2, "both episodes persisted");
    assert!(
        coverage.note.contains('1') && coverage.note.contains('2'),
        "note carries both counts: {}",
        coverage.note
    );

    // The EARS clause verbatim: zero claims over the partial corpus still
    // identifies the limitation — silence is the bug being fixed.
    let empty = query::search(&db, MISSING, 16).expect("search");
    assert!(empty.hits.is_empty(), "no claim matches");
    let coverage = empty
        .coverage
        .as_ref()
        .expect("zero-claim answer identifies its coverage limitation");
    assert_eq!(coverage.unextracted, 1);
    assert_eq!(coverage.total, 2);
}

// A fully extracted corpus has no coverage limitation to identify — the
// note is absent, never noise.
#[test]
fn fully_extracted_corpus_carries_no_coverage() {
    let db = SqliteStore::open_in_memory().expect("open");
    seed_extracted(&db, "ep-1");
    seed_extracted(&db, "ep-2");

    let result = query::search(&db, MATCHING, 16).expect("search");
    assert_eq!(result.hits.len(), 2, "both claims match");
    assert!(result.coverage.is_none(), "no limitation to report");

    let empty = query::search(&db, MISSING, 16).expect("search");
    assert!(empty.hits.is_empty());
    assert!(
        empty.coverage.is_none(),
        "zero claims over a fully extracted corpus is honest not-found, not a coverage gap"
    );
}

// The coverage predicate is cache-entry-at-ANY-version: an episode
// extracted at an older extractor version has its content in the graph,
// so it is not unextracted even though a newer version would re-attempt
// it (ex_park_requeue economics stay untouched).
#[test]
fn episode_extracted_at_any_version_is_not_unextracted() {
    let db = SqliteStore::open_in_memory().expect("open");
    seed_extracted(&db, "ep-v1");
    seed_extracted_no_claims(&db, "ep-v1-empty");

    let result = query::search(&db, MATCHING, 16).expect("search");
    assert!(
        result.coverage.is_none(),
        "cache entries at any version count as extracted"
    );
}

// Coverage is corpus metadata, not walk state: a budget-exhausted answer
// still reports the corpus limitation (and vice versa — the budget
// vocabulary is untouched).
#[test]
fn coverage_note_is_independent_of_the_walk_budget() {
    let db = SqliteStore::open_in_memory().expect("open");
    seed_extracted(&db, "ep-1");
    seed_extracted(&db, "ep-2");
    seed_unextracted(&db, "ep-parked");

    let result = query::search(&db, MATCHING, 1).expect("search");
    assert!(matches!(result.status, BudgetStatus::BudgetExhausted));
    let coverage = result
        .coverage
        .as_ref()
        .expect("coverage survives a truncated walk");
    assert_eq!(coverage.unextracted, 1);
    assert_eq!(coverage.total, 3);
}

proptest! {
    // p_coverage (bajan-913): over arbitrary corpus shapes — matching
    // extracted episodes, extracted-but-empty episodes, and parked/pending
    // episodes — the counts are exact, the note is present exactly when
    // the corpus is partial, and the zero-claims answer carries the same
    // coverage as the matching one.
    #[test]
    fn p_coverage_note(
        n_match in 0usize..=4,
        n_empty_extracted in 0usize..=3,
        n_unextracted in 0usize..=4,
    ) {
        let db = SqliteStore::open_in_memory().expect("open");
        let mut counter = 0usize;
        let mut next = |prefix: &str| {
            counter += 1;
            format!("{prefix}-{counter}")
        };
        for _ in 0..n_match {
            seed_extracted(&db, &next("ep-match"));
        }
        for _ in 0..n_empty_extracted {
            seed_extracted_no_claims(&db, &next("ep-empty"));
        }
        for _ in 0..n_unextracted {
            seed_unextracted(&db, &next("ep-parked"));
        }

        let total = n_match + n_empty_extracted + n_unextracted;
        for pattern in [MATCHING, MISSING] {
            let result = query::search(&db, pattern, 256).expect("search");
            match (&result.coverage, n_unextracted) {
                (Some(coverage), n) if n > 0 => {
                    prop_assert_eq!(coverage.unextracted, n_unextracted);
                    prop_assert_eq!(coverage.total, total);
                    prop_assert!(coverage.note.contains(&n_unextracted.to_string()));
                    prop_assert!(coverage.note.contains(&total.to_string()));
                }
                (None, 0) => {}
                other => panic!("coverage shape wrong for {n_unextracted} unextracted: {other:?}"),
            }
        }
        // The zero-claims answer matches the matching answer's coverage.
        let with_hits = query::search(&db, MATCHING, 256).expect("search");
        let empty = query::search(&db, MISSING, 256).expect("search");
        prop_assert_eq!(with_hits.coverage, empty.coverage);
    }
}
