//! Purpose: tests for the extractor selection surface (bajan-9av) —
//! `ExtractorConfig` parsing from an injected environment, validation of
//! the llm parameter surface, honest not-implemented gate for the LLM
//! extractor (lands with bajan-vg6), and the CLI envelope on
//! misconfiguration.
//! Rationale: config errors must fail at selection with machine-readable
//! reasons, never mid-run; the API key is referenced by env-var NAME and
//! never appears in config or envelopes.

use bajan::extract::ExtractorConfigError;

fn lookup_from<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |key: &str| {
        pairs
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.to_string())
    }
}

/// Default selection (no env at all): the deterministic sentence atomizer
/// (bajan-5zy upgraded the default no-LLM proposer from whole-episode to
/// atomic claims; `legacy` remains selectable behind config).
#[test]
fn default_is_atomic() {
    let config = bajan::extract::ExtractorConfig::from_env_with(&|_| None);
    assert_eq!(config.kind, "atomic");
    // Selects fine — the deterministic atomizer.
    config.select().expect("atomic selects");
}

/// `legacy` remains selectable behind config — the byte-stable
/// whole-episode proposer for existing stores (bajan-5zy).
#[test]
fn legacy_kind_still_selects() {
    let config = bajan::extract::ExtractorConfig::from_env_with(&|key| {
        (key == "BAJAN_EXTRACTOR").then(|| "legacy".to_string())
    });
    assert_eq!(config.kind, "legacy");
    config.select().expect("legacy selects");
}

/// Unknown kinds are refused with a machine-readable error at selection.
#[test]
fn unknown_kind_refused_at_selection() {
    let config = bajan::extract::ExtractorConfig::from_env_with(&lookup_from(&[(
        "BAJAN_EXTRACTOR",
        "quantum",
    )]));
    let err = config.select().expect_err("unknown kind refused");
    assert!(matches!(err, ExtractorConfigError::UnknownKind { ref kind } if kind == "quantum"));
    assert!(err.to_string().contains("unknown extractor kind"));
}

/// kind = llm with a complete parameter surface selects the real LLM
/// extractor (bajan-vg6 implemented it; the honest NotImplemented gate
/// is gone).
#[test]
fn llm_with_full_config_selects_extractor() {
    let config = bajan::extract::ExtractorConfig::from_env_with(&lookup_from(&[
        ("BAJAN_EXTRACTOR", "llm"),
        ("BAJAN_EXTRACTOR_MODEL", "gpt-4o-mini"),
        ("BAJAN_EXTRACTOR_BASE_URL", "https://api.example.com/v1"),
        ("BAJAN_EXTRACTOR_API_KEY_ENV", "MY_API_KEY"),
    ]));
    let extractor = config.select().expect("llm selects the real extractor");
    assert_eq!(extractor.model_id(), Some("gpt-4o-mini"));
    assert_eq!(
        extractor.version(),
        format!("llm-gpt-4o-mini-{}", env!("CARGO_PKG_VERSION"))
    );
}

/// Each missing llm parameter is named individually — misconfiguration is
/// diagnosed in one round trip, not guesswork.
#[test]
fn llm_missing_params_named_individually() {
    let make = |extra: &[(&str, &str)]| {
        let mut pairs = vec![("BAJAN_EXTRACTOR", "llm")];
        pairs.extend_from_slice(extra);
        bajan::extract::ExtractorConfig::from_env_with(&lookup_from(&pairs))
    };
    let err = make(&[]).select().unwrap_err();
    assert!(matches!(
        err,
        ExtractorConfigError::MissingParam {
            param: "BAJAN_EXTRACTOR_MODEL"
        }
    ));
    let err = make(&[("BAJAN_EXTRACTOR_MODEL", "m")])
        .select()
        .unwrap_err();
    assert!(matches!(
        err,
        ExtractorConfigError::MissingParam {
            param: "BAJAN_EXTRACTOR_BASE_URL"
        }
    ));
    let err = make(&[
        ("BAJAN_EXTRACTOR_MODEL", "m"),
        ("BAJAN_EXTRACTOR_BASE_URL", "https://x"),
    ])
    .select()
    .unwrap_err();
    assert!(matches!(
        err,
        ExtractorConfigError::MissingParam {
            param: "BAJAN_EXTRACTOR_API_KEY_ENV"
        }
    ));
}

/// The config never stores the API key itself — only the env-var NAME
/// (secrets discipline: config and envelopes are log-safe).
#[test]
fn config_carries_env_var_name_never_the_key() {
    let config = bajan::extract::ExtractorConfig::from_env_with(&lookup_from(&[
        ("BAJAN_EXTRACTOR", "llm"),
        ("BAJAN_EXTRACTOR_MODEL", "m"),
        ("BAJAN_EXTRACTOR_BASE_URL", "https://x"),
        ("BAJAN_EXTRACTOR_API_KEY_ENV", "MY_SECRET_ENV"),
    ]));
    assert_eq!(config.api_key_env.as_deref(), Some("MY_SECRET_ENV"));
    // The struct has no field that could hold the key value: only names.
    let debug = format!("{config:?}");
    assert!(
        !debug.contains("sk-"),
        "config debug output must be key-free"
    );
}

/// CLI: an unknown extractor kind produces a spec error envelope (exit 1)
/// naming the problem, not a panic or a silent legacy fallback.
#[test]
fn cli_unknown_kind_envelope_and_exit_code() {
    let (code, out) = run_with_env(
        &[("BAJAN_EXTRACTOR", "nonsense")],
        &["--json", "--db", ":memory:", "extract"],
    );
    assert_eq!(code, 1, "misconfiguration is an error: {out}");
    assert!(out.contains("extractor_config"), "{out}");
    assert!(out.contains("unknown extractor kind"), "{out}");
}

/// CLI: default (no env) extraction still works end to end — the seam
/// changed nothing about the happy path.
#[test]
fn cli_default_extraction_still_works() {
    // Seed via ingest, then extract with no BAJAN_EXTRACTOR set.
    let dir =
        std::env::temp_dir().join(format!("bajan-9av-cli-{}-{}", std::process::id(), line!()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let db_path = dir.join("t.db");
    let db_path = db_path.to_str().expect("path").to_string();
    let (code, out) = run_with_env_stdin(
        &[],
        &["--json", "--db", &db_path, "ingest"],
        Some(
            r#"[{"id":"e1","text":"Hello world.","locator":{"kind":"absent"},"source":{"source_type":"markdown","data_cutoff":null,"authority_tier":1,"tags":[]}}]"#,
        ),
    );
    assert_eq!(code, 0, "{out}");
    let (code, out) = run_with_env(&[], &["--json", "--db", &db_path, "extract"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("candidates_proposed"), "{out}");
    let _: serde_json::Value = serde_json::from_str(&out).expect("valid envelope");
    std::fs::remove_dir_all(&dir).ok();
}

// --- binary harness ------------------------------------------------------

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
