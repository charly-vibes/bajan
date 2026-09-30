//! Purpose: the LLM-assisted contradiction proposer (bajan-a8r) — the
//! second contradiction producer, human-gated by
//! specs/contradiction-review.md.
//! Responsibilities: one chat-completions call per pass over the ranked
//! claims (reusing the extractor arc's `llm.rs` transport, backoff, and
//! fence-strip discipline), strict JSON output schema
//! (deny-unknown-fields), honest refusal of hallucinated claim keys, and
//! staging each valid pair through the store's `propose_contradiction`
//! guard — a queue row, NEVER a `contradicts` edge (`cr_proposal_entry`).
//! Rationale: the deterministic scan's edit classes are closed and sound,
//! so it writes edges directly (`gm_contradicts_provenance`); an LLM sees
//! no such closure — its output is unverified opinion and must wait for a
//! human (`cr_human_resolution`). The API key is referenced by env-var
//! NAME and read at call time, never stored or logged.

use crate::llm::{LlmTransport, llm_backoff};
use crate::query::{self, BajanQueryError};
use crate::store::sqlite::SqliteStore;
use serde::Deserialize;

pub const SPEC: &str = "specs/contradiction-review.md";

/// The published proposer output schema: strict — unknown fields are
/// refused so schema drift is caught, not silently tolerated.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmContradictionOutput {
    pub pairs: Vec<LlmPair>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmPair {
    /// Claim key of one side (as listed in the prompt).
    pub from: usize,
    /// Claim key of the other side.
    pub to: usize,
    /// Why the model believes these contradict — carried onto the
    /// proposal row for queue transparency (`cr_queue_transparency`).
    pub rationale: String,
}

/// System prompt: contradiction identification only. The model sees claim
/// keys and texts; it may only cite keys it was shown — anything else is
/// refused downstream (`cr_proposal_entry`).
pub const PROPOSER_SYSTEM_PROMPT: &str = "\
You identify contradictory claim pairs. Return ONLY a JSON object with \
exactly this shape and no other text: \
{\"pairs\":[{\"from\":1,\"to\":2,\"rationale\":\"...\"}]}. \
`from` and `to` must be claim keys from the numbered list in the document; \
`rationale` is one short sentence explaining the contradiction. \
Propose only genuinely contradictory pairs; propose nothing when unsure.";

/// Build the user prompt from the ranked claims: one numbered claim per
/// line (key + verbatim text). Pure function of the claims.
pub fn build_proposer_prompt(claims: &[(usize, &str)]) -> String {
    let mut prompt = String::from("Identify contradictory pairs among these claims.\n\n");
    for (key, text) in claims {
        prompt.push_str(&format!("{key}. {text}\n"));
    }
    prompt
}

/// Parse one provider response body: OpenAI-compatible envelope
/// (choices[0].message.content), fence-stripped, then strictly decoded
/// against the published proposer schema. Any deviation is a retryable
/// infrastructure failure — the endpoint failing, not a pair-level
/// rejection.
fn parse_proposer_response(body: &str) -> Result<LlmContradictionOutput, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("provider response is not JSON: {e}"))?;
    let content = value
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| "provider response missing choices[0].message.content".to_string())?;
    let content = crate::llm::strip_json_fence(content);
    serde_json::from_str::<LlmContradictionOutput>(content).map_err(|e| {
        format!("model output does not match the published proposer schema ({SPEC}): {e}")
    })
}

/// Type of the injected API-key lookup: receives the configured env-var
/// NAME, returns the key value or None.
pub type KeyLookup = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// The LLM contradiction proposer: one chat-completions call per pass,
/// retrying infrastructure failures with the shared backoff schedule and
/// surfacing honest exhaustion. The API key is read at call time from the
/// configured env-var NAME (never stored, never logged).
pub struct LlmContradictionProposer {
    model_id: String,
    base_url: String,
    api_key_env: String,
    max_attempts: u32,
    sleep: Box<dyn Fn(std::time::Duration) + Send + Sync>,
    transport: Box<dyn LlmTransport>,
    key_lookup: KeyLookup,
}

impl std::fmt::Debug for LlmContradictionProposer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The API key never appears here: only the env-var NAME is held.
        f.debug_struct("LlmContradictionProposer")
            .field("model_id", &self.model_id)
            .field("base_url", &self.base_url)
            .field("api_key_env", &self.api_key_env)
            .field("max_attempts", &self.max_attempts)
            .finish_non_exhaustive()
    }
}

impl LlmContradictionProposer {
    /// Build with the real transport, real sleeps, and the process
    /// environment as the key source — override via the `with_*` builders
    /// in tests (scripted transport, no-op sleep, injected lookup).
    pub fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        api_key_env: impl Into<String>,
        max_attempts: u32,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            base_url: base_url.into(),
            api_key_env: api_key_env.into(),
            max_attempts: max_attempts.max(1),
            sleep: Box::new(std::thread::sleep),
            transport: Box::new(crate::llm::UreqTransport::default()),
            key_lookup: Box::new(|key| std::env::var(key).ok()),
        }
    }

    /// Replace the transport (scripted fake in tests).
    pub fn with_transport(mut self, transport: impl LlmTransport + 'static) -> Self {
        self.transport = Box::new(transport);
        self
    }

    /// Replace the inter-attempt sleep (no-op in tests).
    pub fn with_sleep(mut self, sleep: Box<dyn Fn(std::time::Duration) + Send + Sync>) -> Self {
        self.sleep = sleep;
        self
    }

    /// Replace the API-key lookup (injected in tests).
    pub fn with_key_lookup(mut self, lookup: KeyLookup) -> Self {
        self.key_lookup = lookup;
        self
    }

    /// The producer identity stamped on every proposal this proposer
    /// stages (`cr_proposal_entry`): `llm:<model_id>`.
    pub fn producer(&self) -> String {
        format!("llm:{}", self.model_id)
    }

    /// One retrying call: attempt the transport, retry infra failures
    /// with backoff, fail after the budget. The last error detail is
    /// deliberately not carried — the budget outcome, not the endpoint's
    /// wording, is the durable fact.
    fn retrying_call(&self, body: &str) -> Result<LlmContradictionOutput, ()> {
        for attempt in 1..=self.max_attempts {
            let outcome = match (self.key_lookup)(&self.api_key_env) {
                Some(api_key) => self
                    .transport
                    .post(&self.base_url, &api_key, body)
                    .and_then(|response| parse_proposer_response(&response)),
                None => Err(format!(
                    "api key environment variable {} is not set",
                    self.api_key_env
                )),
            };
            if let Ok(output) = outcome {
                return Ok(output);
            }
            if attempt < self.max_attempts {
                (self.sleep)(llm_backoff(attempt));
            }
        }
        Err(())
    }
}

/// Configuration for the proposer read from the extractor arc's env
/// surface: model, base URL, and the env-var NAME of the API key. Missing
/// params are a configuration error surfaced before any call — never a
/// mid-run guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposerConfig {
    pub model_id: String,
    pub base_url: String,
    pub api_key_env: String,
}

/// Proposer configuration errors: every variant names the missing surface.
#[derive(Debug, thiserror::Error)]
pub enum ProposerConfigError {
    #[error("missing required configuration parameter {param} for the LLM contradiction proposer")]
    MissingParam { param: &'static str },
}

impl ProposerConfig {
    /// Read from an injected lookup (the process env in production).
    pub fn from_env_with(
        lookup: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, ProposerConfigError> {
        let model_id =
            lookup("BAJAN_EXTRACTOR_MODEL").ok_or(ProposerConfigError::MissingParam {
                param: "BAJAN_EXTRACTOR_MODEL",
            })?;
        let base_url =
            lookup("BAJAN_EXTRACTOR_BASE_URL").ok_or(ProposerConfigError::MissingParam {
                param: "BAJAN_EXTRACTOR_BASE_URL",
            })?;
        let api_key_env =
            lookup("BAJAN_EXTRACTOR_API_KEY_ENV").ok_or(ProposerConfigError::MissingParam {
                param: "BAJAN_EXTRACTOR_API_KEY_ENV",
            })?;
        Ok(Self {
            model_id,
            base_url,
            api_key_env,
        })
    }

    /// Read from the process environment.
    pub fn from_env() -> Result<Self, ProposerConfigError> {
        Self::from_env_with(&|key| std::env::var(key).ok())
    }
}

/// Report of one contradiction-proposal pass, mirroring
/// `ContradictionReport`: the honest traversal status, the pairs staged
/// into the proposal queue, the pairs refused (hallucinated keys,
/// self-pairs, store guards), and whether the budget ran out.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProposeReport {
    /// Typed traversal outcome (`qt_bounded_traversal` discipline).
    pub status: query::BudgetStatus,
    /// Proposals staged (queue row written, NO edge).
    pub proposed: usize,
    /// Pairs refused: hallucinated claim keys, self-pairs, duplicates,
    /// repropose-guard refusals — every refusal counted, never silent.
    pub refused: usize,
    /// Pairs citing a claim key not among the presented claims
    /// (`cr_proposal_entry`: refused honestly, never repaired).
    pub hallucinated: usize,
    /// The staged pairs, as (from_claim, to_claim) with the lower claim
    /// key first, in staging order.
    pub pairs: Vec<(usize, usize)>,
    /// True when the budget ran out — candidates remained unseen.
    pub exhausted: bool,
}

/// Run the LLM contradiction-proposal pass over ranked claims: ONE call
/// presents the ranked claim list; each returned pair citing two distinct
/// presented keys stages as a queue row with `llm:<model_id>` producer
/// provenance (`cr_proposal_entry`) — never an edge. Hallucinated keys
/// and invalid pairs are refused and counted. The budget bounds the
/// ranked-candidate walk inside `search`; exhaustion is honest.
pub fn run_propose_pass(
    db: &SqliteStore,
    proposer: &LlmContradictionProposer,
    budget: usize,
    now: u64,
) -> Result<ProposeReport, BajanQueryError> {
    let result = query::search(db, "", budget)?;
    let hits = result.hits;

    let mut proposed = Vec::new();
    let mut refused = 0usize;
    let mut hallucinated = 0usize;
    let mut seen: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();

    if !hits.is_empty() {
        let claims: Vec<(usize, &str)> = hits
            .iter()
            .map(|h| (h.claim_key, h.text.as_str()))
            .collect();
        let prompt = build_proposer_prompt(&claims);
        let body = serde_json::json!({
            "model": proposer.model_id,
            "temperature": 0,
            "messages": [
                {"role": "system", "content": PROPOSER_SYSTEM_PROMPT},
                {"role": "user", "content": prompt},
            ]
        })
        .to_string();

        // Exhaustion inside the call (retry budget spent): honest stop,
        // nothing staged — a half-proposed pass would be a broken pass.
        let Ok(output) = proposer.retrying_call(&body) else {
            return Ok(ProposeReport {
                status: query::BudgetStatus::Complete,
                proposed: 0,
                refused: 0,
                hallucinated: 0,
                pairs: Vec::new(),
                exhausted: true,
            });
        };

        let known: std::collections::HashSet<usize> = hits.iter().map(|h| h.claim_key).collect();
        for pair in output.pairs {
            // Lower claim key first (the orientation the contradiction
            // query treats symmetrically and the queue stores).
            let (from, to) = if pair.from <= pair.to {
                (pair.from, pair.to)
            } else {
                (pair.to, pair.from)
            };
            if from == to {
                refused += 1;
                continue;
            }
            if !known.contains(&from) || !known.contains(&to) {
                hallucinated += 1;
                continue;
            }
            if !seen.insert((from, to)) {
                refused += 1;
                continue;
            }
            let from_text = hits
                .iter()
                .find(|h| h.claim_key == from)
                .map(|h| h.text.as_str())
                .unwrap_or_default();
            let to_text = hits
                .iter()
                .find(|h| h.claim_key == to)
                .map(|h| h.text.as_str())
                .unwrap_or_default();
            let fingerprint = serde_json::json!({
                "texts": [from_text, to_text],
                "rationale": pair.rationale,
                "model": proposer.model_id,
            })
            .to_string();
            match db.propose_contradiction(
                from,
                to,
                &proposer.producer(),
                &pair.rationale,
                &fingerprint,
                now,
            ) {
                Ok(crate::store::ProposeOutcome::Queued) => proposed.push((from, to)),
                Ok(_) => refused += 1,
                Err(e) => return Err(BajanQueryError(e.to_string())),
            }
        }
    }

    let exhausted = result.status == query::BudgetStatus::BudgetExhausted;
    Ok(ProposeReport {
        status: if exhausted {
            query::BudgetStatus::BudgetExhausted
        } else {
            query::BudgetStatus::Complete
        },
        proposed: proposed.len(),
        refused,
        hallucinated,
        pairs: proposed,
        exhausted,
    })
}
