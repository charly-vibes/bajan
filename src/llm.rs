//! Purpose: the real LLM claim extractor (bajan-vg6) — `ex_single_call`
//! over the bajan-9av seam against any OpenAI-compatible chat-completions
//! endpoint. Responsibilities: strict JSON output schema (deny-unknown-
//! fields, deterministic fence strip), chunk assembly for exactly one
//! call (CORR-001 — chunks concatenated into one multi-part prompt,
//! cached under the single (episode id, extractor version) key, never one
//! call per chunk), retry with backoff inside the call, budget exhaustion
//! surfaced as `ExtractionFailure::Exhausted` so the pipeline parks the
//! episode (`ex_single_call` abort; cache-absence requeue per
//! `ex_park_requeue`).
//! Rationale: provider-agnostic by design — base URL + model + API-key
//! ENV VAR NAME (the key itself is read at call time, never stored in
//! config or logs); transport and timing are injected seams so tests run
//! fully scripted with no network and no real key. Evidence containment
//! stays with the pipeline's typed gate: a hallucinated span is
//! gate-rejected `evidence_not_contained`, never repaired. The
//! post-pass stays deterministic (`ex_no_llm_post`): no normalization,
//! dedup, or scoring in the prompt path beyond extraction.

use crate::extract::{CandidateClaim, ExtractionFailure, Extractor};
use crate::ingest::{EpisodeRecord, Locator};
use crate::store::Evidence;
use serde::Deserialize;
use std::cell::Cell;
use std::time::Duration;
use text_splitter::{ChunkConfig, TextSplitter};
use tiktoken_rs::cl100k_base;

pub const SPEC: &str = "specs/extraction-claims.md";

/// Token headroom reserved for the prompt instructions and response
/// (`ex_single_call` sizing: chunk capacity = context budget − overhead).
pub const LLM_PROMPT_OVERHEAD_TOKENS: usize = 512;

/// Default model context budget when none is configured.
pub const LLM_DEFAULT_CONTEXT_TOKENS: usize = 8192;

/// Default retry budget: total attempts per episode per pass
/// (`ex_single_call`: failed calls retry with backoff and park after
/// repeated failure).
pub const LLM_DEFAULT_MAX_ATTEMPTS: u32 = 3;

/// Backoff base between attempts: 2s · 2^(attempt−1), capped (bajan-266,
/// burn-in F2: provider-load windows outlast short backoff — the old
/// 500ms base left 3 attempts ~1.5s of total patience, and cross-pass
/// reruns recovered only 2/12 parked episodes).
const LLM_BACKOFF_BASE_MS: u64 = 2_000;

/// Backoff cap — a run must not stall forever on a wedged endpoint.
const LLM_BACKOFF_CAP_MS: u64 = 60_000;

/// Jittered backoff (bajan-266): the deterministic schedule scaled by a
/// per-call fraction in [0.5, 1.0) derived from the wall clock. Only
/// the SLEEP is randomized — extraction results stay deterministic;
/// the point is decorrelating concurrent passes so simultaneous reruns
/// don't sync their retries onto the same provider load window.
pub fn jittered_backoff(attempt: u32) -> Duration {
    let full_ms = llm_backoff(attempt).as_millis() as u64;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(0);
    // Hash spread + [0.5, 1.0) scale: cheap decorrelation, no rand dep.
    let frac_ms = full_ms / 2 + (nanos >> 17) % (full_ms - full_ms / 2 + 1).max(1);
    Duration::from_millis(frac_ms.min(full_ms))
}

/// Cache identity for the LLM extractor: llm namespace + model id +
/// package version. A model change or a prompt change (version bump)
/// re-extracts every episode under the new key — never silently reuse
/// another model's output.
pub fn llm_version(model_id: &str) -> String {
    format!("llm-{model_id}-{}", env!("CARGO_PKG_VERSION"))
}

/// USD → micro-USD (integer accounting: cap comparisons are exact
/// integer math, never float drift).
pub(crate) fn usd_to_micros(usd: f64) -> u64 {
    (usd * 1_000_000.0).round() as u64
}

/// micro-USD → USD for the machine-readable report surface.
pub(crate) fn micros_to_usd(micros: u64) -> f64 {
    micros as f64 / 1_000_000.0
}

/// Fixed 6-decimal USD display for cost-log lines (bajan-3w4): burn-in
/// spends are micro-dollar scale — trimming would hide them.
pub(crate) fn usd_line(micros: u64) -> String {
    format!("${:.6}", micros_to_usd(micros))
}

/// Deterministic backoff schedule between attempts: 500ms · 2^(attempt−1),
/// capped at 8s. Attempt 1 fails → sleep 500ms, attempt 2 fails → 1s, …
pub fn llm_backoff(attempt: u32) -> Duration {
    let exp = attempt.saturating_sub(1).min(16);
    let ms = LLM_BACKOFF_BASE_MS
        .saturating_mul(1u64 << exp)
        .min(LLM_BACKOFF_CAP_MS);
    Duration::from_millis(ms)
}

/// Process-wide cl100k tokenizer, built once (`cl100k_base()` embeds the
/// BPE tables — constructing it per call dominates chunking cost).
fn tokenizer() -> Option<&'static tiktoken_rs::CoreBPE> {
    static BPE: std::sync::OnceLock<tiktoken_rs::CoreBPE> = std::sync::OnceLock::new();
    // A failure here is permanent (embedded tables); a None answer is
    // cached too via get_or_try_init's stored error — re-probing is
    // harmless and keeps the fallthrough honest.
    match BPE.get() {
        Some(bpe) => Some(bpe),
        None => {
            let bpe = cl100k_base().ok()?;
            Some(BPE.get_or_init(|| bpe))
        }
    }
}

/// Split episode text into chunks within a token budget (cl100k via the
/// pinned text-splitter/tiktoken pair), with trimming disabled so the
/// chunks concatenate to the text VERBATIM — assembly for exactly one
/// call (CORR-001). A text under the budget yields exactly one chunk.
pub fn chunk_episode(text: &str, budget_tokens: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let Some(bpe) = tokenizer() else {
        // Tokenizer unavailable: one honest oversized chunk rather than a
        // fabricated split. The call proceeds; the context budget may be
        // exceeded — that is the endpoint's problem to report.
        return vec![text.to_string()];
    };
    let capacity = budget_tokens
        .saturating_sub(LLM_PROMPT_OVERHEAD_TOKENS)
        .max(1);
    let config = ChunkConfig::new(capacity).with_sizer(bpe).with_trim(false);
    TextSplitter::new(config)
        .chunks(text)
        .map(str::to_string)
        .collect()
}

/// Deterministically strip a markdown code fence around JSON model
/// output — a formatting artifact, not content. Unfenced content is
/// only trimmed. Pure function, no model judgment involved.
pub fn strip_json_fence(content: &str) -> &str {
    let trimmed = content.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let after_info = rest
        .split_once('\n')
        .map(|(_, after)| after)
        .unwrap_or(rest);
    let unwrapped = after_info.trim_end();
    match unwrapped.strip_suffix("```") {
        Some(inner) => inner.trim_end(),
        None => unwrapped,
    }
}

/// The published candidate-claim output schema (`ex_output_schema`):
/// strict — unknown fields are refused so schema drift is caught, not
/// silently tolerated; `text` and `evidence` are required, temporal
/// bounds are optional and must be null when the document states none.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmOutput {
    pub claims: Vec<LlmClaim>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmClaim {
    /// Atomic, self-contained claim text.
    pub text: String,
    /// Verbatim substring of the episode text supporting the claim.
    pub evidence: String,
    /// Civil date (YYYY-MM-DD) when the document states one, else null.
    #[serde(default)]
    pub valid_at: Option<String>,
    #[serde(default)]
    pub invalid_at: Option<String>,
}

/// System prompt: extraction instructions only — no normalization, dedup,
/// or scoring work is asked of the model (`ex_no_llm_post` keeps the
/// post-pass deterministic in code, not in the prompt).
pub const LLM_SYSTEM_PROMPT: &str = "\
You extract atomic, self-contained claims from a source document. \
Return ONLY a JSON object with exactly this shape and no other text: \
{\"claims\":[{\"text\":\"...\",\"evidence\":\"...\",\"valid_at\":null,\"invalid_at\":null}]}. \
Each claim's text must be a single atomic claim in the document's language. \
The evidence must be an exact verbatim substring of the document supporting the claim. \
Set valid_at or invalid_at to a YYYY-MM-DD date only when the document states one; \
otherwise null. Never invent dates, evidence, or claims not supported by the document.";

/// Build the single user prompt from the chunk assembly (CORR-001: one
/// multi-part prompt, one call). Pure function of the chunks.
pub fn build_user_prompt(chunks: &[String]) -> String {
    let mut prompt = String::from("Extract claims from the following source document.\n\n");
    if chunks.len() == 1 {
        prompt.push_str(&chunks[0]);
        return prompt;
    }
    for (i, chunk) in chunks.iter().enumerate() {
        prompt.push_str(&format!("[chunk {}/{}]\n", i + 1, chunks.len()));
        prompt.push_str(chunk);
        prompt.push('\n');
    }
    prompt
}

/// Transport seam: one OpenAI-compatible chat-completions POST. Ok
/// carries the raw response body; Err is a retryable infrastructure
/// failure. Injectable so tests run fully scripted.
pub trait LlmTransport: std::fmt::Debug + Send + Sync {
    fn post(&self, base_url: &str, api_key: &str, body_json: &str) -> Result<String, String>;
}

/// The real transport: blocking HTTP via ureq (rustls), global timeout.
#[derive(Debug)]
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    pub fn with_timeout(timeout: Duration) -> Self {
        let config = ureq::config::Config::builder()
            .timeout_global(Some(timeout))
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::with_timeout(Duration::from_secs(120))
    }
}

impl LlmTransport for UreqTransport {
    fn post(&self, base_url: &str, api_key: &str, body_json: &str) -> Result<String, String> {
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let mut response = self
            .agent
            .post(&url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .send(body_json.as_bytes())
            .map_err(|e| format!("llm transport error: {e}"))?;
        response
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("llm transport read error: {e}"))
    }
}

/// Parse one provider response body: OpenAI-compatible envelope
/// (choices[0].message.content), fence-stripped, then strictly decoded
/// against the published schema. Any deviation is a retryable
/// infrastructure failure — malformed output is the endpoint failing,
/// not a claim-level rejection.
fn parse_response(body: &str) -> Result<(LlmOutput, Option<u64>), String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("provider response is not JSON: {e}"))?;
    let content = value
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| "provider response missing choices[0].message.content".to_string())?;
    let content = strip_json_fence(content);
    let output = serde_json::from_str::<LlmOutput>(content).map_err(|e| {
        format!("model output does not match the published claim schema ({SPEC}): {e}")
    })?;
    // Per-call cost (bajan-8cc): the OpenAI-compatible `usage.cost` field
    // (USD). Absent → None — the caller accounts the call as unaccountable.
    let cost_usd = value
        .get("usage")
        .and_then(|u| u.get("cost"))
        .and_then(|c| c.as_f64());
    let cost_micros = cost_usd.map(|usd| {
        let micros = usd_to_micros(usd);
        if micros == 0 && usd > 0.0 {
            1 // sub-micro-USD costs still count — a cap is never invisible
        } else {
            micros
        }
    });
    Ok((output, cost_micros))
}

/// Type of the injected API-key lookup: receives the configured env-var
/// NAME, returns the key value or None.
pub type KeyLookup = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// The real LLM claim extractor (bajan-vg6 default llm): proposes
/// candidate claims via ONE chat-completions call per episode per pass
/// (`ex_single_call`), retrying with backoff inside the call and
/// surfacing `Exhausted` after the retry budget — the pipeline parks the
/// episode. Deterministic inputs: the prompt is a pure function of the
/// episode text and configuration; the API key is read at call time from
/// the configured env-var NAME (never stored, never logged).
pub struct LlmExtractor {
    version: String,
    model_id: String,
    base_url: String,
    api_key_env: String,
    context_tokens: usize,
    max_attempts: u32,
    /// Spend cap in micro-USD (bajan-8cc). 0 = unbounded — only the
    /// direct constructor path (tests, custom plumbing); the config
    /// selection REQUIRES a cap for llm.
    spend_cap_micros: u64,
    /// Accumulated KNOWN spend this extractor instance (one pass),
    /// micro-USD. Calls whose response lacks `usage.cost` are counted in
    /// `unaccounted_calls` instead and fail the next call closed.
    spent_micros: Cell<u64>,
    /// Calls this instance made whose cost could not be accounted.
    unaccounted_calls: Cell<u32>,
    sleep: Box<dyn Fn(Duration) + Send + Sync>,
    transport: Box<dyn LlmTransport>,
    /// Per-call cost log (bajan-3w4): one line per successful call,
    /// emitted as soon as the cost is known — a burn-in's direct spend
    /// measurement. Default sink is stderr; injected in tests.
    call_log: Box<dyn Fn(String) + Send + Sync>,
    /// Global timeout for the built-in ureq transport (bajan-noj).
    /// Default 120s (the burn-in-verified default); overridden via
    /// BAJAN_EXTRACTOR_TIMEOUT_SECS at selection. A custom transport
    /// (with_transport) owns its own timeout policy.
    transport_timeout: Duration,
    key_lookup: KeyLookup,
}

impl std::fmt::Debug for LlmExtractor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The API key never appears here: only the env-var NAME is held.
        f.debug_struct("LlmExtractor")
            .field("version", &self.version)
            .field("model_id", &self.model_id)
            .field("base_url", &self.base_url)
            .field("api_key_env", &self.api_key_env)
            .field("context_tokens", &self.context_tokens)
            .field("max_attempts", &self.max_attempts)
            .finish_non_exhaustive()
    }
}

impl LlmExtractor {
    /// Build with the real transport, real sleeps, and the process
    /// environment as the key source — override via the `with_*` builders
    /// in tests (scripted transport, no-op sleep, injected lookup).
    pub fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        api_key_env: impl Into<String>,
        context_tokens: usize,
        max_attempts: u32,
    ) -> Self {
        let model_id = model_id.into();
        let transport_timeout = Duration::from_secs(120);
        Self {
            version: llm_version(&model_id),
            model_id,
            base_url: base_url.into(),
            api_key_env: api_key_env.into(),
            context_tokens,
            max_attempts,
            spend_cap_micros: 0,
            spent_micros: Cell::new(0),
            unaccounted_calls: Cell::new(0),
            sleep: Box::new(std::thread::sleep),
            transport: Box::new(UreqTransport::with_timeout(transport_timeout)),
            call_log: Box::new(|line| eprintln!("{line}")),
            transport_timeout,
            key_lookup: Box::new(|key| std::env::var(key).ok()),
        }
    }

    /// Set the built-in transport's global HTTP timeout in seconds
    /// (bajan-noj): carries BAJAN_EXTRACTOR_TIMEOUT_SECS from
    /// `ExtractorConfig::select`. Rebuilds the built-in ureq transport;
    /// a custom transport installed via with_transport owns its own
    /// timeout policy and should not be combined with this builder.
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.transport_timeout = Duration::from_secs(secs);
        self.transport = Box::new(UreqTransport::with_timeout(self.transport_timeout));
        self
    }

    /// The transport timeout actually in force (bajan-noj).
    pub fn transport_timeout(&self) -> Duration {
        self.transport_timeout
    }

    /// Replace the transport (scripted fake in tests, custom endpoint
    /// plumbing elsewhere).
    pub fn with_transport(mut self, transport: impl LlmTransport + 'static) -> Self {
        self.transport = Box::new(transport);
        self
    }

    /// Set the spend cap in micro-USD (bajan-8cc): the pre-call gate
    /// refuses further calls once accumulated known spend has reached it,
    /// and any unaccounted call (response without `usage.cost`) fails the
    /// next call closed — a cap that cannot see spend must not keep
    /// spending.
    pub fn with_spend_cap_micros(mut self, micros: u64) -> Self {
        self.spend_cap_micros = micros;
        self
    }

    /// Replace the per-call cost log (captured in tests; default is
    /// stderr): one line per successful call carrying episode id,
    /// attempt number, the response's exact `usage.cost`, and the
    /// running spend against the cap (bajan-3w4).
    pub fn with_call_log(mut self, log: Box<dyn Fn(String) + Send + Sync>) -> Self {
        self.call_log = log;
        self
    }

    /// Replace the inter-attempt sleep (no-op in tests).
    pub fn with_sleep(mut self, sleep: Box<dyn Fn(Duration) + Send + Sync>) -> Self {
        self.sleep = sleep;
        self
    }

    /// Replace the API-key lookup (injected in tests; the lookup receives
    /// the configured env-var NAME and returns the key value or None).
    pub fn with_key_lookup(mut self, lookup: KeyLookup) -> Self {
        self.key_lookup = lookup;
        self
    }

    /// One retrying call: attempt the transport, retry infra failures
    /// with backoff, and fail with `Exhausted` once the budget is spent.
    /// Malformed responses count against the same budget — the endpoint
    /// failing to honor the schema is an endpoint failure. The last
    /// error detail is deliberately not carried: extraction records hold
    /// machine-readable reasons only (`ex_run_record`), and the budget
    /// outcome — not the endpoint's wording — is the durable fact.
    fn retrying_call(&self, body: &str, episode_id: &str) -> Result<LlmOutput, ExtractionFailure> {
        for attempt in 1..=self.max_attempts {
            let outcome = match (self.key_lookup)(&self.api_key_env) {
                Some(api_key) => self
                    .transport
                    .post(&self.base_url, &api_key, body)
                    .and_then(|response| parse_response(&response).map_err(|e| e.to_string())),
                None => Err(format!(
                    "api key environment variable {} is not set",
                    self.api_key_env
                )),
            };
            if let Ok((output, cost_micros)) = outcome {
                // Account the call's spend (bajan-8cc): known cost
                // accumulates; an unaccountable response (no usage.cost)
                // marks the instance — the next pre-call gate fails
                // closed. Success returns regardless: the money is
                // already spent.
                match cost_micros {
                    Some(micros) => self.spent_micros.set(self.spent_micros.get() + micros),
                    None => {
                        let n = self.unaccounted_calls.get();
                        self.unaccounted_calls.set(n + 1);
                    }
                }
                // Per-call cost log (bajan-3w4): the exact response
                // spend, visible where burn-ins look.
                let cap = self.spend_cap_micros;
                let line = match cost_micros {
                    Some(micros) => {
                        let spent = usd_line(self.spent_micros.get());
                        if cap > 0 {
                            format!(
                                "[llm] {episode_id}: attempt {attempt} cost {} — spent {spent} of {}",
                                usd_line(micros),
                                usd_line(cap)
                            )
                        } else {
                            format!(
                                "[llm] {episode_id}: attempt {attempt} cost {} — spent {spent} (no cap)",
                                usd_line(micros)
                            )
                        }
                    }
                    None => {
                        if cap > 0 {
                            format!(
                                "[llm] {episode_id}: attempt {attempt} cost unaccounted — a capped next call fails closed"
                            )
                        } else {
                            format!("[llm] {episode_id}: attempt {attempt} cost unaccounted")
                        }
                    }
                };
                (self.call_log)(line);
                return Ok(output);
            }
            if attempt < self.max_attempts {
                (self.sleep)(jittered_backoff(attempt));
            }
        }
        Err(ExtractionFailure::Exhausted {
            attempts: self.max_attempts,
        })
    }

    /// The pre-call spend gate (bajan-8cc): once accumulated KNOWN spend
    /// has reached the cap, or a prior call went unaccounted, refuse with
    /// zero new calls. The crossing call is allowed — its cost only
    /// becomes known after the response.
    fn spend_cap_refusal(&self) -> Option<ExtractionFailure> {
        if self.spend_cap_micros == 0 {
            return None;
        }
        let unaccounted = self.unaccounted_calls.get();
        let spent = self.spent_micros.get();
        if spent >= self.spend_cap_micros || unaccounted > 0 {
            return Some(ExtractionFailure::SpendCapExceeded {
                spent_usd: micros_to_usd(spent),
                cap_usd: micros_to_usd(self.spend_cap_micros),
                unaccounted_calls: unaccounted,
            });
        }
        None
    }
}

impl Extractor for LlmExtractor {
    fn version(&self) -> &str {
        &self.version
    }

    fn model_id(&self) -> Option<&str> {
        Some(&self.model_id)
    }

    fn transport_timeout(&self) -> Option<std::time::Duration> {
        Some(self.transport_timeout)
    }

    fn propose(&self, episode: &EpisodeRecord) -> Result<Vec<CandidateClaim>, ExtractionFailure> {
        if episode.text.trim().is_empty() {
            return Ok(Vec::new());
        }
        // Spend gate (bajan-8cc) BEFORE any work: a capped instance that
        // reached its limit makes zero new calls.
        if let Some(refusal) = self.spend_cap_refusal() {
            return Err(refusal);
        }
        // Chunk assembly for exactly ONE call (CORR-001): the chunks are
        // concatenated into a single prompt; the output is cached under
        // the single (episode id, extractor version) key by the pipeline.
        let chunks = chunk_episode(&episode.text, self.context_tokens);
        let prompt = build_user_prompt(&chunks);
        let body = serde_json::json!({
            "model": self.model_id,
            "temperature": 0,
            "messages": [
                {"role": "system", "content": LLM_SYSTEM_PROMPT},
                {"role": "user", "content": prompt},
            ]
        })
        .to_string();

        let output = self.retrying_call(&body, &episode.id)?;
        Ok(output
            .claims
            .into_iter()
            .map(|claim| {
                let evidence = match &episode.locator {
                    Locator::Span(locator) => Evidence::Span {
                        text: claim.evidence,
                        locator: locator.clone(),
                    },
                    Locator::Absent => Evidence::Unknown,
                };
                CandidateClaim {
                    text: claim.text,
                    valid_at: claim.valid_at,
                    invalid_at: claim.invalid_at,
                    evidence,
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_response_extracts_message_content() {
        let body = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "{\"claims\":[]}"}}
            ]
        })
        .to_string();
        let (output, cost) = parse_response(&body).expect("parses");
        assert!(output.claims.is_empty());
        assert_eq!(cost, None, "no usage field: the call is unaccountable");
    }

    #[test]
    fn parse_response_reads_usage_cost() {
        let body = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "{\"claims\":[]}"}}],
            "usage": { "cost": 0.000_012 }
        })
        .to_string();
        let (_, cost) = parse_response(&body).expect("parses");
        assert_eq!(cost, Some(12), "usage.cost 0.000012 USD = 12 micro-USD");
    }

    #[test]
    fn llm_output_schema_is_strict() {
        let err = serde_json::from_str::<LlmOutput>(
            r#"{"claims":[{"text":"a","evidence":"b","confidence":0.9}]}"#,
        )
        .expect_err("unknown field refused");
        assert!(!err.to_string().is_empty());
    }
}
