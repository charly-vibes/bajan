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

#[test]
fn version_ok_path_exits_zero() {
    let (code, stdout) = bajan(&["version", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope on stdout");
    assert_eq!(v["ok"].as_bool(), Some(true));
}
