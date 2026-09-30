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

use bajan::extract::{
    ExtractionFailure, Extractor, ExtractorConfig, Finish, Reason, run_extract_with,
};
use bajan::ingest::{self, EpisodeRecord, Locator, SourceMeta};
use bajan::llm::{LlmExtractor, LlmTransport, chunk_episode, llm_backoff, strip_json_fence};
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
    assert!(matches!(err, ExtractionFailure::Exhausted { attempts: 1 }));
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
    assert!(matches!(err, ExtractionFailure::Exhausted { attempts: 1 }));
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
        Reason::RepeatedCallFailure { attempts: 3 }
    );
    assert_eq!(
        finish_of(&runs),
        vec![&Finish::Parked {
            reason: Reason::RepeatedCallFailure { attempts: 3 }
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
    assert!(matches!(err, ExtractionFailure::Exhausted { attempts: 3 }));
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

/// Backoff is exponential from 500ms, capped at 8s.
#[test]
fn backoff_is_exponential_and_capped() {
    assert_eq!(llm_backoff(1), std::time::Duration::from_millis(500));
    assert_eq!(llm_backoff(2), std::time::Duration::from_millis(1000));
    assert_eq!(llm_backoff(3), std::time::Duration::from_millis(2000));
    assert_eq!(
        llm_backoff(20),
        std::time::Duration::from_millis(8_000),
        "capped"
    );
}

// --- config surface --------------------------------------------------------------

/// kind = llm with a complete parameter surface now selects the real LLM
/// extractor (bajan-vg6 replaced the honest NotImplemented gate).
#[test]
fn llm_full_config_selects_extractor() {
    let config = ExtractorConfig {
        kind: "llm".into(),
        model_id: Some("gpt-4o-mini".into()),
        base_url: Some("https://api.example.com/v1".into()),
        api_key_env: Some("MY_API_KEY".into()),
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
