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

/// kind = llm with a complete parameter surface (now including the
/// mandatory spend cap, bajan-8cc) selects the real LLM extractor.
#[test]
fn llm_with_full_config_selects_extractor() {
    let config = bajan::extract::ExtractorConfig::from_env_with(&lookup_from(&[
        ("BAJAN_EXTRACTOR", "llm"),
        ("BAJAN_EXTRACTOR_MODEL", "gpt-4o-mini"),
        ("BAJAN_EXTRACTOR_BASE_URL", "https://api.example.com/v1"),
        ("BAJAN_EXTRACTOR_API_KEY_ENV", "MY_API_KEY"),
        ("BAJAN_MAX_SPEND_USD", "0.50"),
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

// --- spend cap surface (bajan-8cc) -------------------------------------------

// The spend cap is REQUIRED for the llm extractor — running a real
// money-spending extractor with no cost ceiling is the invented-estimate
// failure mode with a wallet attached. Without BAJAN_MAX_SPEND_USD the
// selection refuses, naming the variable.
#[test]
fn llm_without_spend_cap_is_refused() {
    let config = bajan::extract::ExtractorConfig::from_env_with(&lookup_from(&[
        ("BAJAN_EXTRACTOR", "llm"),
        ("BAJAN_EXTRACTOR_MODEL", "test-model"),
        ("BAJAN_EXTRACTOR_BASE_URL", "http://localhost/v1"),
        ("BAJAN_EXTRACTOR_API_KEY_ENV", "TEST_API_KEY"),
    ]));
    let err = config.select().expect_err("cap is required for llm");
    assert!(
        matches!(err, ExtractorConfigError::MissingParam { param } if param == "BAJAN_MAX_SPEND_USD"),
        "got: {err}"
    );
    assert!(err.to_string().contains("BAJAN_MAX_SPEND_USD"));
}

// The cap value must parse as a positive finite USD amount — junk,
// negatives, zero, and infinities are config errors, not silent no-ops.
#[test]
fn spend_cap_must_be_positive_finite_number() {
    for value in ["abc", "-1", "0", "inf", "nan"] {
        let config = bajan::extract::ExtractorConfig::from_env_with(&lookup_from(&[
            ("BAJAN_EXTRACTOR", "llm"),
            ("BAJAN_EXTRACTOR_MODEL", "test-model"),
            ("BAJAN_EXTRACTOR_BASE_URL", "http://localhost/v1"),
            ("BAJAN_EXTRACTOR_API_KEY_ENV", "TEST_API_KEY"),
            ("BAJAN_MAX_SPEND_USD", value),
        ]));
        let err = config
            .select()
            .expect_err(&format!("cap '{value}' must be refused"));
        assert!(
            matches!(err, ExtractorConfigError::BadSpendCap { .. }),
            "value '{value}': got {err}"
        );
    }
}

// CLI: selecting llm without the cap produces the extractor_config error
// envelope (exit 1), naming the missing variable.
#[test]
fn cli_llm_without_cap_envelope_and_exit_code() {
    let (code, out) = run_with_env(
        &[
            ("BAJAN_EXTRACTOR", "llm"),
            ("BAJAN_EXTRACTOR_MODEL", "test-model"),
            ("BAJAN_EXTRACTOR_BASE_URL", "http://localhost:1/v1"),
            ("BAJAN_EXTRACTOR_API_KEY_ENV", "BAJAN_NO_SUCH_KEY"),
        ],
        &["--json", "extract"],
    );
    assert_ne!(code, 0, "missing cap must exit non-zero");
    let v: serde_json::Value = serde_json::from_str(&out).expect("envelope on stdout");
    assert_eq!(v["ok"].as_bool(), Some(false));
    assert_eq!(v["data"]["code"].as_str(), Some("extractor_config"));
    let message = v["data"]["message"].as_str().expect("message");
    assert!(
        message.contains("BAJAN_MAX_SPEND_USD"),
        "message must name the cap variable, got: {message}"
    );
}

// ------------------------------------------------------------------
// bajan-noj: BAJAN_EXTRACTOR_TIMEOUT_SECS — the transport timeout is
// configurable via env; the 120s default survives when unset.

fn llm_env(extra: &[(&str, &str)]) -> bajan::extract::ExtractorConfig {
    let mut pairs = vec![
        ("BAJAN_EXTRACTOR", "llm"),
        ("BAJAN_EXTRACTOR_MODEL", "test-model"),
        ("BAJAN_EXTRACTOR_BASE_URL", "https://api.example.com/v1"),
        ("BAJAN_EXTRACTOR_API_KEY_ENV", "MY_API_KEY"),
        ("BAJAN_MAX_SPEND_USD", "0.50"),
    ];
    pairs.extend_from_slice(extra);
    bajan::extract::ExtractorConfig::from_env_with(&lookup_from(&pairs))
}

/// The ticket's test shape: select with the env var set yields a
/// transport carrying exactly that timeout.
#[test]
fn timeout_env_yields_transport_with_that_timeout() {
    let config = llm_env(&[("BAJAN_EXTRACTOR_TIMEOUT_SECS", "300")]);
    let extractor = config.select().expect("llm selects");
    assert_eq!(
        extractor.transport_timeout(),
        Some(std::time::Duration::from_secs(300)),
        "env-set timeout must reach the transport"
    );
}

/// Unset: the burn-in-verified 120s default is preserved byte-for-byte
/// behavior (missing file/env = current behavior).
#[test]
fn timeout_default_is_120s_when_unset() {
    let extractor = llm_env(&[]).select().expect("llm selects");
    assert_eq!(
        extractor.transport_timeout(),
        Some(std::time::Duration::from_secs(120)),
    );
}

/// Zero is refused at selection — a transport that can never complete
/// a call is misconfiguration, not tuning.
#[test]
fn timeout_zero_refused_at_selection() {
    let err = llm_env(&[("BAJAN_EXTRACTOR_TIMEOUT_SECS", "0")])
        .select()
        .expect_err("zero timeout refused");
    assert!(matches!(err, ExtractorConfigError::BadTimeout { .. }));
    assert!(err.to_string().contains("BAJAN_EXTRACTOR_TIMEOUT_SECS"));
}

/// Non-numeric and negative values are refused at selection with the
/// variable named.
#[test]
fn timeout_non_numeric_refused_at_selection() {
    for value in ["abc", "-5", "12.5", ""] {
        let err = llm_env(&[("BAJAN_EXTRACTOR_TIMEOUT_SECS", value)])
            .select()
            .expect_err("non-integer timeout refused");
        assert!(
            matches!(err, ExtractorConfigError::BadTimeout { .. }),
            "value {value:?} must be BadTimeout"
        );
        assert!(err.to_string().contains(value), "must echo the bad value");
    }
}

/// Reasoning-model headroom: large-but-sane timeouts select fine
/// (burn-in saw chapter p50 ~50-90s under load; the ceiling is the
/// operator's call, not bajan's).
#[test]
fn timeout_large_but_valid_selects() {
    let extractor = llm_env(&[("BAJAN_EXTRACTOR_TIMEOUT_SECS", "3600")])
        .select()
        .expect("3600s is a valid operator choice");
    assert_eq!(
        extractor.transport_timeout(),
        Some(std::time::Duration::from_secs(3600)),
    );
}

/// The deterministic extractors have no transport — they report None
/// (the provided-trait default) regardless of the env var.
#[test]
fn timeout_env_ignored_by_atomic() {
    let config = bajan::extract::ExtractorConfig::from_env_with(&lookup_from(&[
        ("BAJAN_EXTRACTOR", "atomic"),
        ("BAJAN_EXTRACTOR_TIMEOUT_SECS", "300"),
    ]));
    let extractor = config.select().expect("atomic selects");
    assert_eq!(extractor.transport_timeout(), None);
}
