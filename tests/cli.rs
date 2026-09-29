//! Purpose: binary-level contract tests — exit status and output format of
//! the real `bajan` executable.
//! Responsibilities: assert the envelope contract end-to-end (exit non-zero
//! on ok:false per bajan-aan; envelope on argument errors per bajan-ts6).
//! Rationale: `src/cli.rs` unit tests cover envelope construction, but the
//! bugs these guard against live exactly at the process boundary (`main`),
//! so they need a shell-observable regression test.

use std::process::Command;

fn bajan(args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_bajan"))
        .args(args)
        .output()
        .expect("binary built by cargo test");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

// bajan-aan: a stub error path must exit non-zero — a machine consumer
// gating on exit status must never see success for an ok:false envelope.
#[test]
fn stub_error_path_exits_non_zero() {
    let (code, stdout) = bajan(&["ingest", "--json"]);
    assert_ne!(code, 0, "ingest stub must exit non-zero");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope on stdout");
    assert_eq!(v["ok"].as_bool(), Some(false));
}

// bajan-ts6: a bad subcommand must still emit the suite envelope — no
// plain-text clap usage error bypassing the single-output-format invariant.
#[test]
fn argument_error_emits_envelope() {
    let (code, stdout) = bajan(&["nosuchcmd"]);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("argument errors still emit a parseable envelope");
    assert_eq!(v["envelope_version"].as_str(), Some("0.1"));
    assert_eq!(v["ok"].as_bool(), Some(false));
    assert_eq!(v["envelope_kind"].as_str(), Some("error"));
    assert_eq!(v["data"]["code"].as_str(), Some("argument_error"));
    let remediation = v["data"]["remediation"].as_array().expect("remediation");
    assert!(!remediation.is_empty(), "Invariant 3.2.5");
    assert_ne!(code, 0, "argument errors exit non-zero");
}

// bajan-3w1 (Rule-of-5 CORR-001): bare `bajan` must state the mistake —
// clap's about line ("Spec-driven knowledge-graph pipeline CLI") is not an
// error message a consumer can act on.
#[test]
fn no_args_error_names_the_missing_subcommand() {
    let (code, stdout) = bajan(&[]);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope");
    assert_eq!(v["ok"].as_bool(), Some(false));
    let msg = v["data"]["message"].as_str().expect("message");
    assert!(
        msg.contains("subcommand"),
        "message must name the missing subcommand, got: {msg}"
    );
    assert_ne!(code, 0);
}

// bajan-3w1 (Rule-of-5 CORR-002): the missing argument itself (`<CLAIMS>...`)
// must survive in the message — not be truncated away with the header line.
#[test]
fn missing_argument_error_names_the_argument() {
    let (_, stdout) = bajan(&["adopt", "--actor", "x"]);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope");
    let msg = v["data"]["message"].as_str().expect("message");
    assert!(
        msg.contains("CLAIMS"),
        "message must name the missing argument, got: {msg}"
    );
}

#[test]
fn version_ok_path_exits_zero() {
    let (code, stdout) = bajan(&["version", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope on stdout");
    assert_eq!(v["ok"].as_bool(), Some(true));
}

// bajan-2sj (Rule-of-5 CORR-003): a deliberately corrupted store file must
// emit the suite error envelope on stdout with exit 1 — never a panic with
// no envelope (single-output-format contract, bajan-aan/bajan-ts6).
#[test]
fn corrupt_store_emits_error_envelope_and_exits_one() {
    let dir = std::env::temp_dir().join(format!("bajan-2sj-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let db_path = dir.join("corrupt.db");
    let db_str = db_path.to_str().expect("utf8 path").to_string();

    // First run creates the store file with its schema (no episodes yet).
    let (code, _) = bajan(&["extract", "--db", &db_str, "--json"]);
    assert_eq!(code, 0);
    assert!(db_path.exists(), "extract created the store file");

    // Seed one episode row with valid tags, then corrupt the tags column
    // with raw SQL — the store API never writes non-JSON tags, so this
    // simulates external corruption.
    let conn = rusqlite::Connection::open(&db_path).expect("reopen raw");
    conn.execute(
        "INSERT INTO episodes
         (id, text, locator_kind, locator, source_type, data_cutoff, authority_tier, tags)
         VALUES ('ep-1', 'alpha beta', 'absent', NULL, 'note', NULL, 1, '[\"t\"]')",
        [],
    )
    .expect("seed episode row");
    conn.execute("UPDATE episodes SET tags = 'not json'", [])
        .expect("corrupt tags column");

    let (code, stdout) = bajan(&["extract", "--db", &db_str, "--json"]);
    assert_eq!(code, 1, "corrupt-store read exits 1");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope on stdout");
    assert_eq!(v["ok"].as_bool(), Some(false));

    let _ = std::fs::remove_dir_all(&dir);
}
