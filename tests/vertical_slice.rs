//! Purpose: vertical-slice property tests (bajan-6hz) — the thin path
//! ingest→extract→SQLite→first query, governed by the specodelic specs.
//! Responsibilities: red-first property tests for `gm_embedded_store`
//! (dump-then-recreate), `ic_malformed`/`ic_unique_id`/`ic_idempotent`
//! (ingest outcomes), and `qt_bounded_traversal`/`qt_lineage_traceable`/
//! `qt_readonly` (the first read query).
//! Rationale: each test derives from a specodelic property (p_embedded_store,
//! p_malformed, p_idempotent, p_bounded_traversal, p_lineage_traceable,
//! p_readonly); written red before the implementation.

use bajan::ingest::{self, EpisodeRecord, IngestOutcome, Locator, SourceMeta};
use bajan::query::{self, BudgetStatus};
use bajan::store::sqlite::SqliteStore;
use bajan::store::{ClaimNode, ClaimStatus, Evidence, Lineage};
use proptest::prelude::*;

/// Episode text used by the search tests: hedge-free, and a prefix of it
/// serves as claim text so the containment gate passes without hedging.
const EPISODE_TEXT: &str = "alpha beta gamma delta epsilon zeta.";

fn episode(id: &str, text: &str) -> EpisodeRecord {
    EpisodeRecord {
        id: id.into(),
        text: text.into(),
        locator: Locator::Span("heading:Notes".into()),
        source: SourceMeta {
            source_type: "episode".into(),
            data_cutoff: Some("2026-01-01".into()),
            authority_tier: 2,
            tags: vec!["workspace:dev".into()],
        },
    }
}

/// A staged candidate claim over `EPISODE_TEXT`: text is a whitespace-
/// collapsed-contained span of the episode (passes the typed gate), and
/// carries no hedge marker (nothing to drop).
fn staged_claim(episode_id: &str) -> (ClaimNode, Lineage) {
    (
        ClaimNode {
            text: "alpha beta gamma".into(),
            valid_at: None,
            invalid_at: None,
            data_cutoff: Some("2026-01-01".into()),
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

// ---- gm_embedded_store (p_embedded_store): dump-then-recreate from the
// episode stream plus extraction output reproduces the graph exactly. ----

proptest! {
    #[test]
    fn dump_then_recreate_reproduces_the_graph(
        ids in proptest::collection::vec("[a-z0-9-]{3,12}", 1..5),
        texts in proptest::collection::vec("[a-z0-9 .,!?-]{10,60}", 1..5),
    ) {
        let db = SqliteStore::open_in_memory().expect("open");
        let n = ids.len().min(texts.len());
        for i in 0..n {
            let record = EpisodeRecord {
                id: ids[i].clone(),
                text: texts[i].clone(),
                ..episode("seed", "")
            };
            ingest::persist(&db, &record).expect("persist");
            let (mut node, lineage) = staged_claim(&ids[i]);
            node.text = texts[i].clone();
            node.evidence = Evidence::Span {
                text: texts[i].clone(),
                locator: "heading:Notes".into(),
            };
            db.insert_claim(&node, &[lineage], &texts[i]).expect("insert");
        }

        // Dump: the episode stream plus extraction output.
        let stream = ingest::dump_stream(&db);
        let extraction = db.dump_extraction_output();

        // Recreate a fresh store from the dumps alone.
        let fresh = SqliteStore::open_in_memory().expect("open");
        for record in &stream {
            ingest::persist(&fresh, record).expect("re-persist");
        }
        fresh
            .restore_extraction_output(&extraction)
            .expect("restore succeeds");

        prop_assert_eq!(fresh.episodes().len(), n);
        prop_assert_eq!(fresh.dump_extraction_output(), db.dump_extraction_output());
        for (i, text) in texts.iter().take(n).enumerate() {
            prop_assert_eq!(fresh.claim_text(i), Some(text.clone()));
        }
    }
}

// ---- ic_malformed: malformed records are rejected, never persisted ----

proptest! {
    #[test]
    fn malformed_records_are_rejected_never_persisted(
        whitespace in " \t\n{1,5}",
    ) {
        let db = SqliteStore::open_in_memory().expect("open");

        // Whitespace-only verbatim text → rejected (ic_malformed).
        let no_text = EpisodeRecord {
            text: whitespace,
            ..episode("ep-x", "")
        };
        let outcome = ingest::persist(&db, &no_text);
        let rejected_text = matches!(outcome, Ok(IngestOutcome::Rejected { .. }));
        prop_assert!(rejected_text);

        // Empty or whitespace-only id is malformed too (ic_id_canon).
        let no_id = EpisodeRecord {
            id: "  ".into(),
            ..episode("", "real text")
        };
        let outcome = ingest::persist(&db, &no_id);
        let rejected_id = matches!(outcome, Ok(IngestOutcome::Rejected { .. }));
        prop_assert!(rejected_id);
    }
}

// ---- ic_unique_id / ic_idempotent: re-ingest converges, never duplicates ----

#[test]
fn reingest_is_idempotent_and_never_duplicates() {
    let db = SqliteStore::open_in_memory().expect("open");
    let good = episode("ep-ok", "Cache hits are cheap.");
    assert!(matches!(
        ingest::persist(&db, &good),
        Ok(IngestOutcome::Persisted)
    ));
    // Same id, mutated text: still exactly one episode — the re-submission
    // is refused as already persisted, never duplicating or mutating.
    let mutated = episode("ep-ok", "mutated text");
    assert!(matches!(
        ingest::persist(&db, &mutated),
        Ok(IngestOutcome::AlreadyPersisted)
    ));
    assert_eq!(db.episodes().len(), 1);
    assert_eq!(db.episodes()[0].text, "Cache hits are cheap.");
}

// ---- extract: the thin deterministic proposer feeds the typed gate ----

#[test]
fn end_to_end_ingest_extract_then_first_query() {
    let db = SqliteStore::open_in_memory().expect("open");
    ingest::persist(&db, &episode("ep-a", EPISODE_TEXT)).expect("persist");

    let report = bajan::extract::run_extract(&db, "0.1.0").expect("extract");
    assert_eq!(report.episodes_processed, 1);
    assert_eq!(report.candidates_proposed, 1, "one deterministic candidate");
    assert_eq!(report.gate_rejected, 0);

    // The proposed claim is staged and searchable (lineage-traceable).
    let result = query::search(&db, "alpha", 100).expect("search");
    assert!(matches!(result.status, BudgetStatus::Complete));
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].status, ClaimStatus::Staged);
    assert_eq!(result.hits[0].episodes, vec!["ep-a".to_string()]);

    // Same-version re-extract is a cache hit: no new work, no new claims.
    let again = bajan::extract::run_extract(&db, "0.1.0").expect("re-extract");
    assert_eq!(again.episodes_processed, 0, "cache hit: no new work");
    assert_eq!(query::search(&db, "alpha", 100).unwrap().hits.len(), 1);
}

// ---- qt_lineage_traceable + qt_bounded_traversal + qt_readonly ----

proptest! {
    #[test]
    fn search_is_lineage_traceable_and_bounded(
        lineaged in proptest::collection::vec(proptest::bool::ANY, 1..8),
    ) {
        let db = SqliteStore::open_in_memory().expect("open");
        ingest::persist(&db, &episode("ep-a", EPISODE_TEXT)).expect("persist");
        for &live in lineaged.iter() {
            let (node, mut lineage) = staged_claim("ep-a");
            if !live {
                // Lineage-broken: its only edge points at a deleted episode.
                lineage.episode_id = "ep-deleted".into();
            }
            db.insert_claim(&node, &[lineage], EPISODE_TEXT).expect("insert");
        }
        let total = lineaged.len();
        let live_count = lineaged.iter().filter(|&&l| l).count();

        // qt_lineage_traceable: lineage-broken claims are invisible; every
        // returned claim resolves to at least one persisted episode.
        let full = query::search(&db, "alpha", total).expect("search");
        prop_assert_eq!(full.hits.len(), live_count);
        for hit in &full.hits {
            prop_assert!(!hit.episodes.is_empty());
        }

        // qt_bounded_traversal: a truncated run reports the typed marker
        // deterministically, never a silent partial answer.
        if total > 1 {
            let bounded = query::search(&db, "alpha", 1).expect("search");
            prop_assert!(matches!(bounded.status, BudgetStatus::BudgetExhausted));
        }

        // qt_readonly: no state transition, audit, or edge written.
        let before = db.dump_extraction_output();
        let _ = query::search(&db, "alpha", total);
        prop_assert_eq!(db.dump_extraction_output(), before);
    }
}

#[test]
fn not_found_is_honest_when_budget_completes_without_match() {
    let db = SqliteStore::open_in_memory().expect("open");
    ingest::persist(&db, &episode("ep-a", "plain text only.")).expect("persist");
    let result = query::search(&db, "absent", 100).expect("search");
    assert!(matches!(result.status, BudgetStatus::Complete));
    assert!(
        result.hits.is_empty(),
        "not-found is a complete, empty answer"
    );
}
