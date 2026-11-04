//! Purpose: red-first tests for the real LLM claim extractor (bajan-vg6)
//! — `ex_single_call` over the bajan-9av seam. Responsibilities: strict
//! JSON output schema (deny-unknown-fields, fence-stripped deterministically),
//! chunk assembly (text-splitter + tiktoken, pinned — concatenated chunks
//! equal the episode text verbatim, ONE call per episode per pass per
//! CORR-001), retry with backoff inside the call and park after budget
//! exhaustion (`ex_single_call` abort; cache-absence requeue per
//! `ex_park_requeue` — a parked episode holds no cache entry so the next
//! pass re-attempts it with a fresh budget and prior parked run rows
//! persist), hallucinated evidence spans gate-rejected unmodified.
//! Rationale: scripted-transport tests (no real API key in CI) plus one
//! real-HTTP end-to-end through the CLI binary against a local fake
//! OpenAI-compatible server.

use bajan::extract::FailureClass;
use bajan::extract::{
    ExtractionFailure, Extractor, ExtractorConfig, Finish, Reason, run_extract_with,
};
use bajan::ingest::{self, EpisodeRecord, Locator, SourceMeta};
use bajan::llm::{
    LlmExtractor, LlmTransport, chunk_episode, jittered_backoff, llm_backoff, strip_json_fence,
};
use bajan::store::sqlite::SqliteStore;
use bajan::store::{ClaimStatus, Evidence};
use proptest::prelude::*;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

fn episode(id: &str, text: &str) -> EpisodeRecord {
    EpisodeRecord {
        id: id.into(),
        text: text.into(),
        locator: Locator::Span("heading:H".into()),
        source: SourceMeta {
            source_type: "markdown".into(),
            data_cutoff: Some("2026-01-01".into()),
            authority_tier: 2,
            tags: vec!["workspace:dev".into()],
        },
    }
}

fn claims_content(claims: serde_json::Value) -> String {
    serde_json::json!({ "claims": claims }).to_string()
}

/// An OpenAI-compatible chat-completions success body with the given
/// message content.
fn openai_ok(content: &str) -> String {
    serde_json::json!({
        "choices": [{ "message": { "role": "assistant", "content": content } }]
    })
    .to_string()
}

/// Scripted transport shared between the extractor and the test: pops
/// the next step per call, records request bodies. An exhausted script
/// fails every further call.
#[derive(Debug, Clone)]
struct Script(Arc<ScriptInner>);

#[derive(Debug)]
struct ScriptInner {
    steps: Mutex<VecDeque<Result<String, String>>>,
    bodies: Mutex<Vec<String>>,
}

impl Script {
    fn new(steps: Vec<Result<String, String>>) -> Self {
        Self(Arc::new(ScriptInner {
            steps: Mutex::new(steps.into()),
            bodies: Mutex::new(Vec::new()),
        }))
    }

    fn bodies(&self) -> Vec<String> {
        self.0.bodies.lock().expect("bodies lock").clone()
    }

    fn calls(&self) -> usize {
        self.0.bodies.lock().expect("bodies lock").len()
    }
}

impl LlmTransport for Script {
    fn post(&self, _base_url: &str, _api_key: &str, body_json: &str) -> Result<String, String> {
        self.0
            .bodies
            .lock()
            .expect("bodies lock")
            .push(body_json.to_string());
        self.0
            .steps
            .lock()
            .expect("steps lock")
            .pop_front()
            .unwrap_or_else(|| Err("script exhausted".into()))
    }
}

/// The standard test rig: no-op sleep, fixed key, one script.
fn llm(script: Script) -> LlmExtractor {
    llm_with(script, 3)
}

fn llm_with(script: Script, max_attempts: u32) -> LlmExtractor {
    LlmExtractor::new(
        "test-model",
        "http://localhost/v1",
        "TEST_API_KEY",
        1024,
        max_attempts,
    )
    .with_transport(script)
    .with_sleep(Box::new(|_| {}))
    .with_key_lookup(Box::new(|_| Some("sk-test".into())))
}

fn staged_texts(db: &SqliteStore) -> Vec<String> {
    db.claims_with_lineage()
        .expect("claims read back")
        .into_iter()
        .filter(|(_, node, _)| node.status == ClaimStatus::Staged)
        .map(|(_, node, _)| node.text)
        .collect()
}

fn finish_of(runs: &bajan::extract::ExtractionRunStore) -> Vec<&Finish> {
    runs.rows().map(|row| &row.finish).collect()
}

// --- proposal through the seam ----------------------------------------------

/// A successful call produces candidates from the strict JSON output
/// schema: claim text, verbatim evidence span (episode locator attached
/// by the extractor), optional temporal bounds. Model id and version
/// land on the extractor surface.
#[test]
fn llm_extractor_proposes_candidates() {
    let script = Script::new(vec![Ok(openai_ok(&claims_content(serde_json::json!([
        { "text": "A is true.", "evidence": "A is true.", "valid_at": null, "invalid_at": null },
        { "text": "B follows.", "evidence": "B follows.", "valid_at": "2025-01-01", "invalid_at": null }
    ]))))]);
    let extractor = llm(script);
    assert_eq!(extractor.model_id(), Some("test-model"));
    assert_eq!(
        extractor.version(),
        format!("llm-test-model-{}", env!("CARGO_PKG_VERSION")),
        "cache identity: llm namespace + model + package version"
    );
    let ep = episode("ep-1", "A is true. B follows.");
    let candidates = extractor.propose(&ep).expect("propose succeeds");
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].text, "A is true.");
    assert_eq!(
        candidates[0].evidence,
        Evidence::Span {
            text: "A is true.".into(),
            locator: "heading:H".into()
        }
    );
    assert_eq!(candidates[1].valid_at.as_deref(), Some("2025-01-01"));
}

/// The full run stages LLM candidates, and the run row carries the
/// extractor's model id and version (ex_run_record).
#[test]
fn run_extract_stages_llm_candidates_with_model_run_row() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(&db, &episode("ep-1", "A is true. B follows.")).expect("persist");
    let script = Script::new(vec![Ok(openai_ok(&claims_content(serde_json::json!([
        { "text": "A is true.", "evidence": "A is true." }
    ]))))]);
    let extractor = llm(script);
    let mut runs = bajan::extract::ExtractionRunStore::default();
    let report = run_extract_with(&db, &extractor, &mut runs).expect("run succeeds");
    assert_eq!(report.candidates_proposed, 1);
    assert_eq!(report.parked.len(), 0);
    assert_eq!(staged_texts(&db), vec!["A is true.".to_string()]);
    let rows: Vec<_> = runs.rows().collect();
    assert_eq!(rows.len(), 1, "one run row per extraction call");
    assert_eq!(rows[0].model_id.as_deref(), Some("test-model"));
    assert_eq!(rows[0].extractor_version, extractor.version());
    assert!(matches!(rows[0].finish, Finish::Succeeded));
}

/// A hallucinated evidence span (not a substring of the episode text) is
/// gate-rejected with evidence_not_contained — never repaired (the typed
/// gate, not the extractor, owns containment).
#[test]
fn gate_rejects_hallucinated_evidence() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(&db, &episode("ep-1", "A is true. B follows.")).expect("persist");
    let script = Script::new(vec![Ok(openai_ok(&claims_content(serde_json::json!([
        { "text": "C was invented.", "evidence": "C was invented." }
    ]))))]);
    let extractor = llm(script);
    let mut runs = bajan::extract::ExtractionRunStore::default();
    let report = run_extract_with(&db, &extractor, &mut runs).expect("run itself succeeds");
    assert_eq!(report.gate_rejections.len(), 1);
    assert_eq!(
        report.gate_rejections[0].reason,
        Reason::EvidenceNotContained
    );
    assert!(staged_texts(&db).is_empty(), "nothing staged");
}

/// An empty claims array is a legitimate zero-candidate outcome: cached
/// as empty (`ex_typed_gate`), no re-extraction at the same version.
#[test]
fn zero_claims_caches_as_empty() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(&db, &episode("ep-1", "A is true. B follows.")).expect("persist");
    let extractor = llm(Script::new(vec![Ok(openai_ok(&claims_content(
        serde_json::json!([]),
    )))]));
    let report = run_extract_with(&db, &extractor, &mut Default::default()).expect("run succeeds");
    assert_eq!(report.candidates_proposed, 0);
    assert_eq!(report.parked.len(), 0);
    let report2 = run_extract_with(&db, &extractor, &mut Default::default()).expect("second run");
    assert_eq!(report2.episodes_processed, 0, "cached as empty");
}

// --- strict output schema -----------------------------------------------------

/// Malformed model output (content is not JSON) is an infrastructure
/// failure — retryable, and after budget exhaustion the call fails.
#[test]
fn malformed_content_is_infrastructure_failure() {
    let extractor = llm_with(Script::new(vec![Ok(openai_ok("this is not json"))]), 1);
    let ep = episode("ep-1", "A is true.");
    let err = extractor.propose(&ep).expect_err("infrastructure failure");
    assert!(matches!(
        err,
        ExtractionFailure::Exhausted {
            attempts: 1,
            class: _
        }
    ));
}

/// Unknown fields in a claim record are refused (strict published schema
/// — drift is caught, not silently tolerated).
#[test]
fn unknown_output_fields_refused() {
    let extractor = llm_with(
        Script::new(vec![Ok(openai_ok(&claims_content(serde_json::json!([
            { "text": "A is true.", "evidence": "A is true.", "confidence": 0.9 }
        ]))))]),
        1,
    );
    let ep = episode("ep-1", "A is true.");
    assert!(extractor.propose(&ep).is_err(), "unknown field refused");
}

/// A missing required field (evidence) is refused — requiredness is part
/// of the published schema.
#[test]
fn missing_required_fields_refused() {
    let extractor = llm_with(
        Script::new(vec![Ok(openai_ok(&claims_content(
            serde_json::json!([{ "text": "A is true." }]),
        )))]),
        1,
    );
    let ep = episode("ep-1", "A is true.");
    assert!(extractor.propose(&ep).is_err(), "missing evidence refused");
}

/// Fenced JSON content (```json ... ```) is stripped deterministically —
/// the fence is a formatting artifact, not content.
#[test]
fn fenced_json_content_parses() {
    let fenced = format!(
        "```json\n{}\n```",
        claims_content(serde_json::json!([
            { "text": "A is true.", "evidence": "A is true." }
        ]))
    );
    let extractor = llm(Script::new(vec![Ok(openai_ok(&fenced))]));
    let candidates = extractor
        .propose(&episode("ep-1", "A is true."))
        .expect("parses");
    assert_eq!(candidates.len(), 1);
}

/// The fence stripper is a deterministic pure function.
#[test]
fn strip_json_fence_is_deterministic() {
    assert_eq!(strip_json_fence("```json\n{\"a\":1}\n```"), "{\"a\":1}");
    assert_eq!(strip_json_fence("```\n{\"a\":1}\n```"), "{\"a\":1}");
    assert_eq!(
        strip_json_fence(" {\"a\":1} "),
        "{\"a\":1}",
        "unfenced: trimmed only"
    );
    assert_eq!(strip_json_fence("{\"a\":1}"), "{\"a\":1}");
}

/// A missing API key at call time is an infrastructure failure (retry →
/// park), never a panic and never a gate rejection.
#[test]
fn missing_api_key_is_infrastructure_failure() {
    let extractor = LlmExtractor::new("test-model", "http://localhost/v1", "ABSENT_KEY", 1024, 1)
        .with_transport(Script::new(vec![]))
        .with_sleep(Box::new(|_| {}))
        .with_key_lookup(Box::new(|_| None));
    let ep = episode("ep-1", "A is true.");
    let err = extractor.propose(&ep).expect_err("infrastructure failure");
    assert!(matches!(
        err,
        ExtractionFailure::Exhausted {
            attempts: 1,
            class: _
        }
    ));
}

/// The prompt is a pure function of the episode text and configuration:
/// the same episode produces byte-identical request bodies.
#[test]
fn prompt_is_deterministic() {
    let ep = episode("ep-1", "A is true. B follows.");
    let script_a = Script::new(vec![Ok(openai_ok(&claims_content(serde_json::json!([]))))]);
    let script_b = Script::new(vec![Ok(openai_ok(&claims_content(serde_json::json!([]))))]);
    let a = llm(script_a.clone());
    let b = llm(script_b.clone());
    let _ = a.propose(&ep);
    let _ = b.propose(&ep);
    assert_eq!(
        script_a.bodies(),
        script_b.bodies(),
        "identical episodes → identical prompts"
    );
    assert_eq!(script_a.calls(), 1);
}

// --- retry, park, requeue -----------------------------------------------------

/// Failed calls retry with backoff (sleep called between attempts) and a
/// mid-budget success still produces candidates.
#[test]
fn retries_then_succeeds() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let sleeps = Arc::new(AtomicUsize::new(0));
    let counter = sleeps.clone();
    let script = Script::new(vec![
        Err("http 503".into()),
        Ok(openai_ok(&claims_content(serde_json::json!([
            { "text": "A is true.", "evidence": "A is true." }
        ])))),
    ]);
    let extractor = llm(script).with_sleep(Box::new(move |d| {
        let _ = d;
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    let candidates = extractor
        .propose(&episode("ep-1", "A is true."))
        .expect("second attempt succeeds");
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        sleeps.load(Ordering::SeqCst),
        1,
        "one backoff sleep between two attempts"
    );
}

/// Budget exhaustion parks the episode: the report carries the machine-
/// readable reason, the run row finishes Parked, nothing is staged, and
/// the episode holds NO cache entry (ex_single_call abort).
#[test]
fn parks_after_budget_exhaustion() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(&db, &episode("ep-1", "A is true.")).expect("persist");
    let script = Script::new(vec![Err("http 500".into()); 3]);
    let extractor = llm_with(script.clone(), 3);
    let mut runs = bajan::extract::ExtractionRunStore::default();
    let report = run_extract_with(&db, &extractor, &mut runs).expect("park is not a run failure");
    assert_eq!(report.parked.len(), 1);
    assert_eq!(report.parked[0].episode_id, "ep-1");
    assert_eq!(
        report.parked[0].reason,
        Reason::RepeatedCallFailure {
            attempts: 3,
            class: FailureClass::HttpStatus { code: 500 },
        }
    );
    assert_eq!(
        finish_of(&runs),
        vec![&Finish::Parked {
            reason: Reason::RepeatedCallFailure {
                attempts: 3,
                class: FailureClass::HttpStatus { code: 500 },
            }
        }]
    );
    assert!(staged_texts(&db).is_empty());
    assert_eq!(script.calls(), 3, "budget = total attempts");
}

/// A parked episode holds no cache entry, so the next pass re-attempts
/// it with a fresh budget (ex_park_requeue); the prior parked run row
/// persists with its reason; success ends the requeue cycle.
#[test]
fn parked_episode_requeues_with_fresh_budget() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(&db, &episode("ep-1", "A is true.")).expect("persist");
    let failing = llm_with(Script::new(vec![Err("http 500".into()); 3]), 3);
    let mut runs = bajan::extract::ExtractionRunStore::default();
    let first = run_extract_with(&db, &failing, &mut runs).expect("pass 1 parks");
    assert_eq!(first.parked.len(), 1);

    let succeeding = llm(Script::new(vec![Ok(openai_ok(&claims_content(
        serde_json::json!([{ "text": "A is true.", "evidence": "A is true." }]),
    )))]));
    let second = run_extract_with(&db, &succeeding, &mut runs).expect("pass 2 succeeds");
    assert_eq!(second.episodes_processed, 1, "fresh budget: re-attempted");
    assert_eq!(second.candidates_proposed, 1);
    assert_eq!(staged_texts(&db), vec!["A is true.".to_string()]);
    // Prior parked row persists beside the new success row.
    let finishes = finish_of(&runs);
    assert_eq!(finishes.len(), 2);
    assert!(matches!(finishes[0], Finish::Parked { .. }));
    assert!(matches!(finishes[1], Finish::Succeeded));
}

/// Renewed exhaustion parks again with a recorded reason — the park is
/// one run's honest stopping, never a permanent state.
#[test]
fn renewed_exhaustion_parks_again() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(&db, &episode("ep-1", "A is true.")).expect("persist");
    let mut runs = bajan::extract::ExtractionRunStore::default();
    let pass1 = run_extract_with(
        &db,
        &llm_with(Script::new(vec![Err("http 500".into()); 2]), 2),
        &mut runs,
    )
    .expect("pass 1 parks");
    assert_eq!(pass1.parked.len(), 1);
    let pass2 = run_extract_with(
        &db,
        &llm_with(Script::new(vec![Err("http 500".into()); 2]), 2),
        &mut runs,
    )
    .expect("pass 2 parks again");
    assert_eq!(pass2.parked.len(), 1);
    assert_eq!(
        finish_of(&runs).len(),
        2,
        "both passes wrote their own rows"
    );
}

/// Budget exhaustion surfaces as `Exhausted` on the extractor seam —
/// the pipeline decides the park (run row + cache absence), the
/// extractor never misreports it as a gate-level outcome.
#[test]
fn budget_exhaustion_surfaces_as_exhausted() {
    let extractor = llm_with(Script::new(vec![Err("connection refused".into()); 3]), 3);
    let err = extractor
        .propose(&episode("ep-1", "A is true."))
        .expect_err("exhausted after 3 attempts");
    assert!(matches!(
        err,
        ExtractionFailure::Exhausted {
            attempts: 3,
            class: _
        }
    ));
}

// --- chunk assembly (CORR-001) -------------------------------------------------

// Chunking is assembly for exactly one call: concatenated chunks equal
// the episode text VERBATIM (trim disabled), regardless of budget.
proptest! {
    #[test]
    fn p_chunk_assembly_preserves_text(
        sentences in proptest::collection::vec("[a-záéíóúñ]{3,40}", 1..10),
        budget in 8u32..64,
    ) {
        let text: String = sentences.iter().map(|s| format!("{s}. ")).collect();
        let chunks = chunk_episode(&text, budget as usize);
        prop_assert!(!chunks.is_empty());
        let rejoined: String = chunks.concat();
        prop_assert_eq!(rejoined, text, "verbatim chunk assembly (trim disabled)");
    }

    #[test]
    fn p_under_budget_text_is_one_chunk(text in "[a-z0-9 ]{1,50}") {
        let chunks = chunk_episode(&text, usize::MAX);
        prop_assert_eq!(chunks.len(), 1);
        prop_assert_eq!(&chunks[0], &text);
    }
}

/// Tiny budgets still make progress (char-level fallback) without
/// losing text.
#[test]
fn tiny_budget_still_preserves_text() {
    let text = "First sentence here. Second sentence follows.";
    let chunks = chunk_episode(text, 1);
    let rejoined: String = chunks.concat();
    assert_eq!(rejoined, text);
}

// --- backoff schedule -----------------------------------------------------------

/// Backoff is exponential from 2s, capped at 60s (bajan-266, burn-in F2:
/// provider-load windows outlast the old 500ms→8s schedule — 3 attempts
/// waited ~1.5s total and 2/12 parked episodes recovered only on a
/// cross-pass rerun).
#[test]
fn backoff_is_exponential_and_capped() {
    assert_eq!(llm_backoff(1), std::time::Duration::from_millis(2_000));
    assert_eq!(llm_backoff(2), std::time::Duration::from_millis(4_000));
    assert_eq!(llm_backoff(3), std::time::Duration::from_millis(8_000));
    assert_eq!(
        llm_backoff(20),
        std::time::Duration::from_millis(60_000),
        "capped"
    );
}

/// Jitter decorrelates concurrent passes so simultaneous reruns don't
/// sync their retries onto the same load window: every sampled sleep is
/// within [half, full) of the schedule's deterministic backoff.
#[test]
fn jittered_backoff_stays_within_half_of_schedule() {
    for attempt in 1..=6u32 {
        let full = llm_backoff(attempt).as_millis();
        for _ in 0..200 {
            let got = jittered_backoff(attempt).as_millis();
            assert!(
                got >= full / 2 && got <= full,
                "attempt {attempt}: {got}ms outside [{}/2, {full}]",
                full
            );
        }
    }
}

/// The retry loop itself sleeps within the jittered bounds — captured
/// via the injected sleep (durations in [1s,2s] then [2s,4s] for
/// attempts 1 and 2 of a mid-budget success).
#[test]
fn retrying_call_sleeps_within_backoff_bounds() {
    let script = Script::new(vec![
        Err("http 503".into()),
        Err("http 503".into()),
        Ok(openai_ok(&claims_content(serde_json::json!([
            { "text": "A is true.", "evidence": "A is true." }
        ])))),
    ]);
    let sleeps: Arc<Mutex<Vec<std::time::Duration>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = sleeps.clone();
    let extractor = llm_with(script, 3).with_sleep(Box::new(move |d| {
        sink.lock().expect("sleeps lock").push(d);
    }));
    extractor
        .propose(&episode("ep-1", "A is true."))
        .expect("third attempt succeeds");
    let got = sleeps.lock().expect("sleeps lock").clone();
    assert_eq!(got.len(), 2, "one sleep per failed attempt");
    let (first_ms, second_ms) = (got[0].as_millis(), got[1].as_millis());
    assert!((1_000..=2_000).contains(&first_ms), "sleep 1: {first_ms}ms");
    assert!(
        (2_000..=4_000).contains(&second_ms),
        "sleep 2: {second_ms}ms"
    );
}

// --- config surface --------------------------------------------------------------

/// kind = llm with a complete parameter surface (including the
/// mandatory spend cap, bajan-8cc) now selects the real LLM extractor
/// (bajan-vg6 replaced the honest NotImplemented gate).
#[test]
fn llm_full_config_selects_extractor() {
    let config = ExtractorConfig {
        kind: "llm".into(),
        model_id: Some("gpt-4o-mini".into()),
        base_url: Some("https://api.example.com/v1".into()),
        api_key_env: Some("MY_API_KEY".into()),
        max_spend_usd: Some("0.50".into()),
        ..Default::default()
    };
    let extractor = config.select().expect("llm selects");
    assert_eq!(extractor.model_id(), Some("gpt-4o-mini"));
    assert_eq!(
        extractor.version(),
        format!("llm-gpt-4o-mini-{}", env!("CARGO_PKG_VERSION"))
    );
}

/// Missing llm params still fail at selection with machine-readable
/// errors (unchanged from bajan-9av).
#[test]
fn llm_missing_params_still_refused_at_selection() {
    let config = ExtractorConfig {
        kind: "llm".into(),
        model_id: Some("m".into()),
        ..Default::default()
    };
    let err = config.select().expect_err("base_url missing");
    assert!(matches!(
        err,
        bajan::extract::ExtractorConfigError::MissingParam {
            param: "BAJAN_EXTRACTOR_BASE_URL"
        }
    ));
}

// --- real-HTTP end-to-end ---------------------------------------------------------

/// Minimal hand-rolled HTTP/1.1 fake OpenAI server: serves the scripted
/// (status, body) responses, one per connection.
fn spawn_fake_openai(responses: Vec<(u16, String)>) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for (status, body) in responses {
            let (mut stream, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(_) => return,
            };
            // Read the full request (headers + Content-Length body).
            let mut buf: Vec<u8> = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&buf[..pos]).to_lowercase();
                            let len: usize = headers
                                .lines()
                                .find_map(|l| l.strip_prefix("content-length:"))
                                .and_then(|v| v.trim().parse().ok())
                                .unwrap_or(0);
                            if buf.len() >= pos + 4 + len {
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            let resp = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });
    format!("http://127.0.0.1:{port}/v1")
}

/// The real transport path: the CLI binary extracts through a local
/// fake OpenAI-compatible server — no API key value anywhere in config,
/// only the env-var name.
#[test]
fn cli_llm_extract_end_to_end_through_real_http() {
    let dir =
        std::env::temp_dir().join(format!("bajan-vg6-cli-{}-{}", std::process::id(), line!()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let db_path = dir.join("t.db");
    let db_path = db_path.to_str().expect("utf8").to_string();

    let (code, out) = run_with_env_stdin(
        &[],
        &["--json", "--db", &db_path, "ingest"],
        Some(
            r#"[{"id":"e1","text":"A is true. B follows.","locator":{"kind":"span","value":"heading:H"},"source":{"source_type":"markdown","data_cutoff":null,"authority_tier":1,"tags":[]}}]"#,
        ),
    );
    assert_eq!(code, 0, "{out}");

    let base_url = spawn_fake_openai(vec![(
        200,
        openai_ok(&claims_content(serde_json::json!([
            { "text": "A is true.", "evidence": "A is true." }
        ]))),
    )]);
    let (code, out) = run_with_env(
        &[
            ("BAJAN_EXTRACTOR", "llm"),
            ("BAJAN_EXTRACTOR_MODEL", "fake-model"),
            ("BAJAN_EXTRACTOR_BASE_URL", &base_url),
            ("BAJAN_EXTRACTOR_API_KEY_ENV", "FAKE_KEY"),
            ("BAJAN_MAX_SPEND_USD", "1.00"),
            ("FAKE_KEY", "sk-nothing"),
        ],
        &["--json", "--db", &db_path, "extract"],
    );
    assert_eq!(code, 0, "{out}");
    let envelope: serde_json::Value = serde_json::from_str(&out).expect("valid envelope");
    assert_eq!(envelope["ok"].as_bool(), Some(true));
    assert_eq!(envelope["data"]["candidates_proposed"].as_i64(), Some(1));

    // Query read-back: the LLM claim is staged and searchable.
    let (code, out) = run_with_env(&[], &["--json", "--db", &db_path, "query", "true"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("A is true."), "{out}");

    std::fs::remove_dir_all(&dir).ok();
}

// --- binary harness ---------------------------------------------------------------

use std::process::Output;

fn run_with_env(envs: &[(&str, &str)], args: &[&str]) -> (i32, String) {
    run_with_env_stdin(envs, args, None)
}

fn run_with_env_stdin(envs: &[(&str, &str)], args: &[&str], stdin: Option<&str>) -> (i32, String) {
    let exe = env!("CARGO_BIN_EXE_bajan");
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    if let Some(input) = stdin {
        use std::io::Write;
        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn bajan");
        child
            .stdin
            .as_mut()
            .expect("stdin piped")
            .write_all(input.as_bytes())
            .expect("write stdin");
        let out = child.wait_with_output().expect("wait");
        return (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).to_string(),
        );
    }
    let Output { status, stdout, .. } = cmd.output().expect("run bajan");
    (
        status.code().unwrap_or(-1),
        String::from_utf8_lossy(&stdout).to_string(),
    )
}

// --- spend cap (bajan-8cc) ---------------------------------------------------

/// An OpenAI-compatible success body with the given content AND a
/// `usage.cost` (the field OpenAI-compatible providers report when they
/// bill per call — the only spend signal bajan can account).
fn openai_ok_with_cost(content: &str, cost_usd: f64) -> String {
    serde_json::json!({
        "choices": [{ "message": { "role": "assistant", "content": content } }],
        "usage": { "cost": cost_usd }
    })
    .to_string()
}

fn one_claim(text: &str) -> String {
    claims_content(serde_json::json!([
        { "text": text, "evidence": text }
    ]))
}

// The cap gates BEFORE the call: once accumulated known spend has reached
// the cap, the next propose refuses with zero new calls. The crossing
// call itself is allowed — its cost only becomes known after the response.
#[test]
fn spend_cap_gates_the_next_call() {
    let script = Script::new(vec![
        Ok(openai_ok_with_cost(&one_claim("A is true."), 0.000_002)),
        Ok(openai_ok_with_cost(&one_claim("B is true."), 0.000_002)),
    ]);
    let extractor = llm(script.clone()).with_spend_cap_micros(1);
    // Episode 1: pre-call spend 0 < 1 micro — the call runs, its known
    // cost (2 micros) crosses the cap; output returns normally.
    let out = Extractor::propose(&extractor, &episode("ep-1", "A is true."))
        .expect("first call under cap succeeds");
    assert_eq!(out.len(), 1);
    // Episode 2: pre-call spend 2 >= 1 micro — refused, zero new calls.
    let err = Extractor::propose(&extractor, &episode("ep-2", "B is true."))
        .expect_err("cap gates the next call");
    match &err {
        ExtractionFailure::SpendCapExceeded {
            spent_usd,
            cap_usd,
            unaccounted_calls,
        } => {
            assert_eq!(*spent_usd, 0.000_002);
            assert_eq!(*cap_usd, 0.000_001);
            assert_eq!(*unaccounted_calls, 0);
        }
        other => panic!("spend-cap refusal expected, got {other:?}"),
    }
    assert_eq!(script.calls(), 1, "a cap hit makes zero new calls");
}

// A response without usage.cost cannot be accounted — the NEXT call is
// refused (fail-closed): a cap that cannot see spend must not silently
// keep spending (bajan-8cc; documented known limitation for providers
// that do not report per-call cost).
#[test]
fn unaccounted_cost_fails_closed() {
    let script = Script::new(vec![
        Ok(openai_ok(&one_claim("A is true."))),
        Ok(openai_ok(&one_claim("B is true."))),
    ]);
    let extractor = llm(script.clone()).with_spend_cap_micros(1_000_000);
    let out = Extractor::propose(&extractor, &episode("ep-1", "A is true."))
        .expect("call without usage succeeds");
    assert_eq!(out.len(), 1);
    let err = Extractor::propose(&extractor, &episode("ep-2", "B is true."))
        .expect_err("unaccounted spend fails closed");
    match &err {
        ExtractionFailure::SpendCapExceeded {
            spent_usd,
            unaccounted_calls,
            ..
        } => {
            assert_eq!(*spent_usd, 0.0);
            assert_eq!(*unaccounted_calls, 1);
        }
        other => panic!("spend-cap refusal expected, got {other:?}"),
    }
    assert_eq!(script.calls(), 1, "no further calls once unaccountable");
}

// Loop level: a cap hit stops the pass honestly — the crossing call's
// output IS staged (the money was spent), later episodes stay PENDING
// (never parked: a raised cap re-attempts them with the cache intact),
// and the report carries the machine-readable stop.
#[test]
fn spend_cap_stops_the_pass_with_later_episodes_pending() {
    let db = SqliteStore::open(":memory:").expect("store");
    for id in ["ep-1", "ep-2", "ep-3"] {
        ingest::persist(&db, &episode(id, "A is true.")).expect("persist");
    }
    let ok = openai_ok_with_cost(&one_claim("A is true."), 0.000_003);
    let script = Script::new(vec![Ok(ok.clone()), Ok(ok.clone()), Ok(ok)]);
    let extractor = llm(script.clone()).with_spend_cap_micros(5);
    let mut runs = bajan::extract::ExtractionRunStore::default();
    let report =
        run_extract_with(&db, &extractor, &mut runs).expect("cap stop is not a run failure");
    // ep-1 (spend 0<5) and ep-2 (spend 3<5) called; ep-3's pre-call gate
    // refused with zero new calls.
    assert_eq!(
        script.calls(),
        2,
        "crossing call allowed, gated call never made"
    );
    let staged = staged_texts(&db);
    assert_eq!(staged.len(), 2, "crossing call's output is staged");
    assert!(report.spend_cap.is_some(), "report carries the stop");
    let stop = report.spend_cap.expect("checked");
    assert_eq!(stop.spent_usd, 0.000_006);
    assert_eq!(stop.cap_usd, 0.000_005);
    assert_eq!(stop.unaccounted_calls, 0);
    assert_eq!(stop.episodes_left_pending, 1);
    assert_eq!(report.episodes_processed, 2, "honest processed count");
    assert!(report.parked.is_empty(), "a cap stop parks nothing");
    let finishes = finish_of(&runs);
    assert_eq!(finishes.len(), 2, "one run row per made call");
    assert!(
        finishes.iter().all(|f| matches!(f, Finish::Succeeded)),
        "both made calls succeeded"
    );
}

// --- failure telemetry classes (bajan-gso) ---------------------------------------

/// Budget exhaustion names the failure class — the last error's
/// machine-readable code decides (gso: timeout vs HTTP vs schema vs
/// transport, diagnosable without sqlite spelunking or curl).
#[test]
fn exhausted_failure_carries_class_from_transport_error() {
    let script = Script::new(vec![Err("http 503: overloaded".into()); 2]);
    let err = Extractor::propose(&llm_with(script, 2), &episode("ep-1", "A is true."))
        .expect_err("all attempts fail");
    assert!(matches!(
        err,
        bajan::extract::ExtractionFailure::Exhausted {
            attempts: 2,
            class: FailureClass::HttpStatus { code: 503 }
        }
    ));
}

/// Schema-decode failures (malformed provider output) classify as
/// Schema — an endpoint failing to honor the contract, not a network
/// problem.
#[test]
fn exhausted_after_schema_failures_classifies_schema() {
    let script = Script::new(vec![Ok("this is not the provider schema".into()); 2]);
    let err = Extractor::propose(&llm_with(script, 2), &episode("ep-1", "A is true."))
        .expect_err("all attempts fail");
    assert!(matches!(
        err,
        bajan::extract::ExtractionFailure::Exhausted {
            attempts: 2,
            class: FailureClass::Schema
        }
    ));
}

/// The machine-readable `timeout` prefix classifies as Timeout.
#[test]
fn exhausted_timeout_class_from_machine_readable_prefix() {
    let script = Script::new(vec![Err("timeout: request exceeded 120s".into())]);
    let err = Extractor::propose(&llm_with(script, 1), &episode("ep-1", "A is true."))
        .expect_err("attempt fails");
    assert!(matches!(
        err,
        bajan::extract::ExtractionFailure::Exhausted {
            attempts: 1,
            class: FailureClass::Timeout
        }
    ));
}

/// The REAL transport emits machine-readable codes: an HTTP 500 maps to
/// `http 500` so the class survives the String boundary.
#[test]
fn real_http_500_classifies_http_status() {
    let base_url = spawn_fake_openai(vec![(500, "{\"error\":\"overloaded\"}".to_string()); 3]);
    let extractor = LlmExtractor::new("test-model", &base_url, "TEST_API_KEY", 1024, 3)
        .with_sleep(Box::new(|_| {}))
        .with_key_lookup(Box::new(|_| Some("sk-test".into())));
    let err = Extractor::propose(&extractor, &episode("ep-1", "A is true."))
        .expect_err("all attempts 500");
    assert!(matches!(
        err,
        bajan::extract::ExtractionFailure::Exhausted {
            attempts: 3,
            class: FailureClass::HttpStatus { code: 500 }
        }
    ));
}

/// A server that accepts and never answers hits the global timeout; the
/// real transport maps that to the machine-readable `timeout` code
/// (burn-in F3: timeout vs HTTP error was invisible).
#[test]
fn real_timeout_classifies_timeout() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            std::thread::sleep(std::time::Duration::from_secs(3));
            drop(stream);
        }
    });
    let extractor = LlmExtractor::new(
        "test-model",
        format!("http://127.0.0.1:{port}/v1"),
        "TEST_API_KEY",
        1024,
        1,
    )
    .with_timeout_secs(1)
    .with_sleep(Box::new(|_| {}))
    .with_key_lookup(Box::new(|_| Some("sk-test".into())));
    let err =
        Extractor::propose(&extractor, &episode("ep-1", "A is true.")).expect_err("times out");
    assert!(matches!(
        err,
        bajan::extract::ExtractionFailure::Exhausted {
            attempts: 1,
            class: FailureClass::Timeout
        }
    ));
}
// --- per-call cost logging (bajan-3w4) -------------------------------------------
//
// The response `usage.cost` carries the exact per-call spend; the
// extractor logs one line per successful call so burn-ins measure spend
// directly instead of bounding it analytically. Default sink is stderr;
// tests inject a capturing logger.

/// Captured log lines + the matching injected logger.
type CapturedLog = (Arc<Mutex<Vec<String>>>, Box<dyn Fn(String) + Send + Sync>);

fn capturing_log() -> CapturedLog {
    let lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = lines.clone();
    let logger: Box<dyn Fn(String) + Send + Sync> =
        Box::new(move |line| sink.lock().expect("log lock").push(line));
    (lines, logger)
}

/// One line per successful call: episode, attempt, cost, running spend
/// against the cap (bajan-3w4).
#[test]
fn call_log_records_cost_per_call() {
    let script = Script::new(vec![
        Ok(openai_ok_with_cost(&one_claim("A is true."), 0.000_002)),
        Ok(openai_ok_with_cost(&one_claim("B is true."), 0.000_002)),
    ]);
    let (lines, logger) = capturing_log();
    let extractor = llm(script)
        .with_spend_cap_micros(500_000)
        .with_call_log(logger);
    Extractor::propose(&extractor, &episode("ep-1", "A is true.")).expect("ep-1");
    Extractor::propose(&extractor, &episode("ep-2", "B is true.")).expect("ep-2");
    let got = lines.lock().expect("log lock").clone();
    assert_eq!(got.len(), 2, "one line per successful call: {got:?}");
    assert!(got[0].contains("[llm] ep-1"), "{got:?}");
    assert!(got[0].contains("attempt 1"), "{got:?}");
    assert!(got[0].contains("cost $0.000002"), "{got:?}");
    assert!(got[0].contains("of $0.500000"), "{got:?}");
    assert!(
        got[1].contains("[llm] ep-2") && got[1].contains("spent $0.000004"),
        "running spend must accumulate: {got:?}"
    );
}

/// An unaccountable response (no usage.cost) is logged as such — and a
/// capped instance names the fail-closed consequence (bajan-8cc twin).
#[test]
fn call_log_notes_unaccountable_cost() {
    let script = Script::new(vec![Ok(openai_ok(&one_claim("A is true.")))]);
    let (lines, logger) = capturing_log();
    let extractor = llm(script)
        .with_spend_cap_micros(1_000_000)
        .with_call_log(logger);
    Extractor::propose(&extractor, &episode("ep-1", "A is true.")).expect("ok");
    let got = lines.lock().expect("log lock").clone();
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].contains("cost unaccounted"), "{got:?}");
    assert!(got[0].contains("fails closed"), "{got:?}");
}

/// Failed attempts carry no cost — only the successful attempt logs,
/// with its real attempt number; an uncapped instance says so.
#[test]
fn call_log_counts_retry_attempts_and_names_uncapped() {
    let script = Script::new(vec![
        Err("503 overloaded".into()),
        Ok(openai_ok_with_cost(&one_claim("A is true."), 0.000_003)),
    ]);
    let (lines, logger) = capturing_log();
    let extractor = llm_with(script, 2).with_call_log(logger);
    Extractor::propose(&extractor, &episode("ep-1", "A is true.")).expect("retry then ok");
    let got = lines.lock().expect("log lock").clone();
    assert_eq!(
        got.len(),
        1,
        "failed attempts carry no cost — only the success logs: {got:?}"
    );
    assert!(got[0].contains("attempt 2"), "{got:?}");
    assert!(got[0].contains("(no cap)"), "{got:?}");
}

/// The default sink is stderr: through the real binary + real HTTP fake
/// server, the cost line appears on stderr while stdout stays a clean
/// envelope.
#[test]
fn cli_llm_extract_logs_cost_to_stderr() {
    let dir =
        std::env::temp_dir().join(format!("bajan-3w4-cli-{}-{}", std::process::id(), line!()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let db_path = dir.join("t.db");
    let db_path = db_path.to_str().expect("utf8").to_string();

    let (code, out) = run_with_env_stdin(
        &[],
        &["--json", "--db", &db_path, "ingest"],
        Some(
            r#"[{"id":"e1","text":"A is true. B follows.","locator":{"kind":"span","value":"heading:H"},"source":{"source_type":"markdown","data_cutoff":null,"authority_tier":1,"tags":[]}}]"#,
        ),
    );
    assert_eq!(code, 0, "{out}");

    let base_url = spawn_fake_openai(vec![(
        200,
        openai_ok_with_cost(
            &claims_content(serde_json::json!([
                { "text": "A is true.", "evidence": "A is true." }
            ])),
            0.000_004,
        ),
    )]);
    let exe = env!("CARGO_BIN_EXE_bajan");
    let output = std::process::Command::new(exe)
        .args(["--json", "--db", &db_path, "extract"])
        .env("BAJAN_EXTRACTOR", "llm")
        .env("BAJAN_EXTRACTOR_MODEL", "fake-model")
        .env("BAJAN_EXTRACTOR_BASE_URL", &base_url)
        .env("BAJAN_EXTRACTOR_API_KEY_ENV", "FAKE_KEY")
        .env("BAJAN_MAX_SPEND_USD", "1.00")
        .env("FAKE_KEY", "sk-nothing")
        .output()
        .expect("run bajan");
    assert!(
        output.status.success(),
        "extract must succeed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("[llm] e1"), "stderr: {stderr}");
    assert!(stderr.contains("cost $0.000004"), "stderr: {stderr}");
    assert!(stderr.contains("of $1.000000"), "stderr: {stderr}");

    std::fs::remove_dir_all(&dir).ok();
}
