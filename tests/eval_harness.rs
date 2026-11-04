//! Purpose: red-first derivation of specs/eval-claims.md (bajan-2k4) — the
//! eval harness that scores a model extraction run against a versioned
//! golden corpus.
//! Responsibilities: cover every invariant and property of `eval.claims` —
//! versioned corpus loading (`ev_golden_corpus`), deterministic
//! normalization-equivalence + span-containment scoring
//! (`ev_deterministic_scoring`), exact precision/recall with raw counts
//! and one record per (corpus, extractor) pair (`ev_precision_recall`),
//! hedge-dropped as a distinct precision failure (`ev_hedge_scoring`),
//! store-free read-only evaluation (`ev_readonly`), reproducible results
//! rows stamped with corpus/extractor versions (`ev_results_checked_in`),
//! the closed judgment-reason vocabulary (`ev_results-schema`), and the
//! ticket MUST gate: missing metrics emitted as explicit `unknown`, never
//! invented.
//! Rationale: the harness produces the numbers that decide the
//! storage-stack question (evals/results.md) — every scoring decision
//! here is a published-schema decision, so the tests pin the semantics
//! before the implementation exists.

use bajan::eval;
use proptest::prelude::*;
use serde_json::json;

// ---------- fixture helpers (JSON strings — the checked-in format) ----------

fn corpus_json(version: &str, episodes: serde_json::Value) -> String {
    json!({"id": "gold", "version": version, "episodes": episodes}).to_string()
}

fn golden_claim(text: &str, span: &str, hedges: &[&str]) -> serde_json::Value {
    let mut v = json!({"text": text, "source_count": 1, "evidence_span": span});
    if !hedges.is_empty() {
        v["hedges"] = json!(hedges);
    }
    v
}

fn run_json(extractor: &str, claims: serde_json::Value) -> String {
    json!({"run_id": "r1", "extractor_version": extractor, "claims": claims}).to_string()
}

fn candidate(episode_id: &str, text: &str, span: &str) -> serde_json::Value {
    json!({"episode_id": episode_id, "text": text, "evidence_span": span})
}

// ---------- ev_golden_corpus ----------

#[test]
fn corpus_without_version_is_rejected() {
    let no_version = json!({"id": "gold", "episodes": []}).to_string();
    assert!(eval::load_corpus(&no_version).is_err());
    let empty_version = corpus_json("", json!([]));
    assert!(eval::load_corpus(&empty_version).is_err());
}

#[test]
fn golden_corpus_exposes_claims_with_counts_spans_hedges() {
    let episodes = json!([
        {"id": "ep-1", "text": "profits may vary by run. Costs are real.",
         "golden_claims": [
            golden_claim("Costs are real.", "Costs are real.", &[]),
            golden_claim("Profits vary", "profits may vary", &["may"]),
         ]},
        {"id": "ep-2", "text": "no claims here", "golden_claims": []},
    ]);
    let corpus = eval::load_corpus(&corpus_json("2", episodes)).expect("valid corpus");
    assert_eq!(corpus.version, "2");
    assert_eq!(corpus.episodes.len(), 2);
    let ep1 = &corpus.episodes[0];
    assert_eq!(ep1.golden_claims.len(), 2);
    assert_eq!(ep1.golden_claims[0].source_count, 1);
    assert_eq!(ep1.golden_claims[1].hedges, vec!["may".to_string()]);
    assert!(ep1.golden_claims[0].hedges.is_empty());
    assert!(corpus.episodes[1].golden_claims.is_empty());
}

// ---------- ev_deterministic_scoring ----------

#[test]
fn replay_produces_identical_judgments() {
    let episodes = json!([
        {"id": "ep-1", "text": "Costs are real. Profits may vary.",
         "golden_claims": [golden_claim("Costs are real.", "Costs are real.", &[])]},
    ]);
    let corpus = eval::load_corpus(&corpus_json("1", episodes)).expect("valid corpus");
    let claims = json!([candidate("ep-1", "Costs are real.", "Costs are real.")]);
    let run = eval::load_run(&run_json("det-0.1.0", claims)).expect("valid run");
    let a = eval::score(&corpus, &run);
    let b = eval::score(&corpus, &run);
    assert_eq!(a, b, "identical inputs must produce byte-identical scores");
}

#[test]
fn paraphrase_never_matches() {
    // Same subject, different wording — content equivalence is exact
    // normalized-text match, not similarity.
    let episodes = json!([
        {"id": "ep-1", "text": "Costs are real.",
         "golden_claims": [golden_claim("Costs are real.", "Costs are real.", &[])]},
    ]);
    let corpus = eval::load_corpus(&corpus_json("1", episodes)).expect("valid corpus");
    let claims = json!([candidate(
        "ep-1",
        "Costs are actually real.",
        "Costs are real."
    )]);
    let run = eval::load_run(&run_json("det-0.1.0", claims)).expect("valid run");
    let outcome = eval::score(&corpus, &run);
    assert_eq!(outcome.judgments[0].reason, eval::Reason::Spurious);
    assert_eq!(outcome.counts.matched, 0);
}

#[test]
fn span_violation_scores_content_miss_never_match() {
    // Candidate text equals the golden claim but its evidence span does
    // not locate in the episode — a content miss, never a match.
    let episodes = json!([
        {"id": "ep-1", "text": "Costs are real.",
         "golden_claims": [golden_claim("Costs are real.", "Costs are real.", &[])]},
    ]);
    let corpus = eval::load_corpus(&corpus_json("1", episodes)).expect("valid corpus");
    let claims = json!([candidate("ep-1", "Costs are real.", "costs came from mars")]);
    let run = eval::load_run(&run_json("det-0.1.0", claims)).expect("valid run");
    let outcome = eval::score(&corpus, &run);
    assert_eq!(outcome.judgments[0].reason, eval::Reason::ContentMiss);
    assert_eq!(outcome.counts.matched, 0);
    assert_eq!(outcome.counts.content_miss, 1);
}

#[test]
fn duplicate_candidate_is_spurious() {
    // The first candidate consumes the golden claim; the duplicate can
    // only score spurious — precision punishes the double emission.
    let episodes = json!([
        {"id": "ep-1", "text": "Costs are real.",
         "golden_claims": [golden_claim("Costs are real.", "Costs are real.", &[])]},
    ]);
    let corpus = eval::load_corpus(&corpus_json("1", episodes)).expect("valid corpus");
    let claims = json!([
        candidate("ep-1", "Costs are real.", "Costs are real."),
        candidate("ep-1", "Costs are real.", "Costs are real."),
    ]);
    let run = eval::load_run(&run_json("det-0.1.0", claims)).expect("valid run");
    let outcome = eval::score(&corpus, &run);
    assert_eq!(outcome.judgments[0].reason, eval::Reason::Matched);
    assert_eq!(outcome.judgments[1].reason, eval::Reason::Spurious);
    assert_eq!(outcome.counts.matched, 1);
    assert_eq!(outcome.counts.emitted, 2);
}

// ---------- ev_precision_recall ----------

#[test]
fn precision_recall_exact_with_raw_counts() {
    // Seeded: 2 emitted, 1 matched (1 spurious), 2 golden → P=1/2, R=1/2.
    let episodes = json!([
        {"id": "ep-1", "text": "Costs are real. Time is money.",
         "golden_claims": [
            golden_claim("Costs are real.", "Costs are real.", &[]),
            golden_claim("Time is money.", "Time is money.", &[]),
         ]},
    ]);
    let corpus = eval::load_corpus(&corpus_json("1", episodes)).expect("valid corpus");
    let claims = json!([
        candidate("ep-1", "Costs are real.", "Costs are real."),
        candidate("ep-1", "Something invented.", "Time is money."),
    ]);
    let run = eval::load_run(&run_json("det-0.1.0", claims)).expect("valid run");
    let outcome = eval::score(&corpus, &run);
    assert_eq!(outcome.counts.emitted, 2);
    assert_eq!(outcome.counts.matched, 1);
    assert_eq!(outcome.counts.golden, 2);
    assert_eq!(outcome.counts.spurious, 1);
    assert_eq!(outcome.counts.golden_unmatched, 1);
    assert_eq!(outcome.precision, Some(0.5));
    assert_eq!(outcome.recall, Some(0.5));
    assert_eq!(
        outcome.extractor_version, "det-0.1.0",
        "one results record per (corpus version, extractor version) — versions stamped"
    );
    assert_eq!(outcome.corpus_version, "1");
}

// ---------- ev_hedge_scoring ----------

#[test]
fn hedge_drop_is_precision_failure_not_match() {
    // Golden claim carries "may"; candidate text drops it — the content
    // matches but the hedge fidelity fails, distinct from a content miss.
    let episodes = json!([
        {"id": "ep-1", "text": "Profits may vary by run.",
         "golden_claims": [golden_claim("Profits may vary by run.", "Profits may vary by run.", &["may"])]},
    ]);
    let corpus = eval::load_corpus(&corpus_json("1", episodes)).expect("valid corpus");
    let claims = json!([candidate(
        "ep-1",
        "Profits vary by run.",
        "Profits may vary by run."
    )]);
    let run = eval::load_run(&run_json("det-0.1.0", claims)).expect("valid run");
    let outcome = eval::score(&corpus, &run);
    assert_eq!(outcome.judgments[0].reason, eval::Reason::HedgeDropped);
    assert_eq!(
        outcome.counts.matched, 0,
        "a hedge drop is never a plain match"
    );
    assert_eq!(outcome.counts.hedge_dropped, 1);
    assert_eq!(outcome.precision, Some(0.0), "precision failure");
    assert_eq!(outcome.hedge_preservation, Some(0.0));
}

#[test]
fn hedge_preserved_matches_and_added_hedges_are_plain_matches() {
    // Preserving the hedge scores as a match and feeds the preservation
    // rate; ADDING a hedge marker the golden lacks is not scored as a
    // failure — content equivalence ignores hedge tokens on both sides.
    let episodes = json!([
        {"id": "ep-1", "text": "Profits may vary by run. Costs seems high.",
         "golden_claims": [
            golden_claim("Profits may vary by run.", "Profits may vary by run.", &["may"]),
            golden_claim("Costs high.", "Costs seems high.", &[]),
         ]},
    ]);
    let corpus = eval::load_corpus(&corpus_json("1", episodes)).expect("valid corpus");
    let claims = json!([
        candidate(
            "ep-1",
            "Profits may vary by run.",
            "Profits may vary by run."
        ),
        candidate("ep-1", "Costs seems high.", "Costs seems high."),
    ]);
    let run = eval::load_run(&run_json("det-0.1.0", claims)).expect("valid run");
    let outcome = eval::score(&corpus, &run);
    assert_eq!(outcome.counts.matched, 2);
    assert_eq!(outcome.counts.hedge_bearing_golden, 1);
    assert_eq!(outcome.counts.preserved_hedge_matches, 1);
    assert_eq!(outcome.hedge_preservation, Some(1.0));
    assert_eq!(
        outcome.judgments[1].reason,
        eval::Reason::Matched,
        "added hedge is not scored"
    );
}

#[test]
fn hedge_dropped_golden_stays_available_for_clean_candidate() {
    // A hedge-dropping candidate never consumes the golden claim; a later
    // clean candidate can still match it.
    let episodes = json!([
        {"id": "ep-1", "text": "Profits may vary by run.",
         "golden_claims": [golden_claim("Profits may vary by run.", "Profits may vary by run.", &["may"])]},
    ]);
    let corpus = eval::load_corpus(&corpus_json("1", episodes)).expect("valid corpus");
    let claims = json!([
        candidate("ep-1", "Profits vary by run.", "Profits may vary by run."),
        candidate(
            "ep-1",
            "Profits may vary by run.",
            "Profits may vary by run."
        ),
    ]);
    let run = eval::load_run(&run_json("det-0.1.0", claims)).expect("valid run");
    let outcome = eval::score(&corpus, &run);
    assert_eq!(outcome.judgments[0].reason, eval::Reason::HedgeDropped);
    assert_eq!(outcome.judgments[1].reason, eval::Reason::Matched);
    assert_eq!(outcome.counts.preserved_hedge_matches, 1);
}

// ---------- ticket MUST gate: missing metrics are explicit unknown ----------

#[test]
fn unknown_metrics_explicit_when_denominator_zero() {
    let episodes = json!([
        {"id": "ep-1", "text": "plain text only", "golden_claims": []},
    ]);
    let corpus = eval::load_corpus(&corpus_json("1", episodes)).expect("valid corpus");
    // Empty run: no emitted candidates → precision unknown.
    let run = eval::load_run(&run_json("det-0.1.0", json!([]))).expect("valid run");
    let outcome = eval::score(&corpus, &run);
    assert_eq!(
        outcome.precision, None,
        "precision must be unknown, never invented"
    );
    assert_eq!(outcome.recall, None, "no golden claims → recall unknown");
    assert!(outcome.unknown_metrics.contains(&"precision".to_string()));
    assert!(outcome.unknown_metrics.contains(&"recall".to_string()));
    assert!(
        outcome
            .unknown_metrics
            .contains(&"hedge_preservation".to_string())
    );

    // Hedge-free corpus: recall computable, hedge rate unknown.
    let hedge_free = json!([
        {"id": "ep-1", "text": "Costs are real.",
         "golden_claims": [golden_claim("Costs are real.", "Costs are real.", &[])]},
    ]);
    let corpus = eval::load_corpus(&corpus_json("1", hedge_free)).expect("valid corpus");
    let claims = json!([candidate("ep-1", "Costs are real.", "Costs are real.")]);
    let run = eval::load_run(&run_json("det-0.1.0", claims)).expect("valid run");
    let outcome = eval::score(&corpus, &run);
    assert_eq!(outcome.precision, Some(1.0));
    assert_eq!(outcome.recall, Some(1.0));
    assert_eq!(outcome.hedge_preservation, None);
    assert_eq!(
        outcome.unknown_metrics,
        vec!["hedge_preservation".to_string()]
    );
}

// ---------- ev_results_checked_in ----------

#[test]
fn results_row_reproducible_and_version_stamped() {
    let episodes = json!([
        {"id": "ep-1", "text": "Costs are real.",
         "golden_claims": [golden_claim("Costs are real.", "Costs are real.", &[])]},
    ]);
    let claims = json!([candidate("ep-1", "Costs are real.", "Costs are real.")]);
    let corpus1 = eval::load_corpus(&corpus_json("1", episodes.clone())).expect("valid");
    let corpus2 = eval::load_corpus(&corpus_json("2", episodes)).expect("valid");
    let run = eval::load_run(&run_json("det-0.1.0", claims)).expect("valid");

    let row_a = eval::results_row(&eval::score(&corpus1, &run));
    let row_b = eval::results_row(&eval::score(&corpus1, &run));
    assert_eq!(
        row_a, row_b,
        "rerun at identical versions reproduces the row"
    );
    let row_v2 = eval::results_row(&eval::score(&corpus2, &run));
    assert_ne!(row_a, row_v2, "corpus version bump must stamp the row");
    assert!(row_v2.contains("2"), "row carries the corpus version");
    assert!(
        row_v2.contains("det-0.1.0"),
        "row carries the extractor version"
    );
}

// ---------- ev_results-schema ----------

#[test]
fn reason_vocabulary_closed_in_output() {
    // Build an outcome exercising all four reasons; the serialized record
    // must use only the published vocabulary.
    let episodes = json!([
        {"id": "ep-1", "text": "Profits may vary. Costs are real. Time is money.",
         "golden_claims": [
            golden_claim("Profits may vary.", "Profits may vary.", &["may"]),
            golden_claim("Costs are real.", "Costs are real.", &[]),
         ]},
    ]);
    let corpus = eval::load_corpus(&corpus_json("1", episodes)).expect("valid corpus");
    let claims = json!([
        candidate("ep-1", "Profits vary.", "Profits may vary."), // hedge-dropped
        candidate("ep-1", "Costs are real.", "nowhere"),         // content-miss
        candidate("ep-1", "Costs are real.", "Costs are real."), // matched
        candidate("ep-1", "Invented nonsense.", "Invented nonsense."), // spurious
    ]);
    let run = eval::load_run(&run_json("det-0.1.0", claims)).expect("valid run");
    let outcome = eval::score(&corpus, &run);
    let reasons: Vec<&str> = outcome
        .judgments
        .iter()
        .map(|j| eval::reason_code(j.reason))
        .collect();
    assert_eq!(
        reasons,
        vec!["hedge-dropped", "content-miss", "matched", "spurious"],
        "closed reason vocabulary, kebab-case per the published schema"
    );
    assert_eq!(outcome.counts.hedge_dropped, 1);
    assert_eq!(outcome.counts.content_miss, 1);
    assert_eq!(outcome.counts.spurious, 1);
    assert_eq!(outcome.counts.matched, 1);
}

#[test]
fn hedge_detection_at_text_boundaries() {
    // Regression: a hedge marker at the very start or end of a text is a
    // word boundary too — the boundary check must treat an empty
    // neighbor as a boundary, not a miss.
    let episodes = json!([
        {"id": "ep-1", "text": "may vary. Costs could rise.",
         "golden_claims": [
            golden_claim("may vary.", "may vary.", &["may"]),
            golden_claim("Costs could rise.", "Costs could rise.", &["could"]),
         ]},
    ]);
    let corpus = eval::load_corpus(&corpus_json("1", episodes)).expect("valid corpus");
    let claims = json!([
        candidate("ep-1", "may vary.", "may vary."),
        candidate("ep-1", "Costs could rise.", "Costs could rise."),
    ]);
    let run = eval::load_run(&run_json("det-0.1.0", claims)).expect("valid run");
    let outcome = eval::score(&corpus, &run);
    assert_eq!(
        outcome.judgments[0].reason,
        eval::Reason::Matched,
        "marker at text start"
    );
    assert_eq!(outcome.judgments[1].reason, eval::Reason::Matched);
    assert_eq!(outcome.counts.preserved_hedge_matches, 2);
}

// ---------- properties ----------

proptest! {
    #[test]
    fn p_deterministic_scoring(
        golden_text in "[a-z]{3,6}( [a-z]{3,6}){1,6}",
        cand_text in "[a-z]{3,6}( [a-z]{3,6}){0,6}",
        version in "[0-9]{1,2}",
    ) {
        let episodes = json!([
            {"id": "ep-1", "text": golden_text,
             "golden_claims": [golden_claim(&golden_text, &golden_text, &[])]},
        ]);
        let corpus = eval::load_corpus(&corpus_json(&version, episodes)).expect("valid corpus");
        let claims = json!([candidate("ep-1", &cand_text, &golden_text)]);
        let run = eval::load_run(&run_json("det-0.1.0", claims)).expect("valid run");
        let a = eval::score(&corpus, &run);
        let b = eval::score(&corpus, &run);
        prop_assert_eq!(&a, &b, "replay must produce identical match assignments");
        // Content equivalence is token-sequence equality (the generator
        // alphabet is lowercase+space, so token equality == content-form
        // equality here).
        let cand_tokens: Vec<&str> = cand_text.split_whitespace().collect();
        let gold_tokens: Vec<&str> = golden_text.split_whitespace().collect();
        if cand_tokens != gold_tokens {
            prop_assert_eq!(a.counts.matched, 0, "non-equal content never matches");
        } else {
            prop_assert_eq!(a.counts.matched, 1);
            prop_assert_eq!(a.precision, Some(1.0));
            prop_assert_eq!(a.recall, Some(1.0));
        }
    }

    #[test]
    fn p_hedge_scoring(
        // [a-d ] words only — no marker word can appear in the body, so
        // the dropped-vs-preserved delta is exactly the generated hedge;
        // leading letters guarantee non-whitespace (validation rejects
        // whitespace-only texts).
        body in "[a-d]{2,5}( [a-d]{2,5}){1,4}",
        hedge in proptest::option::of(
            prop_oneof!["may", "might", "could", "possibly", "appears"],
        ),
    ) {
        let golden_text = match &hedge {
            Some(h) => format!("{} {}", body, h),
            None => body.clone(),
        };
        let hedges: Vec<&str> = hedge.iter().map(|h| h.as_str()).collect();
        let episodes = json!([
            {"id": "ep-1", "text": golden_text,
             "golden_claims": [golden_claim(&golden_text, &golden_text, &hedges)]},
        ]);
        let corpus = eval::load_corpus(&corpus_json("1", episodes)).expect("valid corpus");

        // Preserved: verbatim candidate text.
        let kept = json!([candidate("ep-1", &golden_text, &golden_text)]);
        let run = eval::load_run(&run_json("det-0.1.0", kept)).expect("valid run");
        let kept_outcome = eval::score(&corpus, &run);
        prop_assert_eq!(kept_outcome.judgments[0].reason, eval::Reason::Matched);
        prop_assert_eq!(kept_outcome.counts.preserved_hedge_matches, usize::from(hedge.is_some()));

        // Dropped: candidate omits the hedge marker.
        if hedge.is_some() {
            let dropped = json!([candidate("ep-1", &body, &golden_text)]);
            let run = eval::load_run(&run_json("det-0.1.0", dropped)).expect("valid run");
            let dropped_outcome = eval::score(&corpus, &run);
            prop_assert_eq!(dropped_outcome.judgments[0].reason, eval::Reason::HedgeDropped);
            prop_assert_eq!(dropped_outcome.counts.matched, 0);
            prop_assert_eq!(dropped_outcome.hedge_preservation, Some(0.0));
        }
    }
}

// ---------- CLI binary contract (ticket METER) ----------

fn write_temp(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, content).expect("write fixture");
    path
}

fn corpus_fixture_text() -> String {
    json!({
        "id": "gold-smoke", "version": "1",
        "episodes": [{
            "id": "ep-1",
            "text": "Costs are real. Profits may vary.",
            "golden_claims": [
                {"text": "Costs are real.", "source_count": 1,
                 "evidence_span": "Costs are real."},
                {"text": "Profits may vary.", "source_count": 1,
                 "evidence_span": "Profits may vary.", "hedges": ["may"]},
            ],
        }],
    })
    .to_string()
}

fn bajan(args: &[std::path::PathBuf]) -> (i32, String) {
    let strs: Vec<String> = args
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let refs: Vec<&str> = strs.iter().map(|s| s.as_str()).collect();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_bajan"))
        .args(&refs)
        .output()
        .expect("binary built by cargo test");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

fn flags(pairs: &[&str]) -> Vec<std::path::PathBuf> {
    pairs.iter().map(|s| std::path::PathBuf::from(*s)).collect()
}

#[test]
fn eval_claims_envelope_reports_denominators_and_unknowns() {
    let dir = std::env::temp_dir().join(format!("bajan-eval-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let corpus = write_temp(&dir, "gold.json", &corpus_fixture_text());
    let run = write_temp(
        &dir,
        "run.json",
        &run_json(
            "llm-deepseek/deepseek-v4-flash-0.1.0",
            json!([candidate("ep-1", "Costs are real.", "Costs are real.")]),
        ),
    );

    // Populated denominators: non-null metrics, empty unknown list.
    let mut args = flags(&["eval", "claims", "--json", "--corpus"]);
    args.push(corpus.clone());
    args.push(std::path::PathBuf::from("--run"));
    args.push(run.clone());
    let (code, stdout) = bajan(&args);
    assert_eq!(code, 0, "ok envelope exits zero");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope on stdout");
    assert_eq!(v["ok"].as_bool(), Some(true));
    assert!(!v["data"]["precision"].is_null(), "denominator non-null");
    assert!(!v["data"]["recall"].is_null());
    assert_eq!(v["data"]["unknown_metrics"], json!([]));
    assert_eq!(v["data"]["counts"]["emitted"], json!(1));
    assert_eq!(v["data"]["counts"]["golden"], json!(2));
    assert_eq!(v["data"]["corpus_version"], json!("1"));

    // Empty run: precision null AND the unknown-metric key explicit.
    let empty_run = write_temp(&dir, "empty-run.json", &run_json("det-0.1.0", json!([])));
    let mut args = flags(&["eval", "claims", "--json", "--corpus"]);
    args.push(corpus);
    args.push(std::path::PathBuf::from("--run"));
    args.push(empty_run);
    let (code, stdout) = bajan(&args);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope");
    assert!(v["data"]["precision"].is_null(), "unknown, never invented");
    assert_eq!(
        v["data"]["unknown_metrics"],
        json!(["precision"]),
        "unknown-metric keys explicit"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn eval_claims_invalid_corpus_emits_error_envelope() {
    let dir = std::env::temp_dir().join(format!("bajan-eval-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let corpus = write_temp(
        &dir,
        "bad.json",
        &json!({"id": "gold", "episodes": []}).to_string(),
    );
    let run = write_temp(&dir, "run.json", &run_json("det-0.1.0", json!([])));
    let (code, stdout) = bajan(
        flags(&[
            "eval",
            "claims",
            "--json",
            "--corpus",
            corpus.to_str().expect("utf8 path"),
            "--run",
            run.to_str().expect("utf8 path"),
        ])
        .as_slice(),
    );
    assert_ne!(code, 0, "invalid corpus must exit non-zero");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope on stdout");
    assert_eq!(v["ok"].as_bool(), Some(false));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn eval_never_touches_store() {
    // ev_readonly at the process boundary: the eval command must not
    // create or open the store db passed via the global --db flag.
    let dir = std::env::temp_dir().join(format!("bajan-eval-ro-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let db = dir.join("must-not-exist.db");
    let corpus = write_temp(&dir, "gold.json", &corpus_fixture_text());
    let run = write_temp(
        &dir,
        "run.json",
        &run_json(
            "det-0.1.0",
            json!([candidate("ep-1", "Costs are real.", "Costs are real.")]),
        ),
    );
    let mut args = flags(&[
        "--db",
        db.to_str().expect("utf8 path"),
        "eval",
        "claims",
        "--json",
        "--corpus",
    ]);
    args.push(corpus);
    args.push(std::path::PathBuf::from("--run"));
    args.push(run);
    let (code, _) = bajan(&args);
    assert_eq!(code, 0);
    assert!(!db.exists(), "eval must never open a store (ev_readonly)");
    let _ = std::fs::remove_dir_all(&dir);
}
