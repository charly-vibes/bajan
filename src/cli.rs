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

/// Convert a stub module result into an error envelope.
///
/// Invariant 3.2.5: remediation is non-empty by construction — the
/// `ErrorResult::new` constructor itself rejects an empty list.
fn stub_envelope(result: &Result<(), BajanError>) -> Envelope<ErrorResult> {
    let err = match result {
        Ok(()) => unreachable!("scaffold stubs always fail; wire success only when implemented"),
        Err(e) => e,
    };
    let (module, spec) = match err {
        BajanError::NotImplemented { module, spec } => (*module, *spec),
    };
    let error = ErrorResult::new(
        "not_implemented",
        &format!("{module} is not implemented in this scaffold"),
        None,
        Some(spec),
        Some(module),
        vec![],
        vec![RemediationEntry {
            command: format!("cat {spec}"),
            description: format!(
                "Read the governing spec for {module}; implementation arrives via the \
                 gated beads tickets (`bd ready`)."
            ),
        }],
    )
    .expect("remediation is non-empty by construction (Invariant 3.2.5)");
    Envelope::error(env!("CARGO_PKG_VERSION"), error, vec![])
}

/// Run a command and return its suite envelope as JSON.
///
/// This is the single serialization point: every command's output passes
/// through here, so the envelope contract cannot be bypassed.
pub fn run(command: Command) -> String {
    let value = match &command {
        Command::Version => serde_json::to_value(version_envelope())
            .expect("envelope serialization cannot fail"),
        Command::Ingest => serde_json::to_value(stub_envelope(&ingest::run()))
            .expect("envelope serialization cannot fail"),
        Command::Extract => serde_json::to_value(stub_envelope(&extract::run()))
            .expect("envelope serialization cannot fail"),
        Command::Resolve => serde_json::to_value(stub_envelope(&resolve::run()))
            .expect("envelope serialization cannot fail"),
        Command::Query => serde_json::to_value(stub_envelope(&query::run()))
            .expect("envelope serialization cannot fail"),
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

    #[test]
    #[test]
    fn adopt_subcommand_parses_and_emits_ok_envelope() {
        let cli = Cli::try_parse_from(["bajan", "adopt", "0", "--actor", "sasha"])
            .expect("adopt parses");
        assert!(matches!(cli.command, Command::Adopt { .. }));
        // Meter: `bajan adopt ... --json` emits ok:true (3.2 green).
        let v = envelope_of(Command::Adopt {
            claims: vec![0],
            actor: Some("sasha".into()),
        });
        assert_eq!(v["ok"].as_bool(), Some(true), "adopt emits ok envelope");
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