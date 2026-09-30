//! Purpose: property + scenario tests for the deterministic
//! contradiction-scan pass (bajan-3hg) — the first in-pipeline producer of
//! `contradicts` edges (`gm_contradicts_provenance`).
//! Responsibilities: red-first derivation of the closed edit classes
//! (negation-marker present/absent, single differing numeral token — no
//! fuzzy subject detection), pass identity + rule provenance on every
//! written edge, honest budget exhaustion, determinism, re-run duplicate
//! refusal, and dump-recreate durability of edge provenance
//! (`gm_embedded_store`).
//! Rationale: the contradiction query (qt_contradiction_query) reads
//! `contradicts` edges that nothing writes — this pass is the producer;
//! every test here guards that the producer stays deterministic, closed
//! over its edit classes, and honest about what it examined.

use bajan::contradict;
use bajan::ingest::{EpisodeRecord, Locator, SourceMeta};
use bajan::query::BudgetStatus;
use bajan::store::sqlite::SqliteStore;
use bajan::store::{EdgeLabel, Evidence, Lineage};
use bajan::{ingest, store};
use proptest::prelude::*;

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

/// Seed an episode and one staged claim backed by it.
fn seed(db: &SqliteStore, ep: &EpisodeRecord, text: &str) -> usize {
    ingest::persist(db, ep).expect("persist");
    let node = store::ClaimNode {
        text: text.into(),
        valid_at: None,
        invalid_at: None,
        data_cutoff: None,
        status: store::ClaimStatus::Staged,
        scope: "workspace:dev".into(),
        source_type: "episode".into(),
        evidence: Evidence::Span {
            text: text.into(),
            locator: "heading:Notes".into(),
        },
    };
    db.insert_claim(
        &node,
        &[Lineage {
            episode_id: ep.id.clone(),
            extractor_version: "0.1.0".into(),
        }],
        &ep.text,
    )
    .expect("insert")
}

fn contradicts_edges(db: &SqliteStore) -> Vec<(usize, usize)> {
    db.edges_with_provenance()
        .expect("read")
        .into_iter()
        .filter(|(_, label, _, _)| *label == EdgeLabel::Contradicts)
        .map(|(from, _, to, _)| (from, to))
        .collect()
}

// --- red-first scenarios -------------------------------------------------

#[test]
fn negation_pair_proposes() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, &episode("ep1", "body one"), "The service is up.");
    let b = seed(&db, &episode("ep2", "body two"), "The service is not up.");

    let report = contradict::run_contradiction_pass(&db, 256, 1_000_000).expect("pass");
    assert_eq!(report.status, BudgetStatus::Complete, "honest complete");
    assert_eq!(
        report.pairs,
        vec![(a.min(b), a.max(b))],
        "negation proposes"
    );
    assert_eq!(contradicts_edges(&db), vec![(a.min(b), a.max(b))]);
}

#[test]
fn no_marker_pair_proposes() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, &episode("ep1", "body one"), "There are retries left.");
    let b = seed(
        &db,
        &episode("ep2", "body two"),
        "There are no retries left.",
    );

    let report = contradict::run_contradiction_pass(&db, 256, 1_000_000).expect("pass");
    assert_eq!(report.pairs.len(), 1, "'no X' vs 'X' proposes");
    let (from, to) = report.pairs[0];
    assert_eq!((from, to), (a.min(b), a.max(b)));
}

#[test]
fn numeric_mismatch_proposes() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(
        &db,
        &episode("ep1", "body one"),
        "The timeout is 30 seconds.",
    );
    let b = seed(
        &db,
        &episode("ep2", "body two"),
        "The timeout is 60 seconds.",
    );

    let report = contradict::run_contradiction_pass(&db, 256, 1_000_000).expect("pass");
    assert_eq!(report.pairs.len(), 1, "numeric mismatch proposes");
    let (from, to) = report.pairs[0];
    assert_eq!((from, to), (a.min(b), a.max(b)));
}

#[test]
fn unrelated_pairs_do_not_propose() {
    let db = SqliteStore::open_in_memory().expect("open");
    seed(&db, &episode("ep1", "body one"), "The service is up.");
    seed(
        &db,
        &episode("ep2", "body two"),
        "Payment processing failed twice.",
    );
    seed(
        &db,
        &episode("ep3", "body three"),
        "The cache uses 64 megabytes.",
    );

    let report = contradict::run_contradiction_pass(&db, 256, 1_000_000).expect("pass");
    assert_eq!(report.pairs, Vec::new(), "unrelated texts never propose");
    assert!(contradicts_edges(&db).is_empty());
}

#[test]
fn equal_texts_and_same_side_negation_do_not_propose() {
    let db = SqliteStore::open_in_memory().expect("open");
    // Identical texts — that is ER's job, not a contradiction.
    seed(&db, &episode("ep1", "body one"), "The alpha service is up.");
    seed(&db, &episode("ep2", "body two"), "The alpha service is up.");
    // Both sides carry the negation — same claim, not a contradiction.
    // (Distinct subject: a lone negated variant against the positive
    // pair above WOULD be a genuine negation pair.)
    seed(
        &db,
        &episode("ep3", "body three"),
        "The beta service is not up.",
    );
    seed(
        &db,
        &episode("ep4", "body four"),
        "The beta service is not up.",
    );
    // Same number on both sides — no mismatch.
    seed(
        &db,
        &episode("ep5", "body five"),
        "The timeout is 30 seconds.",
    );
    seed(
        &db,
        &episode("ep6", "body six"),
        "The timeout is 30 seconds.",
    );

    let report = contradict::run_contradiction_pass(&db, 256, 1_000_000).expect("pass");
    assert_eq!(report.pairs, Vec::new(), "outside the closed classes");
}

#[test]
fn provenance_carries_pass_identity_and_rule() {
    let db = SqliteStore::open_in_memory().expect("open");
    seed(&db, &episode("ep1", "body one"), "The service is up.");
    seed(&db, &episode("ep2", "body two"), "The service is not up.");

    contradict::run_contradiction_pass(&db, 256, 1_000_000).expect("pass");
    let rows = db.edges_with_provenance().expect("read");
    assert_eq!(rows.len(), 1);
    let (_, label, _, provenance) = &rows[0];
    assert_eq!(*label, EdgeLabel::Contradicts);
    let provenance = provenance.as_ref().expect("provenance recorded");
    let value: serde_json::Value = serde_json::from_str(provenance).expect("json provenance");
    assert_eq!(
        value["pass"].as_str(),
        Some(contradict::PASS_ID),
        "pass identity present"
    );
    assert!(
        value["rule"].as_str().is_some(),
        "detection rule present: {provenance}"
    );
}

#[test]
fn numeric_rule_is_recorded_as_numeric() {
    let db = SqliteStore::open_in_memory().expect("open");
    seed(
        &db,
        &episode("ep1", "body one"),
        "The timeout is 30 seconds.",
    );
    seed(
        &db,
        &episode("ep2", "body two"),
        "The timeout is 60 seconds.",
    );

    contradict::run_contradiction_pass(&db, 256, 1_000_000).expect("pass");
    let rows = db.edges_with_provenance().expect("read");
    assert_eq!(rows.len(), 1);
    let provenance = rows[0].3.as_ref().expect("provenance recorded");
    assert!(provenance.contains("numeric"), "rule named: {provenance}");
}

#[test]
fn rerun_refuses_duplicates_without_double_edges() {
    let db = SqliteStore::open_in_memory().expect("open");
    seed(&db, &episode("ep1", "body one"), "The service is up.");
    seed(&db, &episode("ep2", "body two"), "The service is not up.");

    let first = contradict::run_contradiction_pass(&db, 256, 1_000_000).expect("pass");
    assert_eq!(first.proposed, 1);
    assert_eq!(first.refused, 0);

    let second = contradict::run_contradiction_pass(&db, 256, 1_000_000).expect("pass");
    assert_eq!(second.proposed, 0, "re-run proposes nothing new");
    assert_eq!(second.refused, 1, "duplicate refused honestly");
    assert_eq!(contradicts_edges(&db).len(), 1, "never doubled");
}

#[test]
fn budget_exhaustion_is_honest() {
    let db = SqliteStore::open_in_memory().expect("open");
    seed(&db, &episode("ep1", "body one"), "The service is up.");
    seed(&db, &episode("ep2", "body two"), "The service is not up.");
    seed(&db, &episode("ep3", "body three"), "The service is down.");

    // Budget 1: at least one candidate remains unexamined.
    let report = contradict::run_contradiction_pass(&db, 1, 1_000_000).expect("pass");
    assert_eq!(
        report.status,
        BudgetStatus::BudgetExhausted,
        "silent complete would be a lie"
    );
    assert!(
        contradicts_edges(&db).len() <= 1,
        "budget bounds what was written"
    );
}

#[test]
fn pass_is_deterministic() {
    let run = || {
        let db = SqliteStore::open_in_memory().expect("open");
        seed(
            &db,
            &episode("ep1", "body one"),
            "The timeout is 30 seconds.",
        );
        seed(&db, &episode("ep2", "body two"), "The service is not up.");
        seed(&db, &episode("ep3", "body three"), "The service is up.");
        seed(
            &db,
            &episode("ep4", "body four"),
            "The timeout is 60 seconds.",
        );
        contradict::run_contradiction_pass(&db, 256, 1_000_000).expect("pass")
    };
    let first = run();
    let second = run();
    assert_eq!(first, second, "same input, same report");
}

#[test]
fn dump_recreate_preserves_edge_provenance() {
    let db = SqliteStore::open_in_memory().expect("open");
    seed(&db, &episode("ep1", "body one"), "The service is up.");
    seed(&db, &episode("ep2", "body two"), "The service is not up.");
    contradict::run_contradiction_pass(&db, 256, 1_000_000).expect("pass");

    let dump = db.dump_extraction_output().expect("dump");
    let rebuilt = SqliteStore::open_in_memory().expect("open");
    rebuilt.restore_extraction_output(&dump).expect("restore");

    let original = db.edges_with_provenance().expect("read");
    let restored = rebuilt.edges_with_provenance().expect("read");
    assert_eq!(original, restored, "provenance survives rebuild");
    assert!(restored[0].3.is_some(), "provenance is not lost");
}

#[test]
fn legacy_edge_rows_read_with_absent_provenance() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, &episode("ep1", "body one"), "The service is up.");
    let b = seed(&db, &episode("ep2", "body two"), "The service is not up.");
    // The bare-triple writer: no provenance (the legacy/human path).
    db.insert_edge(a, EdgeLabel::Contradicts, b).expect("edge");
    let rows = db.edges_with_provenance().expect("read");
    assert_eq!(rows.len(), 1);
    assert!(rows[0].3.is_none(), "bare triple carries no provenance");
}

// --- property: p_contradicts_provenance ----------------------------------

proptest! {
    // p_contradicts_provenance: every contradicts edge the pass writes
    // carries pass identity and detection-rule provenance; a re-run
    // refuses the identical edge rather than doubling it; pairs outside
    // the closed edit classes never propose.
    #[test]
    fn p_contradicts_provenance(
        subject in "[a-z]{3,10}",
        predicate in "[a-z]{3,12}",
        n1 in 0u32..1000,
        n2 in 0u32..1000,
        negate in proptest::bool::ANY,
        mismatch in proptest::bool::ANY,
    ) {
        prop_assume!(n1 != n2);
        let db = SqliteStore::open_in_memory().expect("open");
        let text_a = format!("The {subject} is {predicate} {n1} times");
        // Exactly one edit class at a time: negation alone, numeral
        // alone, both together (outside BOTH classes), or neither.
        let number_b = if mismatch { n2 } else { n1 };
        let not_flag = if negate { "not " } else { "" };
        let text_b = format!("The {subject} is {not_flag}{predicate} {number_b} times");
        seed(&db, &episode("ep1", "body one"), &text_a);
        seed(&db, &episode("ep2", "body two"), &text_b);

        let report = contradict::run_contradiction_pass(&db, 256, 1_000_000).expect("pass");
        let rows = db.edges_with_provenance().expect("read");

        // Whatever the pass wrote, it carries provenance with pass + rule.
        for (_, label, _, provenance) in &rows {
            prop_assert!(*label == EdgeLabel::Contradicts, "pass writes only contradicts");
            let value: serde_json::Value =
                serde_json::from_str(provenance.as_ref().expect("provenance")).unwrap();
            prop_assert!(value["pass"].as_str() == Some(contradict::PASS_ID));
            prop_assert!(value["rule"].as_str().is_some());
        }

        // The closed classes decide: negation (exactly one side
        // negated) or a single differing numeral — but a negated text
        // with a differing numeral differs in token count too, which is
        // outside both classes. XOR, not OR.
        let want_pairs = if negate ^ mismatch { 1 } else { 0 };
        prop_assert_eq!(report.pairs.len(), want_pairs);

        // Re-run: identical edges refused, never doubled.
        let rerun = contradict::run_contradiction_pass(&db, 256, 1_000_000).expect("pass");
        prop_assert_eq!(rerun.proposed, 0);
        prop_assert_eq!(rerun.refused, report.pairs.len());
        prop_assert_eq!(rows.len(), contradicts_edges(&db).len());
    }
}

// --- CLI wiring -----------------------------------------------------------

#[test]
fn cli_scan_emits_envelope_and_writes_edges() {
    let dir = std::env::temp_dir().join(format!(
        "bajan-scan-{}-{}",
        std::process::id(),
        NAME_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let db_path = dir.join("b.sqlite");
    let db = SqliteStore::open(db_path.to_str().unwrap()).expect("open");
    seed(&db, &episode("ep1", "body one"), "The service is up.");
    seed(&db, &episode("ep2", "body two"), "The service is not up.");
    drop(db);

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_bajan"))
        .arg("--db")
        .arg(db_path.to_str().unwrap())
        .arg("--json")
        .arg("scan")
        .output()
        .expect("binary runs");
    assert!(output.status.success(), "scan exits 0: {output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("envelope");
    assert_eq!(value["ok"], serde_json::Value::Bool(true), "{value}");
    assert_eq!(
        value["data"]["proposed"], 1,
        "scan proposes the negation pair: {value}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

static NAME_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
