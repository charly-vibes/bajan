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
use crate::store::StoreError;

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

    /// Embedded SQLite database path (gm_embedded_store: one file, plain
    /// rows). Defaults to `bajan.db` in the working directory.
    #[arg(long, global = true, default_value = "bajan.db")]
    pub db: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print tool identity as a suite envelope.
    Version,
    /// Persist a normalized episode stream verbatim (the stream is read
    /// from stdin as a JSON array of episode records).
    Ingest,
    /// Propose candidate claims from persisted episodes (deterministic
    /// single-call pass over pending episodes).
    Extract,
    /// Resolve claim identity across versions (stub).
    Resolve,
    /// Search the persisted claim graph (first read command).
    Query {
        /// Text to search for (case-insensitive substring match).
        pattern: String,
        /// Bounded-traversal budget: maximum walked claim nodes.
        #[arg(long, default_value_t = 256)]
        budget: usize,
    },
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

    #[error("{0}")]
    Store(String),
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
            status: "vertical-slice",
            specs: vec![
                "specs/ingestion-contract.md",
                "specs/extraction-claims.md",
                "specs/graph-model.md",
                "specs/query-tools.md",
                "specs/entity-review.md",
                "specs/eval-claims.md",
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
    spec: Option<&str>,
    module: &str,
    remediation: Vec<RemediationEntry>,
) -> serde_json::Value {
    let error = ErrorResult::new(code, message, None, spec, Some(module), vec![], remediation)
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
    match err {
        BajanError::NotImplemented { module, spec } => spec_error_envelope(
            "not_implemented",
            &format!("{module} is not implemented in this scaffold"),
            Some(spec),
            module,
            vec![RemediationEntry {
                command: format!("cat {spec}"),
                description: format!(
                    "Read the governing spec for {module}; implementation arrives via the \
                     gated beads tickets (`bd ready`)."
                ),
            }],
        ),
        BajanError::Store(message) => spec_error_envelope(
            "store_error",
            message,
            Some("specs/graph-model.md"),
            "store",
            vec![RemediationEntry {
                command: "bajan --help".into(),
                description: "The embedded store failed; check the database path and \
                              permissions (gm_embedded_store)."
                    .into(),
            }],
        ),
    }
}

/// Envelope for the human accept path (`gm_human_adopt`), backed by the
/// embedded SQLite store: batch accepts loop per claim — each is
/// individually audited and individually reversible; a refusal aborts the
/// batch with earlier adoptions standing.
fn adopt_envelope(db_path: &str, claims: &[usize], actor: Option<&str>) -> serde_json::Value {
    let db = match crate::store::sqlite::SqliteStore::open(db_path) {
        Ok(db) => db,
        Err(err) => return adopt_error_envelope(&err),
    };
    adopt_with(&db, claims, actor)
}

/// The adopt envelope over an already-open store (testable seeding point).
fn adopt_with(
    db: &crate::store::sqlite::SqliteStore,
    claims: &[usize],
    actor: Option<&str>,
) -> serde_json::Value {
    let actor = actor
        .map(str::to_string)
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "unknown".to_string());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();

    let mut audits = Vec::new();
    for &claim_key in claims {
        match db.adopt(claim_key, &actor, now) {
            Ok(()) => {
                if let Some(record) = db.audit().iter().rev().find(|a| a.claim_key == claim_key) {
                    audits.push(record.clone());
                }
            }
            Err(err) => return adopt_error_envelope(&err),
        }
    }
    serde_json::to_value(Envelope::success(
        env!("CARGO_PKG_VERSION"),
        EnvelopeKind::Ok,
        audits,
        vec![],
        vec![],
    ))
    .expect("envelope serialization cannot fail")
}

/// Error envelope for a refused adoption — spec-traced via the shared
/// construction point.
fn adopt_error_envelope(err: &StoreError) -> serde_json::Value {
    spec_error_envelope(
        "adopt_refused",
        &err.to_string(),
        Some("specs/graph-model.md"),
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
pub fn run(command: Command, db_path: &str) -> String {
    let value = match &command {
        Command::Version => {
            serde_json::to_value(version_envelope()).expect("envelope serialization cannot fail")
        }
        Command::Ingest => {
            // The converter submits the normalized episode stream on stdin
            // as a JSON array (ic_stream-schema).
            use std::io::Read;
            let mut input = String::new();
            let _ = std::io::stdin().read_to_string(&mut input);
            ingest_stream_envelope(db_path, &input)
        }
        Command::Extract => extract_envelope(db_path),
        Command::Resolve => stub_envelope(&resolve::run()),
        Command::Query { pattern, budget } => query_envelope(db_path, pattern, *budget),
        Command::Adopt { claims, actor } => adopt_envelope(db_path, claims, actor.as_deref()),
    };
    serde_json::to_string(&value).expect("envelope serialization cannot fail")
}

/// Open the store for a command; the `db_path` carries the `--db` value
/// resolved by the caller (empty string = in-memory, used by unit tests).
fn command_store(db_path: &str) -> Result<crate::store::sqlite::SqliteStore, serde_json::Value> {
    let path = if db_path.is_empty() {
        ":memory:"
    } else {
        db_path
    };
    crate::store::sqlite::SqliteStore::open(path).map_err(|err| {
        spec_error_envelope(
            "store_error",
            &err.to_string(),
            Some("specs/graph-model.md"),
            "store",
            vec![RemediationEntry {
                command: "bajan --help".into(),
                description: "The embedded store could not be opened; check the --db path \
                              and permissions (gm_embedded_store)."
                    .into(),
            }],
        )
    })
}

/// Ingest a submitted episode stream (JSON array of episode records),
/// emitting one outcome record per submitted episode (ic_outcome-schema).
/// Thin wrapper: opens the store, then delegates to `ingest_with`.
fn ingest_stream_envelope(db_path: &str, stream: &str) -> serde_json::Value {
    match command_store(db_path) {
        Ok(db) => ingest_with(&db, stream),
        Err(value) => value,
    }
}

/// The ingest envelope over an already-open store (testable seeding point).
fn ingest_with(db: &crate::store::sqlite::SqliteStore, stream: &str) -> serde_json::Value {
    let records: Result<Vec<ingest::EpisodeRecord>, _> = serde_json::from_str(stream);
    let records = match records {
        Ok(records) => records,
        Err(err) => {
            return spec_error_envelope(
                "malformed_stream",
                &format!("the episode stream is not valid JSON: {err}"),
                Some("specs/ingestion-contract.md"),
                "ingest",
                vec![RemediationEntry {
                    command: "bajan ingest < stream.json --json".into(),
                    description: "Submit the normalized episode stream as a JSON array of \
                                  episode records (see specs/ingestion-contract.md)."
                        .into(),
                }],
            );
        }
    };
    let outcomes: Vec<serde_json::Value> = records
        .iter()
        .map(|record| {
            let outcome = match ingest::persist(db, record) {
                Ok(outcome) => serde_json::to_value(outcome).expect("ingest outcome serializes"),
                Err(err) => {
                    serde_json::json!({ "outcome": "store_error", "message": err.to_string() })
                }
            };
            serde_json::json!({
                "episode_id": record.id,
                "outcome": outcome,
            })
        })
        .collect();
    serde_json::to_value(Envelope::success(
        env!("CARGO_PKG_VERSION"),
        EnvelopeKind::Ok,
        serde_json::json!({ "outcomes": outcomes }),
        vec![],
        vec![],
    ))
    .expect("envelope serialization cannot fail")
}

/// Run the deterministic extraction pass and emit its report. Thin wrapper
/// opening the store from `--db`.
fn extract_envelope(db_path: &str) -> serde_json::Value {
    let db = match command_store(db_path) {
        Ok(db) => db,
        Err(value) => return value,
    };
    extract_with(&db)
}

/// The extraction envelope over an already-open store (testable).
fn extract_with(db: &crate::store::sqlite::SqliteStore) -> serde_json::Value {
    match extract::run_extract(db, env!("CARGO_PKG_VERSION")) {
        Ok(report) => serde_json::to_value(Envelope::success(
            env!("CARGO_PKG_VERSION"),
            EnvelopeKind::Ok,
            report,
            vec![],
            vec![],
        ))
        .expect("envelope serialization cannot fail"),
        Err(err) => stub_envelope(&Err(err)),
    }
}

/// Run the first read query and emit the result record (qt_query-schema:
/// budget-status vocabulary complete/budget-exhausted, hits carry status
/// verbatim and their persisted-episode lineage).
fn query_envelope(db_path: &str, pattern: &str, budget: usize) -> serde_json::Value {
    let db = match command_store(db_path) {
        Ok(db) => db,
        Err(value) => return value,
    };
    query_with(&db, pattern, budget)
}

/// The query envelope over an already-open store (testable seeding point).
fn query_with(
    db: &crate::store::sqlite::SqliteStore,
    pattern: &str,
    budget: usize,
) -> serde_json::Value {
    match query::search(db, pattern, budget) {
        Ok(result) => serde_json::to_value(Envelope::success(
            env!("CARGO_PKG_VERSION"),
            EnvelopeKind::Ok,
            result,
            vec![],
            vec![],
        ))
        .expect("envelope serialization cannot fail"),
        Err(err) => stub_envelope(&Err(BajanError::Store(err.to_string()))),
    }
}

/// Exit status for an emitted envelope: 0 when the envelope reports `ok`,
/// 1 when it reports failure. Machine consumers gating on exit status must
/// never see success for an error envelope (bajan-aan).
pub fn exit_code(json: &str) -> i32 {
    let v: serde_json::Value =
        serde_json::from_str(json).expect("bajan always emits valid JSON envelopes");
    if v["ok"].as_bool().unwrap_or(false) {
        0
    } else {
        1
    }
}

/// Human-readable one-line message from a clap error (bajan-3w1):
/// - `DisplayHelpOnMissingArgumentOrSubcommand` (bare `bajan`) renders the
///   about line first — that's the tool's purpose, not the mistake, so
///   substitute an explicit "subcommand required" message.
/// - Other kinds render `error: <summary>` then detail lines; the summary
///   alone is often useless ("the following required arguments were not
///   provided:"), so take the first *paragraph* (up to the blank line
///   separating message from usage), newline-collapsed to one line.
fn clap_error_message(err: &clap::Error) -> String {
    if err.kind() == clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand {
        return "a subcommand is required (run bajan --help)".to_string();
    }
    let rendered = err.to_string();
    let paragraph = rendered
        .split("\n\n")
        .next()
        .unwrap_or(&rendered)
        .trim_start_matches("error: ");
    let mut message = String::new();
    for (i, line) in paragraph.lines().enumerate() {
        if i > 0 {
            message.push_str("; ");
        }
        message.push_str(line.trim());
    }
    if message.is_empty() {
        return "argument error".to_string();
    }
    message
}

/// Envelope for clap parse failures (bajan-ts6): the envelope is the single
/// output format, so a bad invocation must still emit ok:false JSON — never
/// clap's plain-text usage error. `spec_ref` is `None`: argument errors are
/// governed by the genesis envelope contract itself, not a pipeline spec.
pub fn argument_error_envelope(err: &clap::Error) -> String {
    let message = clap_error_message(err);
    serde_json::to_string(&spec_error_envelope(
        "argument_error",
        &message,
        None,
        "cli",
        vec![RemediationEntry {
            command: "bajan --help".into(),
            description: "Run with --help for the accepted subcommands and flags.".into(),
        }],
    ))
    .expect("envelope serialization cannot fail")
}

/// Render an envelope JSON string as a short human-readable line.
///
/// Text mode is a convenience rendering of the same envelope — never a
/// second output format with different content.
pub fn render_text(json: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(json).expect("run() always emits valid JSON");
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
    use crate::ingest::{EpisodeRecord, Locator, SourceMeta};

    fn envelope_of(command: Command) -> serde_json::Value {
        serde_json::from_str(&run(command, ":memory:")).expect("run() emits JSON")
    }

    fn seed_episode() -> EpisodeRecord {
        EpisodeRecord {
            id: "ep-seed".into(),
            text: "The parser resolves spans deterministically.".into(),
            locator: Locator::Span("heading:Notes".into()),
            source: SourceMeta {
                source_type: "episode".into(),
                data_cutoff: Some("2026-01-01".into()),
                authority_tier: 2,
                tags: vec!["workspace:dev".into()],
            },
        }
    }

    fn staged_seed_node() -> (crate::store::ClaimNode, crate::store::Lineage) {
        (
            crate::store::ClaimNode {
                text: "The parser resolves spans deterministically.".into(),
                valid_at: None,
                invalid_at: None,
                data_cutoff: Some("2026-01-01".into()),
                status: crate::store::ClaimStatus::Staged,
                scope: "workspace:dev".into(),
                source_type: "episode".into(),
                evidence: crate::store::Evidence::Span {
                    text: "The parser resolves spans deterministically.".into(),
                    locator: "heading:Notes".into(),
                },
            },
            crate::store::Lineage {
                episode_id: "ep-seed".into(),
                extractor_version: "0.1.0".into(),
            },
        )
    }

    fn stream_json() -> String {
        serde_json::to_string(&[seed_episode()]).expect("stream serializes")
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

    // ic_outcome-schema: every submitted episode gets exactly one outcome
    // record; the vertical slice's thin path is ingest → extract → query.
    #[test]
    fn ingest_then_extract_then_query_end_to_end() {
        let db = crate::store::sqlite::SqliteStore::open_in_memory().unwrap();

        let v = ingest_with(&db, &stream_json());
        assert_eq!(v["ok"].as_bool(), Some(true), "ingest emits ok: {v}");
        assert_eq!(
            v["data"]["outcomes"][0]["outcome"]["outcome"], "persisted",
            "the episode persisted"
        );

        // Idempotence at the CLI layer: re-submitting converges.
        let v = ingest_with(&db, &stream_json())
            .as_object()
            .cloned()
            .unwrap();
        let _ = &v; // outcome records already checked in ingest tests

        let report = extract_with(&db);
        assert_eq!(report["ok"].as_bool(), Some(true));
        assert_eq!(report["data"]["episodes_processed"], 1);

        let result = query_with(&db, "parser", 100);
        assert_eq!(result["ok"].as_bool(), Some(true), "query emits ok");
        assert_eq!(result["data"]["status"], "complete");
        assert_eq!(result["data"]["hits"].as_array().map(Vec::len), Some(1));
        assert_eq!(result["data"]["hits"][0]["status"], "staged");
        assert_eq!(result["data"]["hits"][0]["episodes"][0], "ep-seed");
    }

    // qt_bounded_traversal: a truncated CLI search reports budget-exhausted —
    // two matching claims seeded, budget 1: the walk must stop early and say
    // so rather than report a silently partial `complete`.
    #[test]
    fn query_envelope_reports_budget_exhaustion_honestly() {
        let db = crate::store::sqlite::SqliteStore::open_in_memory().unwrap();
        ingest_with(&db, &stream_json());
        extract_with(&db);
        // A second matching claim so the budget (1) stops the walk with
        // claims remaining — the precondition `budget-exhausted` exists to
        // report.
        let (mut node, lineage) = staged_seed_node();
        node.text = "The parser resolves spans predictably.".into();
        db.insert_claim(&node, &[lineage], "ep-seed-2").unwrap();
        db.insert_episode(&EpisodeRecord {
            id: "ep-seed-2".into(),
            ..seed_episode()
        })
        .unwrap();
        let v = query_with(&db, "parser", 1);
        assert_eq!(v["data"]["status"], "budget-exhausted");
        assert_eq!(v["data"]["hits"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn adopt_subcommand_parses() {
        let cli =
            Cli::try_parse_from(["bajan", "adopt", "0", "--actor", "sasha"]).expect("adopt parses");
        assert!(matches!(cli.command, Command::Adopt { .. }));
    }

    #[test]
    fn adopt_envelope_is_ok_for_seeded_store() {
        let db = crate::store::sqlite::SqliteStore::open_in_memory().unwrap();
        db.insert_episode(&seed_episode()).unwrap();
        let (node, lineage) = staged_seed_node();
        db.insert_claim(
            &node,
            &[lineage],
            "The parser resolves spans deterministically.",
        )
        .unwrap();
        let v = adopt_with(&db, &[0], Some("sasha"));
        assert_eq!(v["ok"].as_bool(), Some(true), "adopt emits ok envelope");
        assert_eq!(v["data"][0]["actor"], "sasha");
    }

    // EDGE-001 (ro5u): a no-op accept must not report success.
    #[test]
    fn adopt_requires_at_least_one_claim_key() {
        assert!(Cli::try_parse_from(["bajan", "adopt"]).is_err());
    }

    #[test]
    fn adopt_cli_on_empty_store_is_honest_error() {
        let db = crate::store::sqlite::SqliteStore::open_in_memory().unwrap();
        let v = adopt_with(&db, &[0], Some("sasha"));
        assert_eq!(v["ok"].as_bool(), Some(false));
        assert_eq!(v["data"]["spec_ref"], "specs/graph-model.md");
    }

    #[test]
    fn resolve_still_stub_errors_with_remediation() {
        let v = envelope_of(Command::Resolve);
        assert_eq!(v["ok"].as_bool(), Some(false));
        assert_eq!(v["envelope_kind"].as_str(), Some("error"));
        assert_eq!(
            v["data"]["spec_ref"].as_str(),
            Some("specs/extraction-claims.md")
        );
        let remediation = v["data"]["remediation"].as_array().unwrap();
        assert!(!remediation.is_empty(), "Invariant 3.2.5");
    }

    #[test]
    fn malformed_stream_is_a_spec_traced_error() {
        let db = crate::store::sqlite::SqliteStore::open_in_memory().unwrap();
        let v = ingest_with(&db, "not json at all");
        assert_eq!(v["ok"].as_bool(), Some(false));
        assert_eq!(v["data"]["spec_ref"], "specs/ingestion-contract.md");
    }

    #[test]
    fn exit_code_is_zero_only_for_ok_envelopes() {
        // bajan-aan: exit status mirrors the envelope.
        assert_eq!(exit_code(&run(Command::Version, ":memory:")), 0);
        assert_eq!(exit_code(&run(Command::Resolve, ":memory:")), 1);
    }

    #[test]
    fn text_render_includes_remediation_hint() {
        let text = render_text(&run(Command::Resolve, ":memory:"));
        assert!(text.contains("error:"), "got: {text}");
        assert!(text.contains("specs/extraction-claims.md"), "got: {text}");
    }
}
