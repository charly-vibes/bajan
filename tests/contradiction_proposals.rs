//! Purpose: property + scenario tests for the LLM contradiction-proposal
//! queue (bajan-a8r) — the second contradiction producer, human-gated.
//! Responsibilities: red-first derivation of specs/contradiction-review.md —
//! proposals stage without writing edges (cr_proposal_entry), only an
//! actor-identified human command resolves (cr_human_resolution), rejected
//! pairs re-enter only with changed evidence (cr_repropose_guard), the
//! queue is transparent (cr_queue_transparency), the deterministic scan is
//! untouched (cr_scan_untouched), and dump-recreate preserves the queue
//! (gm_embedded_store).
//! Rationale: LLM output is unverified opinion — it must never land in the
//! graph as an edge without a human; every refusal is honest and counted.

use std::process::Command;

use bajan::contradict;
use bajan::ingest::{EpisodeRecord, Locator, SourceMeta};
use bajan::propose::{LlmContradictionProposer, ProposeReport, ProposerConfig};
use bajan::query::BudgetStatus;
use bajan::store::ProposeOutcome;
use bajan::store::sqlite::SqliteStore;
use bajan::store::{EdgeLabel, Evidence, Lineage, ProposalDecision, ProposalStatus, StoreError};
use bajan::{ingest, store};
use proptest::prelude::*;

// --- shared seed helpers (mirroring tests/contradiction_scan.rs) ----------

fn episode(id: &str, text: &str) -> EpisodeRecord {
    EpisodeRecord {
        id: id.into(),
        text: text.into(),
        locator: Locator::Span("heading:Notes".into()),
        source: SourceMeta {
            source_type: "episode".into(),
            data_cutoff: None,
            authority_tier: 1,
            tags: vec!["workspace:dev".into()],
        },
    }
}

fn seed(db: &SqliteStore, ep: &EpisodeRecord, text: &str) -> usize {
    ingest::persist(db, ep).expect("persist");
    let node = store::ClaimNode {
        text: text.into(),
        valid_at: None,
        invalid_at: None,
        data_cutoff: None,
        status: store::ClaimStatus::Staged,
        scope: "workspace:dev".into(),
        source_type: "episode".into(),
        evidence: Evidence::Span {
            text: text.into(),
            locator: "heading:Notes".into(),
        },
    };
    db.insert_claim(
        &node,
        &[Lineage {
            episode_id: ep.id.clone(),
            extractor_version: "0.1.0".into(),
        }],
        &ep.text,
    )
    .expect("insert")
}

fn scripted_transport(responses: Vec<Result<String, String>>) -> impl bajan::llm::LlmTransport {
    use std::sync::Mutex;
    #[derive(Debug)]
    struct Scripted(Mutex<std::vec::IntoIter<Result<String, String>>>);
    impl bajan::llm::LlmTransport for Scripted {
        fn post(&self, _base: &str, _key: &str, _body: &str) -> Result<String, String> {
            self.0
                .lock()
                .expect("script")
                .next()
                .expect("transport called beyond script")
        }
    }
    Scripted(Mutex::new(responses.into_iter()))
}

fn no_sleep() -> Box<dyn Fn(std::time::Duration) + Send + Sync> {
    Box::new(|_| {})
}

fn proposer_with(script: Vec<Result<String, String>>) -> LlmContradictionProposer {
    LlmContradictionProposer::new("test-model", "https://api.example.test/v1", "TEST_KEY", 3)
        .with_transport(scripted_transport(script))
        .with_sleep(no_sleep())
        .with_key_lookup(Box::new(|_| Some("sk-test".into())))
}

fn pairs_body(pairs: &[(usize, usize, &str)]) -> String {
    let inner: Vec<String> = pairs
        .iter()
        .map(|(f, t, r)| format!("{{\"from\":{f},\"to\":{t},\"rationale\":\"{r}\"}}"))
        .collect();
    let content = format!("{{\"pairs\":[{}]}}", inner.join(","));
    serde_json::json!({"choices":[{"message":{"role":"assistant","content":content}}]}).to_string()
}

// --- cr_proposal_entry ----------------------------------------------------

#[test]
fn valid_pair_stages_proposal_without_edge() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, &episode("ep1", "body one"), "The service is up.");
    let b = seed(&db, &episode("ep2", "body two"), "The service is down.");
    let outcome = db
        .propose_contradiction(a, b, "llm:test-model", "direct negation", "{}", 100)
        .expect("propose");
    assert_eq!(outcome, ProposeOutcome::Queued);
    // No contradicts edge at proposal time — the graph is untouched.
    let edges = db.edges().expect("edges");
    assert!(
        !edges
            .iter()
            .any(|(_, label, _)| *label == EdgeLabel::Contradicts),
        "no contradicts edge may exist before human approval"
    );
    let queue = db.contradiction_queue().expect("queue");
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].from_claim, a.min(b));
    assert_eq!(queue[0].to_claim, a.max(b));
    assert_eq!(queue[0].status, ProposalStatus::Proposed);
    assert_eq!(queue[0].producer, "llm:test-model");
    assert_eq!(queue[0].rationale, "direct negation");
}

#[test]
fn self_pair_refused() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, &episode("ep1", "body one"), "claim one.");
    let err = db
        .propose_contradiction(a, a, "llm:test-model", "r", "{}", 100)
        .expect_err("self-pair refused");
    assert!(matches!(err, StoreError::InvalidContradictionPair { .. }));
}

#[test]
fn hallucinated_key_refused_honestly() {
    let db = SqliteStore::open_in_memory().expect("open");
    let _a = seed(&db, &episode("ep1", "body one"), "claim one.");
    let err = db
        .propose_contradiction(0, 999, "llm:test-model", "r", "{}", 100)
        .expect_err("unknown key refused");
    assert!(matches!(err, StoreError::ClaimNotFound { claim_key: 999 }));
}

#[test]
fn duplicate_open_proposal_refused() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, &episode("ep1", "body one"), "claim one.");
    let b = seed(&db, &episode("ep2", "body two"), "claim two.");
    db.propose_contradiction(a, b, "llm:test-model", "r1", "{}", 100)
        .expect("first");
    // Same unordered pair, either orientation — the queue drains, never doubles.
    let outcome = db
        .propose_contradiction(b, a, "llm:test-model", "r2", "{}", 200)
        .expect("propose returns outcome");
    assert_eq!(outcome, ProposeOutcome::AlreadyProposed);
    assert_eq!(db.contradiction_queue().expect("queue").len(), 1);
}

// --- cr_human_resolution ---------------------------------------------------

#[test]
fn approve_writes_provenanced_edge_and_audit() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, &episode("ep1", "body one"), "claim one.");
    let b = seed(&db, &episode("ep2", "body two"), "claim two.");
    db.propose_contradiction(a, b, "llm:test-model", "r", "{}", 100)
        .expect("propose");
    db.resolve_contradiction_proposal(a, b, ProposalDecision::Approved, "alice", 200)
        .expect("resolve");
    let edges = db.edges_with_provenance().expect("edges");
    let contradicts: Vec<_> = edges
        .iter()
        .filter(|(_, label, _, _)| *label == EdgeLabel::Contradicts)
        .collect();
    assert_eq!(contradicts.len(), 1, "exactly one contradicts edge");
    let provenance = contradicts[0].3.as_deref().expect("provenance present");
    let prov: serde_json::Value = serde_json::from_str(provenance).expect("provenance json");
    assert_eq!(prov["producer"], "llm:test-model");
    assert_eq!(prov["approved_by"], "alice");
    // Queue drains; exactly one audit record.
    assert!(db.contradiction_queue().expect("queue").is_empty());
    let audits = db.contradiction_audits().expect("audits");
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].actor, "alice");
    assert_eq!(audits[0].decision, ProposalDecision::Approved);
    // A resolved proposal cannot resolve again.
    let err = db
        .resolve_contradiction_proposal(a, b, ProposalDecision::Approved, "alice", 300)
        .expect_err("double resolve refused");
    assert!(matches!(err, StoreError::ProposalNotProposed { .. }));
}

#[test]
fn reject_writes_no_edge_and_audits() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, &episode("ep1", "body one"), "claim one.");
    let b = seed(&db, &episode("ep2", "body two"), "claim two.");
    db.propose_contradiction(a, b, "llm:test-model", "r", "{}", 100)
        .expect("propose");
    db.resolve_contradiction_proposal(a, b, ProposalDecision::Rejected, "bob", 200)
        .expect("resolve");
    assert!(
        !db.edges()
            .expect("edges")
            .iter()
            .any(|(_, l, _)| *l == EdgeLabel::Contradicts),
        "rejection writes no edge"
    );
    assert_eq!(db.contradiction_audits().expect("audits").len(), 1);
}

#[test]
fn resolution_requires_actor() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, &episode("ep1", "body one"), "claim one.");
    let b = seed(&db, &episode("ep2", "body two"), "claim two.");
    db.propose_contradiction(a, b, "llm:test-model", "r", "{}", 100)
        .expect("propose");
    let err = db
        .resolve_contradiction_proposal(a, b, ProposalDecision::Approved, "", 200)
        .expect_err("empty actor refused");
    assert!(matches!(err, StoreError::ProposalActorRequired { .. }));
}

// --- cr_repropose_guard ----------------------------------------------------

#[test]
fn rejected_pair_repropose_guard() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, &episode("ep1", "body one"), "claim one.");
    let b = seed(&db, &episode("ep2", "body two"), "claim two.");
    let fingerprint = "{\"texts\":[\"claim one.\",\"claim two.\"]}";
    db.propose_contradiction(a, b, "llm:test-model", "r", fingerprint, 100)
        .expect("propose");
    db.resolve_contradiction_proposal(a, b, ProposalDecision::Rejected, "bob", 200)
        .expect("reject");
    // Unchanged fingerprint: refused, rejection audit intact.
    let outcome = db
        .propose_contradiction(a, b, "llm:test-model", "r", fingerprint, 300)
        .expect("propose returns outcome");
    assert_eq!(outcome, ProposeOutcome::RejectedWithoutNewEvidence);
    assert!(db.contradiction_queue().expect("queue").is_empty());
    // Changed fingerprint: re-queued once.
    let outcome = db
        .propose_contradiction(
            a,
            b,
            "llm:test-model",
            "new rationale",
            "{\"changed\":true}",
            400,
        )
        .expect("propose");
    assert_eq!(outcome, ProposeOutcome::Queued);
    assert_eq!(db.contradiction_queue().expect("queue").len(), 1);
    assert_eq!(db.contradiction_audits().expect("audits").len(), 1);
}

// --- cr_queue_transparency --------------------------------------------------

#[test]
fn queue_lists_open_oldest_first_with_producer_and_rationale() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, &episode("ep1", "body one"), "claim one.");
    let b = seed(&db, &episode("ep2", "body two"), "claim two.");
    let c = seed(&db, &episode("ep3", "body three"), "claim three.");
    db.propose_contradiction(a, b, "llm:m1", "older", "{}", 500)
        .expect("propose");
    db.propose_contradiction(a, c, "llm:m2", "newer", "{}", 900)
        .expect("propose");
    let queue = db.contradiction_queue().expect("queue");
    assert_eq!(queue.len(), 2);
    assert_eq!(queue[0].queued_at, 500, "oldest first");
    assert_eq!(queue[1].queued_at, 900);
    assert_eq!(queue[0].producer, "llm:m1");
    assert_eq!(queue[1].rationale, "newer");
}

// --- cr_scan_untouched -------------------------------------------------------

#[test]
fn scan_untouched_by_queue_activity() {
    let db = SqliteStore::open_in_memory().expect("open");
    let _ = seed(&db, &episode("ep1", "body one"), "The service is up.");
    let _ = seed(&db, &episode("ep2", "body two"), "The service is not up.");
    let before = contradict::run_contradiction_pass(&db, 256, 100).expect("scan");
    assert_eq!(before.proposed, 1);
    // Queue a proposal, resolve it, re-run the scan — identical report shape.
    let a = before.pairs[0].0;
    let b = before.pairs[0].1;
    db.propose_contradiction(a, b, "llm:m", "r", "{}", 200)
        .expect("propose");
    db.resolve_contradiction_proposal(a, b, ProposalDecision::Rejected, "bob", 300)
        .expect("reject");
    let after = contradict::run_contradiction_pass(&db, 256, 400).expect("scan");
    assert_eq!(after.status, BudgetStatus::Complete);
    assert_eq!(after.refused, 1, "re-run refuses the scan's own edge");
    assert_eq!(after.proposed, 0);
}

// --- proposer (scripted transport) -------------------------------------------

#[test]
fn proposer_stages_valid_pairs_with_llm_producer() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, &episode("ep1", "body one"), "The server is in Berlin.");
    let b = seed(&db, &episode("ep2", "body two"), "The server is in Paris.");
    let proposer = proposer_with(vec![Ok(pairs_body(&[(a, b, "different cities")]))]);
    let report: ProposeReport =
        bajan::propose::run_propose_pass(&db, &proposer, 256, 100).expect("pass");
    assert_eq!(report.status, BudgetStatus::Complete);
    assert_eq!(report.proposed, 1);
    assert_eq!(report.hallucinated, 0);
    let queue = db.contradiction_queue().expect("queue");
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].producer, "llm:test-model");
    assert_eq!(queue[0].rationale, "different cities");
    assert!(
        !db.edges()
            .expect("edges")
            .iter()
            .any(|(_, l, _)| *l == EdgeLabel::Contradicts),
        "proposer never writes edges"
    );
}

#[test]
fn proposer_refuses_hallucinated_keys_without_staging() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, &episode("ep1", "body one"), "claim one.");
    let _b = seed(&db, &episode("ep2", "body two"), "claim two.");
    // Claim key 42 does not exist — refused, never repaired, never staged.
    let proposer = proposer_with(vec![Ok(pairs_body(&[(a, 42, "hallucinated")]))]);
    let report = bajan::propose::run_propose_pass(&db, &proposer, 256, 100).expect("pass");
    assert_eq!(report.proposed, 0);
    assert_eq!(report.hallucinated, 1);
    assert!(db.contradiction_queue().expect("queue").is_empty());
}

#[test]
fn proposer_malformed_output_exhausts_without_partial_state() {
    let db = SqliteStore::open_in_memory().expect("open");
    let _a = seed(&db, &episode("ep1", "body one"), "claim one.");
    let _b = seed(&db, &episode("ep2", "body two"), "claim two.");
    let proposer = proposer_with(vec![
        Ok("not json".into()),
        Err("llm transport error: 503".into()),
        Err("llm transport error: 503".into()),
    ]);
    let report = bajan::propose::run_propose_pass(&db, &proposer, 256, 100).expect("pass");
    assert_eq!(report.proposed, 0, "exhausted budget stages nothing");
    assert!(report.exhausted, "honest exhaustion");
    assert!(db.contradiction_queue().expect("queue").is_empty());
}

#[test]
fn proposer_missing_api_key_exhausts_without_staging() {
    let db = SqliteStore::open_in_memory().expect("open");
    let _a = seed(&db, &episode("ep1", "body one"), "claim one.");
    let _b = seed(&db, &episode("ep2", "body two"), "claim two.");
    let proposer = LlmContradictionProposer::new(
        "test-model",
        "https://api.example.test/v1",
        "MISSING_KEY",
        2,
    )
    .with_transport(scripted_transport(vec![Ok(pairs_body(&[(1, 2, "x")]))]))
    .with_sleep(no_sleep())
    .with_key_lookup(Box::new(|_| None));
    let report = bajan::propose::run_propose_pass(&db, &proposer, 256, 100).expect("pass");
    assert_eq!(report.proposed, 0);
    assert!(report.exhausted);
}

// --- config (env-var NAME never the key) --------------------------------------

#[test]
fn proposer_config_from_env() {
    let config = ProposerConfig::from_env_with(&|key| match key {
        "BAJAN_EXTRACTOR_MODEL" => Some("gpt-4o-mini".into()),
        "BAJAN_EXTRACTOR_BASE_URL" => Some("https://api.example.com/v1".into()),
        "BAJAN_EXTRACTOR_API_KEY_ENV" => Some("MY_KEY".into()),
        _ => None,
    })
    .expect("full config parses");
    assert_eq!(config.model_id, "gpt-4o-mini");
    assert_eq!(config.base_url, "https://api.example.com/v1");
    assert_eq!(config.api_key_env, "MY_KEY");
    let err = ProposerConfig::from_env_with(&|_| None).expect_err("missing params refused");
    assert!(matches!(
        err,
        bajan::propose::ProposerConfigError::MissingParam {
            param: "BAJAN_EXTRACTOR_MODEL"
        }
    ));
}

// --- dump-recreate (gm_embedded_store) -----------------------------------------

#[test]
fn dump_recreate_preserves_proposal_queue_and_audits() {
    let db = SqliteStore::open_in_memory().expect("open");
    let a = seed(&db, &episode("ep1", "body one"), "claim one.");
    let b = seed(&db, &episode("ep2", "body two"), "claim two.");
    let c = seed(&db, &episode("ep3", "body three"), "claim three.");
    db.propose_contradiction(a, b, "llm:m", "r1", "{}", 100)
        .expect("propose");
    db.propose_contradiction(a, c, "llm:m", "r2", "{}", 110)
        .expect("propose");
    db.resolve_contradiction_proposal(a, b, ProposalDecision::Approved, "alice", 120)
        .expect("approve");

    let dump = db.dump_extraction_output().expect("dump");
    let restored = SqliteStore::open_in_memory().expect("open");
    // Episodes must be restored for the lineage claims to resolve.
    for ep in [
        episode("ep1", "body one"),
        episode("ep2", "body two"),
        episode("ep3", "body three"),
    ] {
        ingest::persist(&restored, &ep).expect("persist");
    }
    restored.restore_extraction_output(&dump).expect("restore");

    let queue = restored.contradiction_queue().expect("queue");
    assert_eq!(queue.len(), 1, "open proposal survives");
    assert_eq!(queue[0].from_claim, a.min(c));
    assert_eq!(queue[0].producer, "llm:m");
    let audits = restored.contradiction_audits().expect("audits");
    assert_eq!(audits.len(), 1, "audit trail survives");
    assert_eq!(audits[0].actor, "alice");
    // And the approved edge survives too (existing dump discipline).
    assert_eq!(
        restored
            .edges()
            .expect("edges")
            .iter()
            .filter(|(_, l, _)| *l == EdgeLabel::Contradicts)
            .count(),
        1
    );
}

// --- binary-level CLI contract (propose misconfig envelope) --------------

/// Run the real `bajan` binary (mirrors tests/cli.rs; named to avoid the
/// edition-2024 crate-name shadowing a bare `bajan` fn would cause).
fn run_bajan(args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_bajan"))
        .args(args)
        .output()
        .expect("binary built by cargo test");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

// A proposer run without the BAJAN_EXTRACTOR_* config must emit a
// spec-traced error envelope and exit non-zero — never a mid-run guess.
#[test]
fn propose_without_config_emits_error_envelope() {
    let dir = std::env::temp_dir().join(format!(
        "bajan-cprop-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let db = dir.join("b.db");
    let db = db.to_str().expect("path");
    let (code, stdout) = run_bajan(&["--db", db, "--json", "propose"]);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("envelope");
    assert_eq!(v["ok"].as_bool(), Some(false));
    assert_eq!(v["data"]["code"].as_str(), Some("proposer_config_error"));
    assert_eq!(
        v["data"]["spec_ref"].as_str(),
        Some("specs/contradiction-review.md")
    );
    assert_ne!(code, 0, "misconfigured propose exits non-zero");
    std::fs::remove_dir_all(&dir).ok();
}

// --- p_proposal_entry / p_human_resolution proptest -----------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn p_proposal_lifecycle(
        tier in 0u8..3,
        first in 1usize..500,
        second in 1usize..500,
        approve in proptest::bool::ANY,
    ) {
        let db = SqliteStore::open_in_memory().expect("open");
        let text = format!("claim number {first} at tier {tier}.");
        let other = format!("claim number {second} differs.");
        let a = seed(&db, &episode("ep1", "body one"), &text);
        let b = seed(&db, &episode("ep2", "body two"), &other);
        prop_assert!(a != b);
        let outcome = db
            .propose_contradiction(a, b, "llm:m", "r", "{}", 100)
            .expect("propose");
        prop_assert_eq!(outcome, ProposeOutcome::Queued);
        prop_assert!(db.edges().expect("edges").iter().all(|(_, l, _)| *l != EdgeLabel::Contradicts));
        let decision = if approve { ProposalDecision::Approved } else { ProposalDecision::Rejected };
        db.resolve_contradiction_proposal(a, b, decision, "actor", 200)
            .expect("resolve");
        let edges = db.edges().expect("edges");
        let contradict_count = edges.iter().filter(|(_, l, _)| *l == EdgeLabel::Contradicts).count();
        if approve {
            prop_assert_eq!(contradict_count, 1);
        } else {
            prop_assert_eq!(contradict_count, 0);
        }
        prop_assert_eq!(db.contradiction_audits().expect("audits").len(), 1);
        prop_assert!(db.contradiction_queue().expect("queue").is_empty());
    }
}
