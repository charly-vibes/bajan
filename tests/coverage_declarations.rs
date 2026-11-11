//! Purpose: red-first coverage-declaration evidence (bajan-alb,
//! add-query-coverage-evidence) — every accepted import run persists an
//! immutable declared-id set (ic_coverage_declaration), and zero-hit
//! reads qualify `not-found` with a coverage marker instead of asserting
//! world-absence over an incomplete corpus (qt_coverage_status).
//! Responsibilities: declared-set persistence on `persist_stream` runs
//! (singles declare one id; zero-episode streams declare nothing),
//! D1 bounded verbatim storage (count+digest beyond the bound), D2
//! immutability (reruns converge, never mutate or duplicate), D3
//! zero-hit-only marker with hit results byte-identical, D4 reason
//! precedence (budget-exhausted > coverage-partial > not-found), D5
//! relevance over ALL persisted declarations.
//! Rationale: a zero-result read over a partially ingested corpus reads
//! as "the world contains no matching facts" when the truth is "the
//! corpus is incomplete" — the marker says "the question may be
//! answerable later", never silently. The marker is deliberately a
//! separate field from the extraction-coverage note (bajan-913): this is
//! ingest coverage (declared vs persisted episodes), not extraction
//! coverage (extracted vs persisted episodes).
//! Rationale for the conservative beyond-bound shape: verbatim ids are
//! bounded per D1, so presence beyond the bound cannot be resolved —
//! the tail counts as unpersisted (a false `coverage-complete` would be
//! exactly the dishonesty this change removes).

use bajan::ingest::{EpisodeRecord, Locator, SourceMeta};
use bajan::query::{self, BudgetStatus};
use bajan::store::sqlite::{DECLARED_ID_BOUND, SqliteStore};
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

/// A record with a valid stable id but no verbatim text: the run
/// *declares* the id, but the episode never persists (ic_malformed) —
/// the deterministic declared-but-unpersisted shape.
fn malformed(id: &str) -> EpisodeRecord {
    EpisodeRecord {
        text: "   ".into(),
        ..episode(id)
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

/// Seed a claim over an already-persisted episode (idempotent persist)
/// and mark extraction complete so the *extraction*-coverage note
/// (bajan-913) stays silent — assertions here are about ingest coverage.
fn seed_claim(db: &SqliteStore, id: &str) {
    let ep = episode(id);
    bajan::ingest::persist(db, &ep).expect("persist");
    let (node, lineage) = matching_claim(id);
    db.insert_claim(&node, &[lineage], &ep.text)
        .expect("insert claim");
    db.mark_extracted(id, "0.1.0").expect("mark extracted");
}

/// The canonical partial-corpus run: one run declaring five ids,
/// persisting exactly three (two records are malformed — declared but
/// never persisted).
fn partial_run(db: &SqliteStore) {
    let stream = vec![
        episode("ep-1"),
        episode("ep-2"),
        episode("ep-3"),
        malformed("ep-4"),
        malformed("ep-5"),
    ];
    let outcomes = bajan::ingest::persist_stream(db, &stream);
    assert_eq!(outcomes.len(), 5, "one outcome per submitted record");
}

// Delta scenario "Partial run leaves an honest declaration": a run
// declaring five ids that persists only three still records all five —
// the declaration is written at acceptance, before any outcome.
#[test]
fn partial_run_declares_its_full_id_set() {
    let db = SqliteStore::open_in_memory().expect("open");
    partial_run(&db);

    let decls = db.coverage_declarations().expect("declarations");
    assert_eq!(decls.len(), 1, "one run, one declaration");
    assert_eq!(decls[0].declared_count, 5, "all five ids declared");
    assert_eq!(decls[0].stored_ids, 5, "verbatim within the bound");
    let stored = db.coverage_declared_ids().expect("declared ids");
    assert_eq!(
        stored,
        vec!["ep-1", "ep-2", "ep-3", "ep-4", "ep-5"],
        "the declared set is the sorted normalized id set"
    );
}

// D2 immutability: re-running the same stream (same set, any order)
// converges to the same declaration key — no new row, no mutation.
#[test]
fn declaration_rerun_converges_without_new_rows() {
    let db = SqliteStore::open_in_memory().expect("open");
    let stream = vec![episode("ep-b"), episode("ep-a"), episode("ep-c")];
    bajan::ingest::persist_stream(&db, &stream);
    let before = db.coverage_declarations().expect("declarations");

    // Same stream again, then shuffled: same declared SET either way.
    bajan::ingest::persist_stream(&db, &stream);
    let shuffled = vec![episode("ep-c"), episode("ep-a"), episode("ep-b")];
    bajan::ingest::persist_stream(&db, &shuffled);

    let after = db.coverage_declarations().expect("declarations");
    assert_eq!(after.len(), before.len(), "reruns add no declaration rows");
    assert_eq!(
        after[0].run_id, before[0].run_id,
        "the declared-set digest is order-independent"
    );
    assert_eq!(after[0].declared_count, 3);
}

// Singles declare one id: a single-episode stream is a run of one.
#[test]
fn single_episode_stream_declares_one_id() {
    let db = SqliteStore::open_in_memory().expect("open");
    bajan::ingest::persist_stream(&db, &[episode("ep-solo")]);

    let decls = db.coverage_declarations().expect("declarations");
    assert_eq!(decls.len(), 1);
    assert_eq!(decls[0].declared_count, 1);
    assert_eq!(
        db.coverage_declared_ids().expect("ids"),
        vec!["ep-solo".to_string()]
    );
}

// Acceptance E2E verbatim: declare 5, persist 3, zero-hit query ->
// coverage-partial (5,3); persist the last 2 -> same query -> not-found.
#[test]
fn coverage_e2e_declare5_persist3_then_complete() {
    let db = SqliteStore::open_in_memory().expect("open");
    partial_run(&db);

    let empty = query::search(&db, MISSING, 16).expect("search");
    assert!(empty.hits.is_empty(), "no claim matches");
    let cov = empty
        .ingest_coverage
        .as_ref()
        .expect("zero-hit over a partial corpus carries the marker");
    assert_eq!((cov.declared, cov.persisted), (5, 3));

    // D3: hit results carry no marker and are unchanged.
    seed_claim(&db, "ep-1");
    let hits = query::search(&db, MATCHING, 16).expect("search");
    assert_eq!(hits.hits.len(), 1);
    assert!(
        hits.ingest_coverage.is_none(),
        "hit-carrying reads never carry the marker"
    );

    // The corrected run persists the remaining two declared ids and
    // writes its OWN declaration; the original is never mutated.
    let corrections = vec![episode("ep-4"), episode("ep-5")];
    bajan::ingest::persist_stream(&db, &corrections);
    let decls = db.coverage_declarations().expect("declarations");
    assert_eq!(decls.len(), 2, "the correction is its own run");
    assert!(
        decls.iter().any(|d| d.declared_count == 5),
        "original declaration intact"
    );

    // Same query over the now-complete corpus: bare not-found.
    let empty = query::search(&db, MISSING, 16).expect("search");
    assert!(empty.hits.is_empty());
    assert!(
        empty.ingest_coverage.is_none(),
        "coverage-complete zero-hit is honest not-found, no marker"
    );
}

// D1 bound: the declared set stores verbatim up to the bound and
// count-plus-digest beyond it; beyond-bound presence cannot be resolved,
// so the tail counts as unpersisted (conservative, never false-complete).
#[test]
fn declared_id_set_verbatim_up_to_bound_digest_beyond() {
    let db = SqliteStore::open_in_memory().expect("open");
    let n = DECLARED_ID_BOUND + 2;
    let stream: Vec<EpisodeRecord> = (0..n).map(|i| episode(&format!("ep-{i:05}"))).collect();
    bajan::ingest::persist_stream(&db, &stream);

    let decls = db.coverage_declarations().expect("declarations");
    assert_eq!(decls.len(), 1);
    assert_eq!(
        decls[0].declared_count, n,
        "the run row carries the full count"
    );
    assert_eq!(
        decls[0].stored_ids, DECLARED_ID_BOUND,
        "verbatim storage is bounded"
    );

    // Every id persisted, yet the beyond-bound tail is unverifiable:
    // declared (n) > verifiable-persisted (bound) -> partial, honestly.
    let empty = query::search(&db, MISSING, 16).expect("search");
    let cov = empty
        .ingest_coverage
        .as_ref()
        .expect("beyond-bound declarations stay partial");
    assert_eq!((cov.declared, cov.persisted), (n, DECLARED_ID_BOUND));
}

// Migration D: a store with no declarations reports coverage-complete —
// the honest default (absence of declarations is no known
// incompleteness), no marker, no backfill.
#[test]
fn no_declaration_store_reports_no_marker() {
    let db = SqliteStore::open_in_memory().expect("open");
    seed_claim(&db, "ep-direct");

    let empty = query::search(&db, MISSING, 16).expect("search");
    assert!(empty.hits.is_empty());
    assert!(empty.ingest_coverage.is_none());
}

// D4 precedence: a truncated scan is definitive about neither the world
// nor the corpus — budget-exhausted outranks coverage.
#[test]
fn budget_exhaustion_outranks_coverage() {
    let db = SqliteStore::open_in_memory().expect("open");
    partial_run(&db);
    for id in ["ep-1", "ep-2", "ep-3"] {
        seed_claim(&db, id);
    }

    let result = query::search(&db, MISSING, 1).expect("search");
    assert!(result.hits.is_empty());
    assert!(
        matches!(result.status, BudgetStatus::BudgetExhausted),
        "the truncated scan reports budget-exhausted"
    );
    assert!(
        result.ingest_coverage.is_none(),
        "budget exhaustion outranks coverage — no marker"
    );
}

// D3: hit results are byte-identical — no marker on reads with hits
// (asserted on the E2E too; here over a richer partial corpus).
#[test]
fn hit_results_carry_no_marker() {
    let db = SqliteStore::open_in_memory().expect("open");
    partial_run(&db);
    for id in ["ep-1", "ep-2", "ep-3"] {
        seed_claim(&db, id);
    }

    let hits = query::search(&db, MATCHING, 16).expect("search");
    assert_eq!(hits.hits.len(), 3);
    assert!(hits.ingest_coverage.is_none());
}

proptest! {
    // p_coverage_declaration (bajan-alb): over arbitrary stream shapes —
    // valid records, malformed records (declared, never persisted),
    // intra-stream duplicates, shuffles, and reruns — each accepted run
    // persists exactly one immutable declaration recording its distinct
    // normalized declared-id set; reruns and shuffles converge to the
    // same declaration without adding rows.
    #[test]
    fn p_coverage_declaration(
        n_good in 0usize..=5,
        n_malformed in 0usize..=2,
        n_dup in 0usize..=2,
        shuffle in proptest::bool::ANY,
        rerun in proptest::bool::ANY,
    ) {
        let db = SqliteStore::open_in_memory().expect("open");
        let mut stream: Vec<EpisodeRecord> = Vec::new();
        for i in 0..n_good {
            stream.push(episode(&format!("ep-{i:02}")));
        }
        for i in 0..n_malformed {
            stream.push(malformed(&format!("ep-bad-{i:02}")));
        }
        for _ in 0..n_dup {
            // intra-stream duplicate of the first id (first-wins)
            stream.push(episode("ep-00"));
        }
        let mut declared: Vec<String> = (0..n_good)
            .map(|i| format!("ep-{i:02}"))
            .chain((0..n_malformed).map(|i| format!("ep-bad-{i:02}")))
            .collect();
        if n_dup > 0 {
            // the duplicated id is declared too (the set is over submissions)
            declared.push("ep-00".to_string());
        }
        declared.sort();
        declared.dedup();
        let shuffled = if shuffle {
            let mut s = stream.clone();
            s.reverse();
            s
        } else {
            stream.clone()
        };
        let outcomes = bajan::ingest::persist_stream(&db, &shuffled);
        prop_assert_eq!(outcomes.len(), shuffled.len(), "one outcome per record");
        if rerun {
            bajan::ingest::persist_stream(&db, &shuffled);
        }

        let decls = db.coverage_declarations().expect("declarations");
        if declared.is_empty() {
            prop_assert!(decls.is_empty(), "a zero-episode stream declares nothing");
            return Ok(());
        }
        prop_assert_eq!(decls.len(), 1, "one declaration per declared set");
        prop_assert_eq!(decls[0].declared_count, declared.len());
        prop_assert_eq!(decls[0].stored_ids, declared.len());
        prop_assert_eq!(db.coverage_declared_ids().expect("ids"), declared);
        if rerun {
            // the rerun converged: still exactly one row, unchanged
            let again = db.coverage_declarations().expect("declarations");
            prop_assert!(again[0].run_id == decls[0].run_id);
        }
    }

    // p_coverage_status (bajan-alb): over arbitrary declared/persisted
    // shapes and read shapes — the marker appears exactly on zero-hit
    // completed reads over partially covered corpora, with exact counts;
    // complete corpora, declaration-free stores, and budget-exhausted or
    // hit-carrying reads never carry it.
    #[test]
    fn p_coverage_status(
        n_persisted in 0usize..=4,
        n_missing in 0usize..=4,
        want_hits in proptest::bool::ANY,
        exhaust in proptest::bool::ANY,
    ) {
        let db = SqliteStore::open_in_memory().expect("open");
        let mut stream: Vec<EpisodeRecord> = Vec::new();
        for i in 0..n_persisted {
            stream.push(episode(&format!("ep-{i:02}")));
        }
        for i in 0..n_missing {
            stream.push(malformed(&format!("ep-miss-{i:02}")));
        }
        if !stream.is_empty() {
            bajan::ingest::persist_stream(&db, &stream);
        }
        for i in 0..n_persisted {
            seed_claim(&db, &format!("ep-{i:02}"));
        }

        let pattern = if want_hits { MATCHING } else { MISSING };
        // Budget 0 truncates on the first walked claim, so it exhausts
        // exactly when at least one claim exists to walk.
        let budget = if exhaust && n_persisted > 0 { 0 } else { 16 };
        let result = query::search(&db, pattern, budget).expect("search");

        let exhausted = exhaust && n_persisted > 0;
        prop_assert_eq!(
            matches!(result.status, BudgetStatus::BudgetExhausted),
            exhausted,
            "exhaustion only when requested and walkable"
        );
        if exhausted {
            prop_assert!(result.ingest_coverage.is_none(), "budget outranks coverage");
            return Ok(());
        }
        if want_hits && n_persisted > 0 {
            // a hit-carrying read: claims exist and the pattern matches
            prop_assert_eq!(result.hits.len(), n_persisted, "hits over claimed episodes");
            prop_assert!(result.ingest_coverage.is_none(), "hits carry no marker");
            return Ok(());
        }
        // Zero-hit completed read: either the missing pattern, or the
        // matching pattern over a claim-free store (n_persisted == 0 —
        // honestly zero-hit either way).
        match (result.ingest_coverage.as_ref(), n_missing) {
            (Some(cov), n) if n > 0 => {
                prop_assert_eq!((cov.declared, cov.persisted), (n_persisted + n, n_persisted));
            }
            (None, 0) => {}
            other => panic!("coverage marker shape wrong: {other:?}"),
        }
    }
}
