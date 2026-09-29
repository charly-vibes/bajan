//! Purpose: clap CLI definition plus the single `run` dispatch that emits a
//! genesis suite JSON envelope for every bajan command.
//! Responsibilities: parse arguments (global `--json`), route subcommands to
//! module handlers, map module errors to envelope errors, render text mode.
//! Rationale: the envelope is the *single* output format for all CLI commands
//! (callers check `ok` first); Invariant 3.2.5 forbids emitting an error
//! without non-empty remediation, so every error path carries a fix hint.

use clap::{Parser, Subcommand};
use genesis::envelope::{Envelope, EnvelopeKind, ErrorResult, RemediationEntry};
use serde::Serialize;

use crate::extract;
use crate::ingest;
use crate::query;
use crate::resolve;
use crate::store::{ClaimStore, StoreError};

/// Top-level CLI: `bajan [--json] <subcommand>`.
#[derive(Debug, Parser)]
#[command(
    name = "bajan",
    version,
    about = "Spec-driven knowledge-graph pipeline CLI"
)]
pub struct Cli {
    /// Emit the machine-readable suite envelope instead of a text rendering.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print tool identity as a suite envelope.
    Version,
    /// Persist a normalized episode stream verbatim (stub).
    Ingest,
    /// Propose candidate claims from persisted episodes (stub).
    Extract,
    /// Resolve claim identity across versions (stub).
    Resolve,
    /// Read-side queries over the claim graph (stub).
    Query,
    /// Human accept action: move staged claims to active (one audit record
    /// per claim).
    Adopt {
        /// Claim keys to accept (batch permitted, per-claim audited; at
        /// least one — a no-op accept must not report success).
        #[arg(num_args = 1.., required = true)]
        claims: Vec<usize>,
        /// Operator identity; defaults to $USER.
        #[arg(long)]
        actor: Option<String>,
    },
}

/// Error type shared by all bajan pipeline modules.
///
/// Every variant carries the governing spec so the CLI can stamp `spec_ref`
/// on the emitted error envelope — errors are always traceable to the
/// invariant they guard.
#[derive(Debug, thiserror::Error)]
pub enum BajanError {
    #[error("{module} is not implemented in this scaffold (see {spec})")]
    NotImplemented {
        module: &'static str,
        spec: &'static str,
    },
}

/// Version metadata carried in the `version` envelope's `data`.
#[derive(Debug, Serialize)]
struct VersionData {
    name: &'static str,
    version: &'static str,
    status: &'static str,
    specs: Vec<&'static str>,
}

fn version_envelope() -> Envelope<VersionData> {
    Envelope::success(
        env!("CARGO_PKG_VERSION"),
        EnvelopeKind::Version,
        VersionData {
            name: "bajan",
            version: env!("CARGO_PKG_VERSION"),
            status: "scaffold",
            specs: vec![
                "specs/ingestion-contract.md",
                "specs/extraction-claims.md",
                "specs/graph-model.md",
            ],
        },
        vec![],
        vec![],
    )
}

/// Spec-traced error envelope: one construction point for every error
/// path, so Invariant 3.2.5 (non-empty remediation) holds by construction
/// — the `ErrorResult::new` constructor itself rejects an empty list.
fn spec_error_envelope(
    code: &str,
    message: &str,
    spec: &str,
    module: &str,
    remediation: Vec<RemediationEntry>,
) -> serde_json::Value {
    let error = ErrorResult::new(code, message, None, Some(spec), Some(module), vec![], remediation)
        .expect("remediation is non-empty by construction (Invariant 3.2.5)");
    serde_json::to_value(Envelope::error(env!("CARGO_PKG_VERSION"), error, vec![]))
        .expect("envelope serialization cannot fail")
}

/// Convert a stub module result into an error envelope.
fn stub_envelope(result: &Result<(), BajanError>) -> serde_json::Value {
    let err = match result {
        Ok(()) => unreachable!("scaffold stubs always fail; wire success only when implemented"),
        Err(e) => e,
    };
    let (module, spec) = match err {
        BajanError::NotImplemented { module, spec } => (*module, *spec),
    };
    spec_error_envelope(
        "not_implemented",
        &format!("{module} is not implemented in this scaffold"),
        spec,
        module,
        vec![RemediationEntry {
            command: format!("cat {spec}"),
            description: format!(
                "Read the governing spec for {module}; implementation arrives via the \
                 gated beads tickets (`bd ready`)."
            ),
        }],
    )
}

/// Envelope for the human adopt path (`gm_human_adopt`).
///
/// Success data carries the per-claim audit records; refusal, unknown
/// claim keys, or an empty session store emit a spec-traced error envelope
/// citing specs/graph-model.md.
fn adopt_envelope(store: &mut ClaimStore, claims: &[usize], actor: Option<&str>) -> serde_json::Value {
    let actor = actor
        .map(str::to_string)
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "unknown".to_string());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();

    match store.adopt_batch(claims, &actor, now) {
        Ok(audits) => serde_json::to_value(Envelope::success(
            env!("CARGO_PKG_VERSION"),
            EnvelopeKind::Ok,
            audits,
            vec![],
            vec![],
        ))
        .expect("envelope serialization cannot fail"),
        Err(err) => adopt_error_envelope(&err),
    }
}

/// Error envelope for a refused adoption — spec-traced via the shared
/// construction point.
fn adopt_error_envelope(err: &StoreError) -> serde_json::Value {
    spec_error_envelope(
        "adopt_refused",
        &err.to_string(),
        "specs/graph-model.md",
        "adopt",
        vec![RemediationEntry {
            command: "bajan adopt <claims>... --actor <id>".into(),
            description:
                "Adopt accepts staged (proposed) claims only; active and rejected claims are \
                 never touched, and no automated path can set active (gm_human_adopt)."
                    .into(),
        }],
    )
}

/// Run a command and return its suite envelope as JSON.
///
/// This is the single serialization point: every command's output passes
/// through here, so the envelope contract cannot be bypassed.
pub fn run(command: Command) -> String {
    let value = match &command {
        Command::Version => serde_json::to_value(version_envelope())
            .expect("envelope serialization cannot fail"),
        Command::Ingest => stub_envelope(&ingest::run()),
        Command::Extract => stub_envelope(&extract::run()),
        Command::Resolve => stub_envelope(&resolve::run()),
        Command::Query => stub_envelope(&query::run()),
        Command::Adopt { claims, actor } => {
            // Session-scoped store: no persistence engine yet (vertical-slice
            // ticket wires SQLite), so the CLI adopt path is honest about an
            // empty store — refusal, never fake success.
            let mut store = ClaimStore::default();
            adopt_envelope(&mut store, claims, actor.as_deref())
        }
    };
    serde_json::to_string(&value).expect("envelope serialization cannot fail")
}

/// Render an envelope JSON string as a short human-readable line.
///
/// Text mode is a convenience rendering of the same envelope — never a
/// second output format with different content.
pub fn render_text(json: &str) -> String {
    let v: serde_json::Value =
        serde_json::from_str(json).expect("run() always emits valid JSON");
    if v["ok"].as_bool().unwrap_or(false) {
        format!(
            "{} v{} ({})",
            v["data"]["name"].as_str().unwrap_or("bajan"),
            v["cli_version"].as_str().unwrap_or("?"),
            v["data"]["status"].as_str().unwrap_or("ok"),
        )
    } else {
        let data = &v["data"];
        let mut out = format!(
            "error: {} ({})",
            data["code"].as_str().unwrap_or("?"),
            data["message"].as_str().unwrap_or(""),
        );
        if let Some(spec) = data["spec_ref"].as_str() {
            out.push_str(&format!("\n  spec: {spec}"));
        }
        if let Some(remediation) = data["remediation"].as_array() {
            for r in remediation {
                out.push_str(&format!(
                    "\n  fix: {} — {}",
                    r["command"].as_str().unwrap_or(""),
                    r["description"].as_str().unwrap_or(""),
                ));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope_of(command: Command) -> serde_json::Value {
        serde_json::from_str(&run(command)).expect("run() emits JSON")
    }

    #[test]
    fn version_envelope_is_ok_suite() {
        let v = envelope_of(Command::Version);
        assert_eq!(v["ok"].as_bool(), Some(true));
        assert_eq!(v["envelope_version"].as_str(), Some("0.1"));
        assert_eq!(v["envelope_kind"].as_str(), Some("version"));
        assert_eq!(v["cli_version"].as_str(), Some(env!("CARGO_PKG_VERSION")));
        assert_eq!(v["data"]["name"].as_str(), Some("bajan"));
    }

    #[test]
    fn json_flag_parses_after_subcommand() {
        let cli = Cli::try_parse_from(["bajan", "version", "--json"]).expect("parse");
        assert!(cli.json);
        assert!(matches!(cli.command, Command::Version));
    }

    fn staged_seed_node() -> crate::store::ClaimNode {
        crate::store::ClaimNode {
            text: "t".into(),
            valid_at: None,
            invalid_at: None,
            data_cutoff: None,
            status: crate::store::ClaimStatus::Staged,
            scope: "s".into(),
            source_type: "episode".into(),
            evidence: crate::store::Evidence::Unknown,
        }
    }

    #[test]
    fn adopt_subcommand_parses() {
        let cli = Cli::try_parse_from(["bajan", "adopt", "0", "--actor", "sasha"])
            .expect("adopt parses");
        assert!(matches!(cli.command, Command::Adopt { .. }));
    }

    #[test]
    fn adopt_envelope_is_ok_for_seeded_store() {
        // Meter (3.2): adopt emits ok:true once the store holds the claim.
        let mut store = ClaimStore::default();
        store.insert(staged_seed_node(), "e").unwrap();
        let v =
            serde_json::to_value(adopt_envelope(&mut store, &[0], Some("sasha"))).unwrap();
        assert_eq!(v["ok"].as_bool(), Some(true), "adopt emits ok envelope");
    }

    // EDGE-001 (ro5u): a no-op accept must not report success.
    #[test]
    fn adopt_requires_at_least_one_claim_key() {
        assert!(Cli::try_parse_from(["bajan", "adopt"]).is_err());
    }

    #[test]
    fn adopt_cli_on_empty_store_is_honest_error() {
        // No persistence engine yet: a session store holds nothing, so the
        // CLI adopt path must emit a spec-traced error, never fake success.
        let v = envelope_of(Command::Adopt {
            claims: vec![0],
            actor: Some("sasha".into()),
        });
        assert_eq!(v["ok"].as_bool(), Some(false));
        assert_eq!(v["data"]["spec_ref"], "specs/graph-model.md");
    }

    #[test]
    fn every_stub_errors_with_nonempty_remediation() {
        let cases = [
            (Command::Ingest, "specs/ingestion-contract.md"),
            (Command::Extract, "specs/extraction-claims.md"),
            (Command::Resolve, "specs/extraction-claims.md"),
            (Command::Query, "specs/graph-model.md"),
        ];
        for (command, spec) in cases {
            let v = envelope_of(command);
            assert_eq!(v["ok"].as_bool(), Some(false), "{spec}");
            assert_eq!(v["envelope_kind"].as_str(), Some("error"), "{spec}");
            assert_eq!(v["data"]["spec_ref"].as_str(), Some(spec), "{spec}");
            let remediation = v["data"]["remediation"]
                .as_array()
                .unwrap_or_else(|| panic!("remediation present for {spec}"));
            assert!(
                !remediation.is_empty(),
                "Invariant 3.2.5 violated for {spec}"
            );
        }
    }

    #[test]
    fn text_render_includes_remediation_hint() {
        let text = render_text(&run(Command::Ingest));
        assert!(text.contains("error:"), "got: {text}");
        assert!(text.contains("specs/ingestion-contract.md"), "got: {text}");
    }
}