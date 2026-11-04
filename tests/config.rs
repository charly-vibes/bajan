//! Purpose: per-workspace config surface (bajan-bku) — a strict,
//! minimal `bajan.toml` in the working directory: staleness window,
//! default budgets, LLM retry budget. Resolution order is CLI flag >
//! file > built-in default; the missing file is byte-identical current
//! behavior; misconfiguration fails at startup with a machine-readable
//! envelope and exit 1. Config is never store state (dump-recreate
//! unaffected). No secret values: API keys stay env-var-NAME based.
//! Rationale: qt_ranking names a staleness window as if configurable
//! and budgets are hardcoded 256 / LLM_DEFAULT_MAX_ATTEMPTS — a
//! workspace needs to tune them without editing code.

use std::path::{Path, PathBuf};
use std::process::Output;

fn dir_with(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "bajan-bku-{}-{}-{}",
        std::process::id(),
        line!(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    for (name, body) in files {
        std::fs::write(dir.join(name), body).expect("write file");
    }
    dir
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_bajan"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run bajan")
}

fn stdout_json(out: &Output) -> serde_json::Value {
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("valid envelope")
}

// --- strict parsing (unit) ------------------------------------------------------

/// All three keys parse alongside comments and blank lines.
#[test]
fn parses_keys_with_comments() {
    let cfg = bajan::config::BajanConfig::parse(
        "# workspace tuning\nstaleness_window_days = 90\n\ndefault_budget = 64\nmax_attempts = 5\n",
    )
    .expect("parses");
    assert_eq!(cfg.staleness_window_days, Some(90));
    assert_eq!(cfg.default_budget, Some(64));
    assert_eq!(cfg.max_attempts, Some(5));
}

/// Unknown keys are refused — a typo must not silently disable tuning.
#[test]
fn unknown_key_refused() {
    let err = bajan::config::BajanConfig::parse("staleness_window = 90\n")
        .expect_err("unknown key refused");
    assert!(err.to_string().contains("staleness_window"), "{err}");
}

/// Table headers and anything that is not `key = <integer>` are refused.
#[test]
fn malformed_lines_refused() {
    assert!(bajan::config::BajanConfig::parse("[budgets]\n").is_err());
    assert!(bajan::config::BajanConfig::parse("default_budget\n").is_err());
    assert!(bajan::config::BajanConfig::parse("default_budget = \"64\"\n").is_err());
    assert!(bajan::config::BajanConfig::parse("default_budget = abc\n").is_err());
    assert!(bajan::config::BajanConfig::parse("staleness_window_days = -5\n").is_err());
}

/// Empty and comment-only bodies are valid all-defaults configs.
#[test]
fn empty_body_is_all_defaults() {
    let cfg = bajan::config::BajanConfig::parse("\n# nothing\n").expect("parses");
    assert_eq!(cfg, bajan::config::BajanConfig::default());
}

// --- binary: failure surface ------------------------------------------------------

/// A malformed config fails at startup: ok:false envelope, code
/// config_parse, exit 1 — never a silently-ignored file.
#[test]
fn malformed_config_is_envelope_exit_one() {
    let dir = dir_with(&[("bajan.toml", "wat_is_this = 1\n")]);
    let out = run_in(&dir, &["--json", "query", "x"]);
    assert_eq!(out.status.code(), Some(1), "must exit 1");
    let v = stdout_json(&out);
    assert_eq!(v["ok"].as_bool(), Some(false));
    assert_eq!(v["data"]["code"].as_str(), Some("config_parse"));
    assert!(
        v["data"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("wat_is_this"),
        "message names the offending key"
    );
}

// --- binary: budget precedence -----------------------------------------------------

fn seed_two_claims(dir: &Path) {
    let ingest = r#"[{"id":"e1","text":"alpha claim one.","locator":{"kind":"span","value":"h1"},"source":{"source_type":"markdown","data_cutoff":null,"authority_tier":1,"tags":[]}},{"id":"e2","text":"beta claim two.","locator":{"kind":"span","value":"h2"},"source":{"source_type":"markdown","data_cutoff":null,"authority_tier":1,"tags":[]}}]"#;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_bajan"))
        .args(["--db", "t.db", "ingest"])
        .current_dir(dir)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ingest");
    use std::io::Write;
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(ingest.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("ingest");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );

    let out = run_in(dir, &["--db", "t.db", "extract"]);
    assert!(
        out.status.success(),
        "extract: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = run_in(dir, &["--db", "t.db", "adopt", "0", "1"]);
    assert!(
        out.status.success(),
        "adopt: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// File overrides built-in default: default_budget = 1 with two live
/// claims truncates the walk (budget-exhausted, one hit at most).
#[test]
fn file_budget_overrides_builtin_default() {
    let dir = dir_with(&[("bajan.toml", "default_budget = 1\n")]);
    seed_two_claims(&dir);
    let out = run_in(&dir, &["--json", "--db", "t.db", "query", "claim"]);
    let v = stdout_json(&out);
    assert_eq!(v["ok"].as_bool(), Some(true));
    assert_eq!(
        v["data"]["status"].as_str(),
        Some("budget-exhausted"),
        "file budget 1 truncates: {}",
        v
    );
    assert_eq!(v["data"]["hits"].as_array().map(Vec::len), Some(1));
}

/// CLI flag overrides the file: --budget 10 completes the same walk.
#[test]
fn cli_flag_overrides_file_budget() {
    let dir = dir_with(&[("bajan.toml", "default_budget = 1\n")]);
    seed_two_claims(&dir);
    let out = run_in(
        &dir,
        &["--json", "--db", "t.db", "query", "--budget", "10", "claim"],
    );
    let v = stdout_json(&out);
    assert_eq!(
        v["data"]["status"].as_str(),
        Some("complete"),
        "flag beats file: {}",
        v
    );
    assert_eq!(v["data"]["hits"].as_array().map(Vec::len), Some(2));
}

/// Missing file is byte-identical current behavior: complete at the
/// built-in 256 default.
#[test]
fn missing_file_is_builtin_default() {
    let dir = dir_with(&[]);
    seed_two_claims(&dir);
    let out = run_in(&dir, &["--json", "--db", "t.db", "query", "claim"]);
    let v = stdout_json(&out);
    assert_eq!(v["data"]["status"].as_str(), Some("complete"));
    assert_eq!(v["data"]["hits"].as_array().map(Vec::len), Some(2));
    assert!(
        v["data"]["staleness_warning"].is_null(),
        "no staleness without file: {v}"
    );
}

// --- binary: staleness window -------------------------------------------------------

/// The file's staleness window governs the demotion: a cutoff ~400 days
/// old is stale under the built-in 365-day default but fresh under a
/// configured 800-day window (qt_ranking staleness flag flips).
#[test]
fn file_staleness_window_governs_demotion() {
    let cutoff = bajan::query::iso_from_days(bajan::query::today_days_utc() - 400);
    let ingest = format!(
        r#"[{{"id":"e1","text":"alpha claim one.","locator":{{"kind":"span","value":"h1"}},"source":{{"source_type":"markdown","data_cutoff":"{cutoff}","authority_tier":2,"tags":[]}}}}]"#
    );
    let dir = dir_with(&[]);
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_bajan"))
        .args(["--db", "t.db", "ingest"])
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ingest");
    use std::io::Write;
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(ingest.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("ingest");
    assert!(out.status.success());
    let out = run_in(&dir, &["--db", "t.db", "extract"]);
    assert!(out.status.success());
    let out = run_in(&dir, &["--db", "t.db", "adopt", "0"]);
    assert!(out.status.success());

    // Default 365-day window: stale (warning present).
    let out = run_in(&dir, &["--json", "--db", "t.db", "query", "claim"]);
    let v = stdout_json(&out);
    assert!(
        !v["data"]["staleness_warning"].is_null(),
        "400d cutoff is stale under the 365d default: {v}"
    );

    // File raises the window: fresh (warning gone).
    std::fs::write(dir.join("bajan.toml"), "staleness_window_days = 800\n").expect("write cfg");
    let out = run_in(&dir, &["--json", "--db", "t.db", "query", "claim"]);
    let v = stdout_json(&out);
    assert!(
        v["data"]["staleness_warning"].is_null(),
        "800d window keeps the claim fresh: {v}"
    );
}

// --- binary: LLM retry budget --------------------------------------------------------

/// Minimal fake OpenAI server (one scripted response per connection).
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

fn ok_body() -> String {
    serde_json::json!({
        "choices": [{ "message": { "role": "assistant",
            "content": "{\"claims\":[{\"text\":\"alpha claim one.\",\"evidence\":\"alpha claim one.\"}]}" } }],
        "usage": { "cost": 0.000001 }
    })
    .to_string()
}

/// The file's max_attempts gates the LLM retry budget: 2 attempts
/// against two failures park the episode; a 3-attempt config succeeds —
/// no code change, only the config line.
#[test]
fn file_max_attempts_gates_llm_retries() {
    let dir = dir_with(&[("bajan.toml", "max_attempts = 2\n")]);
    seed_two_claims(&dir);

    let run_llm = |dir: &Path, base_url: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_bajan"))
            .args(["--json", "--db", "t.db", "extract"])
            .current_dir(dir)
            .env("BAJAN_EXTRACTOR", "llm")
            .env("BAJAN_EXTRACTOR_MODEL", "fake-model")
            .env("BAJAN_EXTRACTOR_BASE_URL", base_url)
            .env("BAJAN_EXTRACTOR_API_KEY_ENV", "FAKE_KEY")
            .env("BAJAN_MAX_SPEND_USD", "1.00")
            .env("FAKE_KEY", "sk-nothing")
            .output()
            .expect("run bajan extract")
    };

    // Pass 1: budget 2 — both attempts 500 → episode parked, nothing
    // proposed (the third scripted success is never reached).
    let base_url = spawn_fake_openai(vec![
        (500, "{\"error\":\"overloaded\"}".to_string()),
        (500, "{\"error\":\"overloaded\"}".to_string()),
        (200, ok_body()),
    ]);
    let out = run_llm(&dir, &base_url);
    let v = stdout_json(&out);
    assert_eq!(
        v["ok"].as_bool(),
        Some(true),
        "park is not a run failure: {v}"
    );
    assert_eq!(v["data"]["parked"].as_array().map(Vec::len), Some(1), "{v}");

    // Pass 2: budget 3 — the fresh pass re-attempts (no cache entry was
    // written) and the third attempt succeeds.
    std::fs::write(dir.join("bajan.toml"), "max_attempts = 3\n").expect("write cfg");
    let base_url = spawn_fake_openai(vec![
        (500, "{\"error\":\"overloaded\"}".to_string()),
        (500, "{\"error\":\"overloaded\"}".to_string()),
        (200, ok_body()),
    ]);
    let out = run_llm(&dir, &base_url);
    let v = stdout_json(&out);
    assert_eq!(v["data"]["candidates_proposed"].as_i64(), Some(1), "{v}");

    // A stale env-var name in the file must not exist — API keys stay
    // env-var-NAME based (anti-goal): the parser refuses unknown keys so
    // no secret-bearing key can ever be added.
    assert!(
        bajan::config::BajanConfig::parse("api_key = \"sk-secret\"\n").is_err(),
        "secret-bearing keys are refused by the strict key set"
    );
}
