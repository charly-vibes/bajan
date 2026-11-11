//! Purpose: extraction module — propose candidate claims typed against the
//! published claim schema, with lineage edges to their source episode.
//! Responsibilities: boundary between persisted episodes and proposed
//! claims; claim-schema conformance at the typed gate.
//! Rationale: governed by specs/extraction-claims.md; this scaffold ships
//! the module seam only (bajan-2xv), behavior arrives with the gated
//! implementation tickets.

use crate::cli::BajanError;
use crate::ingest::EpisodeRecord;
use crate::store::{ClaimNode, Evidence, Lineage};
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

#[cfg(test)]
use proptest::prelude::*;

pub const SPEC: &str = "specs/extraction-claims.md";

/// The class of an extractor failure (bajan-gso): timeout vs HTTP
/// status vs generic transport vs schema-decode — diagnosable from the
/// report without reproducing the call. Carried on park reasons and
/// `Exhausted` failures; never on claim nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FailureClass {
    /// The configured transport timeout elapsed.
    Timeout,
    /// The provider returned an HTTP error status.
    HttpStatus { code: u16 },
    /// Any other transport-level error (connection refused, DNS, auth
    /// plumbing).
    Transport,
    /// The response body did not decode against the published schema.
    Schema,
}

impl FailureClass {
    /// Stable machine-readable code for the text render (gso).
    pub fn code(&self) -> String {
        match self {
            FailureClass::Timeout => "timeout".into(),
            FailureClass::HttpStatus { code } => format!("http-{code}"),
            FailureClass::Transport => "transport".into(),
            FailureClass::Schema => "schema".into(),
        }
    }
}

impl std::fmt::Display for FailureClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.code())
    }
}

/// Which fixed-in-code non-claim shape rule matched a candidate
/// (`ex_shape_rejection`, bajan-w17, design D1). Explainable per rule:
/// each names a reason the text is not an assertion. Serializes
/// snake_case for run-row storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeRule {
    /// Interrogative-final text after whitespace collapse — a question is
    /// not an assertion.
    InterrogativeFinal,
    /// Heading/section-title fragment — direct markdown heading text, or a
    /// candidate whose text aligns inside a converter-supplied heading
    /// anchor (an `ex_deterministic_structure` marker).
    HeadingFragment,
    /// Navigation/list boilerplate — breadcrumb chains, menu rows, bare
    /// list markers, or marker-led fragments with no asserted content.
    NavBoilerplate,
}

/// Machine-readable reason for a failure outcome (`ex_run_record`).
///
/// Reasons are stable snake_case codes stored on the run or rejection
/// record — never on a claim node (`gm_schema_v2` field list stays
/// closed). Each variant names the invariant that produces it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum Reason {
    /// Episode parked in `rejected` after repeated call failure
    /// (`ex_single_call`). Carries the attempt count and the failure
    /// class of the last error (bajan-gso).
    RepeatedCallFailure { attempts: u32, class: FailureClass },
    /// Candidate rejected at the validation gate because its evidence span
    /// fails whitespace-collapsed containment (`ex_evidence_containment`,
    /// enforced from bajan-0hs.7).
    EvidenceNotContained,
    /// Candidate rejected because persisting it would drop hedge marker(s)
    /// present in the supporting episode text (`gm_hedge_anchor`).
    HedgeMarkerDropped,
    /// Prior-version `staged` claim tombstoned by a version-bump
    /// re-extraction (`ex_supersession`). Carried on the supersession
    /// record in the claim store — never on the claim node.
    SupersededByReextraction,
    /// Every candidate refused by the deterministic section-kind filter
    /// (`ex_section_filter`): the episode's tags include a non-knowledge
    /// section kind. Carries the offending tag.
    SectionFiltered { tag: String },
    /// Candidate refused by the deterministic shape filter
    /// (`ex_shape_rejection`): its text matches the fixed-in-code
    /// non-claim deny set. Carries the matched rule.
    ShapeFiltered { rule: ShapeRule },
}

impl Reason {
    /// Stable machine-readable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            Reason::RepeatedCallFailure { .. } => "repeated_call_failure",
            Reason::EvidenceNotContained => "evidence_not_contained",
            Reason::HedgeMarkerDropped => "hedge_marker_dropped",
            Reason::SupersededByReextraction => "superseded_by_reextraction",
            Reason::SectionFiltered { .. } => "section_filtered",
            Reason::ShapeFiltered { .. } => "shape_filtered",
        }
    }
}

/// Finish status of an extraction call (`ex_run_record`).
///
/// Failure outcomes carry their machine-readable reason structurally — a
/// parked or gate-rejected run cannot exist without one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Finish {
    /// Call succeeded; candidates reached the gate outcome.
    Succeeded,
    /// Episode parked in `rejected` after repeated call failure
    /// (`ex_single_call` abort path).
    Parked { reason: Reason },
    /// Call succeeded but candidates were refused at the gate
    /// (`ex_typed_gate` refuse path).
    GateRejected { reason: Reason },
}

/// One run row per extraction call (`ex_run_record`): episode id,
/// extractor version, model id when the extractor uses an LLM, started
/// and finished timestamps (epoch millis), and finish status. Parallel
/// to — and without changing — the `(episode id, extractor version)`
/// cache economics: rows are additive telemetry, never cache entries.
/// `shape_filtered` records the non-claim rules matched by candidates
/// that the shape filter refused while the call itself succeeded
/// (`ex_shape_rejection`): additive too — old rows without the field
/// deserialize to an empty list, and an empty list serializes away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractionRun {
    pub episode_id: String,
    pub extractor_version: String,
    pub model_id: Option<String>,
    pub started_at: u64,
    pub finished_at: u64,
    pub finish: Finish,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shape_filtered: Vec<ShapeRule>,
}

/// The run-record store: append-only, one row per extraction call
/// (`ex_run_record`). Parallel to the `(episode id, extractor version)`
/// cache — recording a run never creates, invalidates, or reuses a cache
/// entry.
#[derive(Debug, Default, Clone)]
pub struct ExtractionRunStore {
    rows: Vec<ExtractionRun>,
}

impl ExtractionRunStore {
    /// Record exactly one run row for one extraction call.
    pub fn record(&mut self, run: ExtractionRun) {
        self.rows.push(run);
    }

    pub fn rows(&self) -> std::slice::Iter<'_, ExtractionRun> {
        self.rows.iter()
    }
}

/// One park outcome on the report (`ex_single_call` abort path): the
/// parked episode and its machine-readable reason — repeated call
/// failure after the extractor's retry budget was exhausted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParkedEpisode {
    pub episode_id: String,
    pub reason: Reason,
}

/// One gate refusal on the report (`ex_typed_gate` refuse path): the
/// refused episode and its machine-readable reason (`ex_run_record` —
/// reasons live on rejection records, never on claim nodes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateRejection {
    pub episode_id: String,
    pub reason: Reason,
}

/// One candidate claim proposed by an extractor — exactly the fields an
/// extractor may set: claim text, temporal bounds, and typed evidence.
/// Status, scope, source type, data cutoff, and lineage are NOT the
/// extractor's business — the pipeline projects them from the episode
/// record and its configuration (`gm_schema_v2` stays closed; extractors
/// never fabricate metadata they cannot know).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateClaim {
    pub text: String,
    pub valid_at: Option<String>,
    pub invalid_at: Option<String>,
    pub evidence: Evidence,
}

/// Why an extractor failed to propose for one episode.
/// Infrastructure failures (network, auth, malformed provider response)
/// are NOT gate rejections — they fail the run honestly (`ex_run_record`
/// class separation, bajan-r1h discipline).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ExtractionFailure {
    /// The extractor could not complete its call. Propagates — the run
    /// fails; the episode stays pending and retryable.
    #[error("extraction infrastructure failure: {0}")]
    Infrastructure(String),
    /// The extractor exhausted its retry budget (`ex_single_call`
    /// repeated-failure abort). The PIPELINE parks the episode: run row
    /// `Parked` with reason `repeated_call_failure`, report entry, no
    /// cache entry — the next pass re-attempts with a fresh budget
    /// (`ex_park_requeue`).
    #[error("extraction retry budget exhausted after {attempts} attempts ({class})")]
    Exhausted { attempts: u32, class: FailureClass },
    /// The configured spend cap refused further calls (bajan-8cc): the
    /// pre-call gate found accumulated spend at/over the cap, or a prior
    /// call went unaccounted (response without `usage.cost` — a cap
    /// that cannot see spend must not keep spending). The pass STOPS:
    /// the episode stays pending (never parked), and the report carries
    /// the stop (`spend_cap`).
    #[error(
        "spend cap reached: spent ${spent_usd:.6} of ${cap_usd:.6} cap, {unaccounted_calls} unaccounted call(s)"
    )]
    SpendCapExceeded {
        spent_usd: f64,
        cap_usd: f64,
        unaccounted_calls: u32,
    },
}

/// The extractor seam (bajan-9av): the boundary between persisted
/// episodes and candidate proposals. `ex_single_call`/`ex_run_record`
/// name the contract but not the implementation — this trait makes
/// "the proposer differs, the contract does not" structural.
///
/// Implementors propose candidates for ONE episode per `propose` call;
/// the pipeline (not the extractor) owns the gate, the cache, the
/// run-row telemetry, supersession, and scope/lineage projection.
/// A version bump (any change to `version()`) re-extracts every episode
/// under the new key — the version is the cache identity.
pub trait Extractor: std::fmt::Debug {
    /// Cache-identity version of this extractor (`ex_single_call`:
    /// the (episode id, extractor version) pair keys the cache).
    fn version(&self) -> &str;
    /// Model id when this extractor uses an LLM — lands verbatim on the
    /// run rows (`ex_run_record`); `None` for deterministic extractors.
    fn model_id(&self) -> Option<&str> {
        None
    }
    /// Propose candidate claims for one episode. Batch semantics: the
    /// returned set is staged all-or-nothing — one gate failure in the
    /// batch refuses the whole episode's candidates (no partial
    /// staging, per-episode gate outcome).
    fn propose(&self, episode: &EpisodeRecord) -> Result<Vec<CandidateClaim>, ExtractionFailure>;

    /// The network timeout actually in force for this extractor's
    /// transport (bajan-noj); `None` for extractors with no transport.
    /// Selection-testable: BAJAN_EXTRACTOR_TIMEOUT_SECS must yield a
    /// transport carrying exactly that timeout.
    fn transport_timeout(&self) -> Option<std::time::Duration> {
        None
    }
}

/// The vertical-slice extractor report: one deterministic single-call
/// pass over pending episodes (`ex_single_call`), one candidate per
/// episode, typed at the door by the gate (`ex_typed_gate`), cached per
/// (episode id, extractor version).
///
/// Outcome classes are separated (bajan-r1h): a persisted candidate
/// counts in `candidates_proposed`; a gate refusal is recorded per
/// episode in `gate_rejections` with its machine-readable reason. A
/// zero-candidate episode is yet another class — legitimately extracted
/// and cached as empty (`ex_typed_gate`) — and cannot arise in this
/// slice: the deterministic proposer always proposes exactly one
/// candidate. A version-bump re-extraction additionally tombstones the
/// episode's prior-version `staged` claims (`ex_supersession`), counted
/// in `superseded` — `staged` claims take the supersession path; an
/// `active` claim is never mutated (`ex_supersession`): instead, a new
/// output that CONFLICTS with it stages an invalidation proposal citing
/// the causing episode (`ex_mutation_proposal`), counted in
/// `proposals_staged`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExtractReport {
    pub episodes_processed: usize,
    pub candidates_proposed: usize,
    pub superseded: usize,
    /// Invalidation proposals staged this pass against ACTIVE claims
    /// whose evidence position the new output conflicts with
    /// (`ex_mutation_proposal`). Duplicates (same claim + causing
    /// episode) are idempotent skips, never re-counted.
    pub proposals_staged: usize,
    pub gate_rejections: Vec<GateRejection>,
    /// Episodes parked this pass after retry-budget exhaustion
    /// (`ex_single_call` abort; `ex_park_requeue`: no cache entry — the
    /// next pass re-attempts with a fresh budget).
    pub parked: Vec<ParkedEpisode>,
    /// The spend-cap stop (bajan-8cc): Some when the pass ended early
    /// because the configured cost cap refused further calls. The
    /// stopped episode and everything after it stay PENDING (never
    /// parked) — a raised cap re-attempts them with the cache intact.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spend_cap: Option<SpendCapStop>,
}

/// The machine-readable spend-cap stop (bajan-8cc): known spend at the
/// stop, the cap, calls whose cost could not be accounted (responses
/// without `usage.cost` — enforcement failed closed), and how many
/// episodes were never attempted because of the stop.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpendCapStop {
    pub spent_usd: f64,
    pub cap_usd: f64,
    pub unaccounted_calls: u32,
    pub episodes_left_pending: usize,
}

/// Classify a store refusal at the wired gate (bajan-r1h): a gate-level
/// refusal maps to its machine-readable reason; an infrastructure
/// failure (SQL) is never a gate rejection — it propagates and the run
/// fails honestly instead of being swallowed into the report. The wired
/// path pre-validates batches (see run_extract_with); this classifier
/// survives for run-row/test parity of the refusal taxonomy.
#[allow(dead_code)]
fn classify_refusal(err: &crate::store::StoreError) -> Result<Reason, BajanError> {
    match err {
        crate::store::StoreError::HedgeMarkerDropped { .. } => Ok(Reason::HedgeMarkerDropped),
        other => Err(BajanError::Store(other.to_string())),
    }
}

/// Epoch milliseconds for run-row timestamps (`ex_run_record`).
fn epoch_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Minimum claim-sized unit length in chars (bajan-5zy): sentences (or
/// accumulated fragments) below this merge into the next unit — no
/// orphan fragments like a bare `Dr.` abbreviation sentence.
pub const ATOMIZER_MIN_UNIT_CHARS: usize = 20;

/// Maximum claim-sized unit length in chars (bajan-5zy): units above
/// this split at semicolon connectors (`; `) — never mid-word. A run-on
/// with no semicolons stays whole: splitting is best-effort, documented,
/// never a hard cut (an honest oversized unit beats a mangled one).
pub const ATOMIZER_MAX_UNIT_CHARS: usize = 300;

/// Cache-identity version of the default atomizer: namespaced under
/// `atomic-` so stores extracted by the legacy whole-episode proposer
/// (plain package version) re-extract — with supersession — when the
/// default switched (bajan-5zy): the upgrade must not silently reuse
/// legacy whole-episode output.
pub fn atomic_default_version() -> String {
    format!("atomic-{}", env!("CARGO_PKG_VERSION"))
}

/// Segment episode text into atomic claim-sized units (bajan-5zy):
/// UAX #29 sentence boundaries (unicode-segmentation, `=`-pinned), then
/// (1) merge fragments below `ATOMIZER_MIN_UNIT_CHARS` — forward while
/// accumulating, and the trailing short fragment merges back into the
/// previous unit; (2) split units above `ATOMIZER_MAX_UNIT_CHARS` at
/// semicolon connectors, the semicolon staying with the left part and
/// the following whitespace dropped.
///
/// Deterministic pure function: same text → same units. Every unit is a
/// verbatim substring of the text (byte-slice of the original), and the
/// units re-concatenate to the text modulo whitespace — `ic_verbatim`
/// holds by construction, so the typed gate's whitespace-collapsed
/// containment and hedge-marker survival pass without repair.
pub fn atomic_claim_units(text: &str) -> Vec<String> {
    // 1. UAX #29 sentence spans (byte offsets), trimmed, non-empty.
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut offset = 0usize;
    for sentence in text.split_sentence_bounds() {
        let start = offset;
        offset += sentence.len();
        let lead = sentence.len() - sentence.trim_start().len();
        let s = start + lead;
        let end = s + sentence.trim().len();
        if end > s {
            spans.push((s, end));
        }
    }
    if spans.is_empty() {
        return Vec::new();
    }

    // 2. Merge below the minimum: accumulate forward — a unit still under
    // the minimum absorbs the next sentence; a trailing short fragment
    // merges back into the previous unit.
    let mut units: Vec<(usize, usize)> = Vec::new();
    for (s, e) in spans {
        match units.last_mut() {
            Some(last) if char_len(text, last.0, last.1) < ATOMIZER_MIN_UNIT_CHARS => {
                last.1 = e;
            }
            _ => units.push((s, e)),
        }
    }
    if units.len() > 1 {
        let last = *units.last().expect("len > 1 checked");
        if char_len(text, last.0, last.1) < ATOMIZER_MIN_UNIT_CHARS {
            units.pop();
            let prev = units.last_mut().expect("len > 1 checked");
            prev.1 = last.1;
        }
    }

    // 3. Split run-ons above the maximum at semicolon connectors: the
    // `;` stays with the left part; the whitespace after it is dropped
    // (re-concatenation modulo whitespace still holds). Parts still over
    // the maximum are emitted whole — never a hard cut mid-word.
    let mut out = Vec::new();
    for (s, e) in units {
        if char_len(text, s, e) <= ATOMIZER_MAX_UNIT_CHARS {
            out.push(text[s..e].to_string());
            continue;
        }
        let mut parts: Vec<(usize, usize)> = Vec::new();
        let mut part_start = s;
        let mut cursor = s;
        while let Some(pos) = text[cursor..e].find(';') {
            let semi = cursor + pos;
            parts.push((part_start, semi + 1));
            let mut next = semi + 1;
            while next < e {
                let Some(c) = text[next..].chars().next() else {
                    break;
                };
                if !c.is_whitespace() {
                    break;
                }
                next += c.len_utf8();
            }
            cursor = next;
            part_start = next;
        }
        parts.push((part_start, e));
        for (ps, pe) in parts {
            if pe > ps {
                out.push(text[ps..pe].to_string());
            }
        }
    }
    out
}

fn char_len(text: &str, start: usize, end: usize) -> usize {
    text[start..end].chars().count()
}

/// The deterministic sentence atomizer (bajan-5zy default extractor):
/// upgrades the no-LLM proposer from whole-episode text to atomic claims
/// (`ex_no_llm_post`-compliant, fully offline). One candidate per
/// claim-sized unit (`atomic_claim_units`), verbatim span evidence with
/// the episode locator (typed absent marker when the episode has none).
/// Deterministic: `model_id()` stays `None`; the version is the
/// cache identity (`atomic_default_version` for the default build).
#[derive(Debug)]
pub struct SentenceAtomizer {
    version: String,
}

impl SentenceAtomizer {
    pub fn new(version: impl Into<String>) -> Self {
        Self {
            version: version.into(),
        }
    }
}

impl Extractor for SentenceAtomizer {
    fn version(&self) -> &str {
        &self.version
    }
    fn propose(&self, episode: &EpisodeRecord) -> Result<Vec<CandidateClaim>, ExtractionFailure> {
        Ok(atomic_claim_units(&episode.text)
            .into_iter()
            .map(|unit| {
                let evidence = match &episode.locator {
                    crate::ingest::Locator::Span(locator) => Evidence::Span {
                        text: unit.clone(),
                        locator: locator.clone(),
                    },
                    crate::ingest::Locator::Absent => Evidence::Unknown,
                };
                CandidateClaim {
                    text: unit,
                    valid_at: None,
                    invalid_at: None,
                    evidence,
                }
            })
            .collect())
    }
}

/// The legacy deterministic proposer (bajan-9av): the
/// whole-episode-text candidate of the vertical slice, byte-stable with
/// the pre-seam behavior — one candidate per pending episode, verbatim
/// span evidence (locator from the episode record when derivable, typed
/// absent marker otherwise). Deterministic: `model_id()` stays `None`
/// and the version is carried by the caller's run context. Kept behind
/// config as the byte-stable mode for existing stores (bajan-5zy — the
/// default is now the sentence atomizer).
#[derive(Debug)]
struct LegacyProposer {
    version: String,
}

impl Extractor for LegacyProposer {
    fn version(&self) -> &str {
        &self.version
    }
    fn propose(&self, episode: &EpisodeRecord) -> Result<Vec<CandidateClaim>, ExtractionFailure> {
        let evidence = match &episode.locator {
            crate::ingest::Locator::Span(locator) => Evidence::Span {
                text: episode.text.clone(),
                locator: locator.clone(),
            },
            crate::ingest::Locator::Absent => Evidence::Unknown,
        };
        Ok(vec![CandidateClaim {
            text: episode.text.clone(),
            valid_at: None,
            invalid_at: None,
            evidence,
        }])
    }
}

/// Extractor selection configuration (bajan-9av): which extractor runs,
/// and its provider-agnostic LLM parameters. Provider-agnostic on
/// purpose — base URL + model + API-key ENV VAR NAME (never the key
/// itself in config); no provider SDK crates (anti-goal: heavyweight
/// provider deps in bajan). Consumed by bajan-vg6's LLM extractor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExtractorConfig {
    /// Extractor kind: `atomic` (the deterministic sentence atomizer,
    /// the default since bajan-5zy), `legacy` (the byte-stable
    /// whole-episode proposer for existing stores), `llm` (the real LLM
    /// extractor, bajan-vg6).
    pub kind: String,
    /// Model id when kind = llm (e.g. `gpt-4o-mini`).
    pub model_id: Option<String>,
    /// OpenAI-compatible chat-completions base URL when kind = llm.
    pub base_url: Option<String>,
    /// NAME of the environment variable carrying the API key when
    /// kind = llm — the key itself is read at call time, never stored
    /// in config or envelopes.
    pub api_key_env: Option<String>,
    /// Model context budget in tokens when kind = llm (chunk sizing:
    /// budget minus prompt overhead). Defaults to
    /// `llm::LLM_DEFAULT_CONTEXT_TOKENS` when None.
    pub context_tokens: Option<usize>,
    /// Retry budget (total attempts per episode per pass) when
    /// kind = llm. Defaults to `llm::LLM_DEFAULT_MAX_ATTEMPTS` when None.
    pub max_attempts: Option<u32>,
    /// Raw BAJAN_MAX_SPEND_USD value when set (bajan-8cc): the spend
    /// cap in USD. REQUIRED for kind = llm — a real-money extractor
    /// never runs without a cost ceiling. Parsed + validated at
    /// selection.
    pub max_spend_usd: Option<String>,
    /// Raw BAJAN_EXTRACTOR_TIMEOUT_SECS value when set (bajan-noj):
    /// the transport's global HTTP timeout in seconds. Burn-in F4 —
    /// reasoning models under load exceed the 120s default (chapter
    /// p50 ~50-90s). Parsed + validated at selection (positive
    /// integer seconds); unset keeps the 120s default.
    pub timeout_secs: Option<String>,
}

/// Machine-readable extractor-selection errors — envelope surface,
/// never panics.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExtractorConfigError {
    /// Unknown BAJAN_EXTRACTOR kind.
    #[error("unknown extractor kind: {kind}")]
    UnknownKind { kind: String },
    /// kind = llm selected but its required parameter is missing.
    #[error("extractor llm requires {param}")]
    MissingParam { param: &'static str },
    /// BAJAN_MAX_SPEND_USD is set but is not a positive finite USD
    /// amount (bajan-8cc).
    #[error("invalid {var} value '{value}': must be a positive finite USD amount")]
    BadSpendCap { var: &'static str, value: String },
    /// BAJAN_EXTRACTOR_TIMEOUT_SECS is set but is not a positive
    /// integer number of seconds (bajan-noj).
    #[error("invalid {var} value '{value}': must be a positive integer number of seconds")]
    BadTimeout { var: &'static str, value: String },
}

impl ExtractorConfig {
    /// Parse the config from an injected environment lookup — pure and
    /// race-free for tests; `from_env` wraps `std::env::var`.
    pub fn from_env_with(lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        ExtractorConfig {
            kind: lookup("BAJAN_EXTRACTOR").unwrap_or_else(|| "atomic".to_string()),
            model_id: lookup("BAJAN_EXTRACTOR_MODEL"),
            base_url: lookup("BAJAN_EXTRACTOR_BASE_URL"),
            api_key_env: lookup("BAJAN_EXTRACTOR_API_KEY_ENV"),
            context_tokens: None,
            max_attempts: None,
            max_spend_usd: lookup("BAJAN_MAX_SPEND_USD"),
            timeout_secs: lookup("BAJAN_EXTRACTOR_TIMEOUT_SECS"),
        }
    }

    /// Parse the config from the process environment.
    pub fn from_env() -> Self {
        Self::from_env_with(&|key| std::env::var(key).ok())
    }

    /// Validate + build the extractor for this configuration.
    pub fn select(&self) -> Result<Box<dyn Extractor>, ExtractorConfigError> {
        match self.kind.as_str() {
            "atomic" => Ok(Box::new(SentenceAtomizer::new(atomic_default_version()))),
            "legacy" => Ok(Box::new(LegacyProposer {
                version: env!("CARGO_PKG_VERSION").to_string(),
            })),
            "llm" => {
                // Validate the parameter surface now so misconfiguration
                // fails at selection, not mid-run.
                let Some(model_id) = self.model_id.clone() else {
                    return Err(ExtractorConfigError::MissingParam {
                        param: "BAJAN_EXTRACTOR_MODEL",
                    });
                };
                let Some(base_url) = self.base_url.clone() else {
                    return Err(ExtractorConfigError::MissingParam {
                        param: "BAJAN_EXTRACTOR_BASE_URL",
                    });
                };
                let Some(api_key_env) = self.api_key_env.clone() else {
                    return Err(ExtractorConfigError::MissingParam {
                        param: "BAJAN_EXTRACTOR_API_KEY_ENV",
                    });
                };
                // Spend cap (bajan-8cc) — REQUIRED: a real-money
                // extractor never runs without a cost ceiling. Parsed
                // and validated here so misconfiguration fails at
                // selection, never mid-run.
                let Some(raw_cap) = self.max_spend_usd.as_deref() else {
                    return Err(ExtractorConfigError::MissingParam {
                        param: "BAJAN_MAX_SPEND_USD",
                    });
                };
                let cap_usd: f64 =
                    raw_cap
                        .trim()
                        .parse()
                        .map_err(|_| ExtractorConfigError::BadSpendCap {
                            var: "BAJAN_MAX_SPEND_USD",
                            value: raw_cap.to_string(),
                        })?;
                if !cap_usd.is_finite() || cap_usd <= 0.0 {
                    return Err(ExtractorConfigError::BadSpendCap {
                        var: "BAJAN_MAX_SPEND_USD",
                        value: raw_cap.to_string(),
                    });
                }
                let cap_micros = crate::llm::usd_to_micros(cap_usd).max(1);
                // Transport timeout (bajan-noj): BAJAN_EXTRACTOR_TIMEOUT_SECS
                // overrides the 120s default; validated here (positive
                // integer seconds) so misconfiguration fails at selection.
                let timeout_secs = match self.timeout_secs.as_deref() {
                    Some(raw) => match raw.trim().parse::<u64>() {
                        Ok(secs) if secs > 0 => secs,
                        _ => {
                            return Err(ExtractorConfigError::BadTimeout {
                                var: "BAJAN_EXTRACTOR_TIMEOUT_SECS",
                                value: raw.to_string(),
                            });
                        }
                    },
                    None => 120,
                };
                Ok(Box::new(
                    crate::llm::LlmExtractor::new(
                        model_id,
                        base_url,
                        api_key_env,
                        self.context_tokens
                            .unwrap_or(crate::llm::LLM_DEFAULT_CONTEXT_TOKENS),
                        self.max_attempts
                            .unwrap_or(crate::llm::LLM_DEFAULT_MAX_ATTEMPTS),
                    )
                    .with_spend_cap_micros(cap_micros)
                    .with_timeout_secs(timeout_secs),
                ))
            }
            other => Err(ExtractorConfigError::UnknownKind {
                kind: other.to_string(),
            }),
        }
    }
}

/// Run the extraction pass with the default extractor — the
/// deterministic sentence atomizer since bajan-5zy — over every
/// persisted episode without extraction output cached at its version.
pub fn run_extract(
    db: &crate::store::sqlite::SqliteStore,
    runs: &mut ExtractionRunStore,
) -> Result<ExtractReport, BajanError> {
    run_extract_with(db, &SentenceAtomizer::new(atomic_default_version()), runs)
}

/// Run the legacy whole-episode proposer at an EXPLICIT version — the
/// version-bump test surface: supersession and cache dynamics across
/// versions are contract behavior independent of which extractor runs
/// (`ex_single_call`, `ex_supersession`). Byte-stable with the
/// pre-bajan-5zy default.
pub fn run_extract_versioned(
    db: &crate::store::sqlite::SqliteStore,
    extractor_version: &str,
    runs: &mut ExtractionRunStore,
) -> Result<ExtractReport, BajanError> {
    run_extract_with(
        db,
        &LegacyProposer {
            version: extractor_version.to_string(),
        },
        runs,
    )
}

/// Run an extractor's pass over every persisted episode without
/// extraction output cached at the extractor's version, recording one
/// run row per call into `runs` (`ex_run_record` — additive telemetry,
/// never cache economics).
///
/// Pipeline-owned responsibilities (the seam contract): the gate
/// (containment + store-side hedge/schema checks), the cache (keyed on
/// (episode id, extractor version)), supersession on version bumps,
/// run-row telemetry with the extractor's model id, and the projection
/// of scope/source-type/data-cutoff/lineage onto each candidate
/// (`gm_schema_v2` stays closed — extractors never fabricate metadata).
///
/// Batch semantics (seam decision, all-or-nothing): one gate failure in
/// an episode's candidate batch refuses the WHOLE episode's batch — no
/// partial staging. A half-proposed episode is neither a claim set nor a
/// rejection; it is a broken extraction.
///
/// Caching policy (decided in bajan-r1h, honest): a gate-refused episode
/// is NOT cached at this version — it stays pending and is retryable at
/// the same version — and its refusal is recorded on the report and the
/// run row with a machine-readable reason. Zero-candidate episodes are
/// legitimately extracted and cached as empty (`ex_typed_gate`);
/// infrastructure failures propagate as errors and are never reported
/// as gate rejections.
///
/// CORR-001 (bajan-15i design note): when a future LLM extractor chunks
/// input, chunking is *assembly for exactly one call* — chunks are
/// concatenated into one multi-part prompt and the output is cached under
/// the single (episode id, extractor version) key. Never one call per
/// chunk (`ex_single_call`, specs/extraction-claims.md).
/// Non-knowledge section kinds refused wholesale by the deterministic
/// post-pass (`ex_section_filter`, bajan-162): converter-emitted
/// `section-kind:` tags whose episodes produce noise, not knowledge —
/// praise/testimonial blurbs, copyright pages, acknowledgments. Fixed in
/// code, never an LLM judgment; a config surface (bajan-bku) may revisit.
pub const SECTION_FILTER_DENY: [&str; 3] = [
    "section-kind:praise",
    "section-kind:copyright",
    "section-kind:acknowledgments",
];

/// Remainder of `collapsed` after a leading list marker, if any
/// (`ex_shape_rejection` rule c). Covers bullets (`-`, `*`, `+`, `•`)
/// and numbered markers (`1.`, `2)`).
fn list_marker_remainder(collapsed: &str) -> Option<&str> {
    for marker in ["-", "*", "+", "•"] {
        if let Some(rest) = collapsed.strip_prefix(marker) {
            return Some(rest.trim_start());
        }
    }
    let digits = collapsed
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit())
        .count();
    if digits > 0 {
        let rest = &collapsed[digits..];
        if let Some(rest) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')) {
            return Some(rest.trim_start());
        }
    }
    None
}

/// The fixed-in-code non-claim shape matcher (`ex_shape_rejection`,
/// bajan-w17, design D1): conservative, deterministic, and explainable —
/// every rule reduces to "this text is not an assertion". Returns the
/// first matching rule in fixed order (interrogative, heading,
/// boilerplate); `None` leaves the candidate untouched. Never an LLM
/// judgment (`ex_no_llm_post` holds).
pub fn non_claim_shape(text: &str, episode_text: &str) -> Option<ShapeRule> {
    let collapsed = crate::store::collapse(text);
    // (a) Interrogative-final: a question is not an assertion. Only the
    // FINAL `?` counts — quoted or mid-sentence questions are prose.
    if collapsed.ends_with('?') {
        return Some(ShapeRule::InterrogativeFinal);
    }
    // (b) Heading/section-title fragment: direct markdown heading text,
    // or a candidate whose text equals a converter-supplied heading
    // anchor in the episode body (`ex_deterministic_structure` markers —
    // consumed deterministically, never re-asserted as claims).
    if collapsed.starts_with('#') {
        return Some(ShapeRule::HeadingFragment);
    }
    if !collapsed.is_empty() {
        for line in episode_text.lines() {
            if let Some(heading) = line.strip_prefix('#')
                && crate::store::collapse(heading) == collapsed
            {
                return Some(ShapeRule::HeadingFragment);
            }
        }
    }
    // (b2) Heading-glued lead: a title phrase opening directly into a
    // sentence with no boundary — a converter heading flattened into the
    // prose stream, so no `#` anchor survives to align against (flat
    // conversion loses the `ex_deterministic_structure` markers). The
    // glued fragment leads with the title, not an assertion.
    if heading_glued_lead(&collapsed) {
        return Some(ShapeRule::HeadingFragment);
    }
    // (c) Navigation/list boilerplate: breadcrumb chains (` > `), menu
    // rows (`|`-separated), bare list markers, or marker-led fragments
    // that assert nothing (no sentence punctuation, at most a handful of
    // words). A numbered or bulleted item with a real sentence survives.
    if collapsed.contains(" > ") || collapsed.matches('|').count() >= 2 {
        return Some(ShapeRule::NavBoilerplate);
    }
    if let Some(rest) = list_marker_remainder(&collapsed) {
        let asserts_nothing = rest.is_empty()
            || (!rest.contains('.')
                && !rest.contains('!')
                && !rest.contains('?')
                && rest.split_whitespace().count() <= 6);
        if asserts_nothing {
            return Some(ShapeRule::NavBoilerplate);
        }
    }
    // (c2) Document-structure navigation: reading-order references
    // ("part 2", "chapters 3 and 4", "the following chapter") are
    // guide-talk about the text, not assertions about the world.
    if is_structure_reference(&collapsed) {
        return Some(ShapeRule::NavBoilerplate);
    }
    None
}

/// Words that end a title-case lead run (`ex_shape_rejection` rule b):
/// articles, prepositions, conjunctions, and sentence-opening pronouns —
/// a glued title never continues past them, and a legitimate sentence
/// almost always does.
const TITLE_RUN_STOPWORDS: [&str; 33] = [
    "the", "a", "an", "and", "or", "of", "in", "on", "for", "to", "with", "by", "we", "i", "you",
    "they", "he", "she", "it", "not", "but", "if", "when", "once", "this", "that", "as", "at",
    "is", "was", "there", "here", "am",
];

/// Reading-order nouns whose numeric reference marks guide-talk
/// (`ex_shape_rejection` rule c).
const STRUCTURE_NOUNS: [&str; 8] = [
    "part",
    "parts",
    "chapter",
    "chapters",
    "section",
    "sections",
    "appendix",
    "appendices",
];

/// Fully-capitalized word (at least two letters) — a title-run member.
fn all_caps_word(word: &str) -> bool {
    let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).collect();
    letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase())
}

/// Capitalized, non-stopword, all-lowercase after the first letter — a
/// title-run member for the title-case lead path.
fn title_word(word: &str) -> bool {
    let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() || TITLE_RUN_STOPWORDS.contains(&word.to_lowercase().as_str()) {
        return false;
    }
    letters[0].is_uppercase() && letters[1..].iter().all(|c| c.is_lowercase())
}

/// After a title run of `run` words: the next word opens a sentence
/// (capitalized) at a clean boundary (no trailing punctuation on the
/// title's last word) — the missing-sentence-boundary signature.
fn run_opens_sentence(words: &[&str], run: usize) -> bool {
    run < words.len()
        && words[run].chars().next().is_some_and(|c| c.is_uppercase())
        && words[run - 1]
            .chars()
            .last()
            .is_some_and(|c| c.is_alphanumeric())
}

/// Heading-glued lead detection (`ex_shape_rejection` rule b): an
/// ALL-CAPS title run (any length) or a two-word title-case phrase,
/// either followed by a sentence-opening word at a clean boundary — or
/// ending the text entirely (a bare title fragment).
fn heading_glued_lead(collapsed: &str) -> bool {
    let words: Vec<&str> = collapsed.split_whitespace().collect();
    let mut run = 0;
    while run < words.len() && all_caps_word(words[run]) {
        run += 1;
    }
    if run >= 2 && (run == words.len() || run_opens_sentence(&words, run)) {
        return true;
    }
    let mut run = 0;
    while run < words.len().min(2) && title_word(words[run]) {
        run += 1;
    }
    run == 2 && (run == words.len() || run_opens_sentence(&words, run))
}

/// Document-structure reference detection (`ex_shape_rejection` rule c):
/// a reading-order noun with a numeric marker ("part 2", "chapters 3"),
/// or a relational chapter mention ("the following chapter").
fn is_structure_reference(collapsed: &str) -> bool {
    let lower = collapsed.to_lowercase();
    if lower.contains("following chapter") || lower.contains("next chapter") {
        return true;
    }
    let words: Vec<&str> = lower.split_whitespace().collect();
    words.windows(2).any(|pair| {
        STRUCTURE_NOUNS.contains(&pair[0])
            && pair[1].chars().next().is_some_and(|c| c.is_ascii_digit())
    })
}

pub fn run_extract_with(
    db: &crate::store::sqlite::SqliteStore,
    extractor: &dyn Extractor,
    runs: &mut ExtractionRunStore,
) -> Result<ExtractReport, BajanError> {
    let extractor_version = extractor.version().to_string();
    let model_id = extractor.model_id().map(|m| m.to_string());
    let pending = db.pending_episodes(&extractor_version)?;
    let mut report = ExtractReport {
        episodes_processed: pending.len(),
        candidates_proposed: 0,
        superseded: 0,
        proposals_staged: 0,
        gate_rejections: Vec::new(),
        parked: Vec::new(),
        spend_cap: None,
    };
    for (episode_index, episode) in pending.iter().enumerate() {
        let started_at = epoch_millis();
        // Supersede first (`ex_supersession`): re-extracting this episode
        // at a new version tombstones its prior-version `staged` claims
        // regardless of the new candidate's gate outcome — the event is
        // the re-extraction, not the candidate's acceptance. Same-version
        // provenance inequality does the prior-version test, so a retry
        // at the same version supersedes nothing.
        let tombstoned =
            db.supersede_prior_versions(&episode.id, &extractor_version, started_at)?;
        report.superseded += tombstoned.len();
        // Propose through the seam. Two failure classes (`ex_run_record`
        // class separation): budget exhaustion PARKS the episode — run
        // row with the machine-readable reason, report entry, no cache
        // entry, the pass moves on (`ex_park_requeue`: the next pass
        // re-attempts with a fresh budget); a non-retryable
        // infrastructure failure propagates — the run fails honestly, the
        // episode stays pending, and the failure is never misreported as
        // a gate rejection or a park.
        let proposed = match extractor.propose(episode) {
            Ok(proposed) => proposed,
            Err(ExtractionFailure::Exhausted { attempts, class }) => {
                let reason = Reason::RepeatedCallFailure { attempts, class };
                report.parked.push(ParkedEpisode {
                    episode_id: episode.id.clone(),
                    reason: reason.clone(),
                });
                runs.record(ExtractionRun {
                    episode_id: episode.id.clone(),
                    extractor_version: extractor_version.clone(),
                    model_id: model_id.clone(),
                    started_at,
                    finished_at: epoch_millis(),
                    finish: Finish::Parked { reason },
                    shape_filtered: Vec::new(),
                });
                continue;
            }
            Err(ExtractionFailure::Infrastructure(message)) => {
                return Err(BajanError::Store(message));
            }
            // Spend cap (bajan-8cc): the pass stops honestly — nothing is
            // parked, the refused episode and everything after it stay
            // pending (a raised cap re-attempts them with the cache
            // intact), and the report carries the machine-readable stop.
            Err(ExtractionFailure::SpendCapExceeded {
                spent_usd,
                cap_usd,
                unaccounted_calls,
            }) => {
                report.episodes_processed = episode_index;
                report.spend_cap = Some(SpendCapStop {
                    spent_usd,
                    cap_usd,
                    unaccounted_calls,
                    episodes_left_pending: pending.len() - episode_index,
                });
                break;
            }
        };
        let lineage = Lineage {
            episode_id: episode.id.clone(),
            extractor_version: extractor_version.clone(),
        };
        // Section-kind filter (`ex_section_filter`, bajan-162): a tagged
        // non-knowledge episode refuses every candidate before the gate —
        // episode-level rejection consistent with the all-or-nothing batch
        // gate — and is CACHED as extracted, unlike gate rejections: the
        // filter is a deterministic function of the episode's own tags, so
        // re-extraction would deterministically re-produce filterable junk
        // and burn a call on it. Reason lands on the run row and the report
        // (`ex_run_record`), never on a claim node.
        if let Some(tag) = episode
            .source
            .tags
            .iter()
            .find(|t| SECTION_FILTER_DENY.contains(&t.as_str()))
        {
            let reason = Reason::SectionFiltered { tag: tag.clone() };
            db.mark_extracted(&episode.id, &extractor_version)?;
            let finished_at = epoch_millis();
            report.gate_rejections.push(GateRejection {
                episode_id: episode.id.clone(),
                reason: reason.clone(),
            });
            runs.record(ExtractionRun {
                episode_id: episode.id.clone(),
                extractor_version: extractor_version.clone(),
                model_id: model_id.clone(),
                started_at,
                finished_at,
                finish: Finish::GateRejected { reason },
                shape_filtered: Vec::new(),
            });
            continue;
        }
        // Shape filter (`ex_shape_rejection`, bajan-w17): candidates whose
        // TEXT matches the fixed-in-code non-claim deny set are refused per
        // candidate — unlike the episode-level section filter — while
        // surviving prose continues to the gate unchanged. Refusals land on
        // the run row (`ex_run_record`), never a claim node. An episode
        // whose every candidate is shape-rejected is CACHED as extracted,
        // like the section filter: the shape pass is a deterministic
        // function of the cached call output, so re-extraction would
        // deterministically re-produce filterable shapes and burn a call on
        // it (`ex_single_call` cache economics; deny-set changes are
        // forward-only — they bind new extractions, never cached output).
        let mut shape_filtered: Vec<ShapeRule> = Vec::new();
        let mut survivors: Vec<CandidateClaim> = Vec::new();
        for claim in proposed {
            match non_claim_shape(&claim.text, &episode.text) {
                Some(rule) => shape_filtered.push(rule),
                None => survivors.push(claim),
            }
        }
        if survivors.is_empty() && !shape_filtered.is_empty() {
            let reason = Reason::ShapeFiltered {
                rule: shape_filtered[0],
            };
            db.mark_extracted(&episode.id, &extractor_version)?;
            let finished_at = epoch_millis();
            report.gate_rejections.push(GateRejection {
                episode_id: episode.id.clone(),
                reason: reason.clone(),
            });
            runs.record(ExtractionRun {
                episode_id: episode.id.clone(),
                extractor_version: extractor_version.clone(),
                model_id: model_id.clone(),
                started_at,
                finished_at,
                finish: Finish::GateRejected { reason },
                shape_filtered,
            });
            continue;
        }
        // Project every candidate into a full claim node — status, scope,
        // source type, and data cutoff come from the episode record, never
        // from the extractor.
        let candidates: Vec<ClaimNode> = survivors
            .into_iter()
            .map(|c| ClaimNode {
                text: c.text,
                valid_at: c.valid_at,
                invalid_at: c.invalid_at,
                data_cutoff: episode.source.data_cutoff.clone(),
                status: crate::store::ClaimStatus::Staged,
                // The slice scope: the episode's own workspace tags
                // projected onto the proposed claim (ex_tag_inheritance
                // refines later; untagged episodes stay unscoped rather
                // than invented).
                scope: episode
                    .source
                    .tags
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "workspace:unscoped".into()),
                source_type: episode.source.source_type.clone(),
                evidence: c.evidence,
            })
            .collect();
        // Typed gate, all-or-nothing: containment per candidate first
        // (`ex_evidence_containment`), then the store-side gate checks
        // (hedge anchors, schema) at persistence. The store stages
        // candidates one INSERT at a time, so all-or-nothing is enforced
        // by pre-validating the whole batch BEFORE the first insert:
        // containment here, hedge-marker survival here (same predicate
        // the store applies at insert). Refusals are classified;
        // infrastructure failures propagate, never counted as refusals.
        let mut batch_reason: Option<Reason> = None;
        for candidate in &candidates {
            if let Err(reason) = check_evidence_containment(&candidate.evidence, &episode.text) {
                batch_reason = Some(reason);
                break;
            }
            if let Evidence::Span { text, .. } = &candidate.evidence {
                let dropped = crate::store::dropped_hedge_markers(text, &episode.text);
                if !dropped.is_empty() {
                    batch_reason = Some(Reason::HedgeMarkerDropped);
                    break;
                }
            }
        }
        let outcome = match batch_reason {
            Some(reason) => Err(reason),
            None => {
                // Pre-validated batch: insert all. A store error here is
                // infrastructure (SQL), not a gate refusal — propagates.
                for candidate in &candidates {
                    db.insert_claim(candidate, std::slice::from_ref(&lineage), &episode.text)?;
                }
                Ok(())
            }
        };
        let finished_at = epoch_millis();
        let finish = match outcome {
            Ok(()) => {
                // ex_mutation_proposal: a new output that conflicts with
                // an existing ACTIVE claim stages an invalidation
                // proposal citing the causing episode; the active claim
                // is not mutated. Before mark_extracted so a store
                // failure leaves the episode pending (retryable), never
                // cached half-done.
                stage_mutation_proposals(
                    db,
                    episode,
                    &candidates,
                    &extractor_version,
                    &mut report,
                )?;
                report.candidates_proposed += candidates.len();
                db.mark_extracted(&episode.id, &extractor_version)?;
                Finish::Succeeded
            }
            Err(reason) => {
                // Decided caching policy: a refused episode is NOT cached
                // at this version — retryable at the same version.
                report.gate_rejections.push(GateRejection {
                    episode_id: episode.id.clone(),
                    reason: reason.clone(),
                });
                Finish::GateRejected { reason }
            }
        };
        runs.record(ExtractionRun {
            episode_id: episode.id.clone(),
            extractor_version: extractor_version.clone(),
            model_id: model_id.clone(),
            started_at,
            finished_at,
            finish,
            shape_filtered,
        });
    }
    Ok(report)
}

/// Stage invalidation proposals for re-extraction conflicts with existing
/// ACTIVE claims (`ex_mutation_proposal`). CONFLICT PREDICATE (bajan-c4p,
/// pinned operationally): a prior-version ACTIVE claim — lineage on this
/// episode, provenance inequality vs the re-extract version, the same
/// prior-version test as `supersede_prior_versions` — conflicts with a
/// new candidate iff BOTH carry `Span` evidence at the SAME locator
/// (same evidence position) and the whitespace-collapsed CLAIM texts
/// DIFFER. The evidence span itself may be identical (it comes from the
/// same episode) — what conflicts is the claim the new output states at
/// that position. `Evidence::Unknown` never conflicts (no position). One
/// proposal per (active claim, causing episode) pair: the store's
/// duplicate guard turns a re-conflict on a later pass into an
/// idempotent skip, never an infrastructure failure. The active claim
/// is never mutated — the proposal waits for the human adopt path.
fn stage_mutation_proposals(
    db: &crate::store::sqlite::SqliteStore,
    episode: &EpisodeRecord,
    candidates: &[ClaimNode],
    extractor_version: &str,
    report: &mut ExtractReport,
) -> Result<(), BajanError> {
    for (claim_key, node, lineage) in db.claims_with_lineage()? {
        let on_episode = lineage.iter().any(|l| l.episode_id == episode.id);
        if !on_episode
            || lineage
                .iter()
                .any(|l| l.extractor_version == extractor_version)
        {
            continue;
        }
        if node.status != crate::store::ClaimStatus::Active {
            continue;
        }
        let Evidence::Span {
            text: _,
            locator: active_locator,
        } = &node.evidence
        else {
            continue;
        };
        for candidate in candidates {
            let Evidence::Span {
                text: _,
                locator: cand_locator,
            } = &candidate.evidence
            else {
                continue;
            };
            if cand_locator != active_locator
                || crate::store::collapse(&candidate.text) == crate::store::collapse(&node.text)
            {
                continue;
            }
            match db.stage_invalidation_proposal(claim_key, &episode.id) {
                Ok(()) => report.proposals_staged += 1,
                // Already proposed by an earlier pass: honest no-op.
                Err(crate::store::StoreError::DuplicateProposal { .. }) => {}
                Err(e) => return Err(e.into()),
            }
            // One proposal per active claim per pass — a second
            // conflicting candidate adds no information.
            break;
        }
    }
    Ok(())
}

/// Typed-gate containment check (`ex_evidence_containment`): a candidate's
/// span evidence is accepted only if it survives whitespace-collapsed
/// containment against the episode text — all whitespace runs become a
/// single space on both sides (exact containment is unimplementable over
/// converter-extracted text). A failing span is rejected with reason
/// `evidence_not_contained`, never repaired. The typed absent marker
/// passes: alignment-failure episodes persist, flagged by the reflection
/// pass — containment does not apply to absence.
pub fn check_evidence_containment(evidence: &Evidence, episode_text: &str) -> Result<(), Reason> {
    match evidence {
        Evidence::Span { text, .. } => {
            let span_c = crate::store::collapse(text);
            let episode_c = crate::store::collapse(episode_text);
            if episode_c.contains(&span_c) {
                Ok(())
            } else {
                Err(Reason::EvidenceNotContained)
            }
        }
        Evidence::Unknown => Ok(()),
    }
}

/// Deterministic reflection-pass flag (`ex_reflection`, extended by the
/// evidence-containment decision): the single output of the pass is flags,
/// so it can never reject, repair, or introduce claims — no second audit
/// mechanism exists by construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "flag", rename_all = "snake_case")]
pub enum ReflectionFlag {
    /// Evidence is the typed absent marker (sentence alignment failed):
    /// actionable later, never a violation, never scheduled for
    /// re-extraction by this pass.
    UnknownEvidenceSpan,
}

/// Flag a candidate's evidence for the deterministic reflection pass.
/// Real spans are never flagged here; the typed absent marker yields
/// exactly one `UnknownEvidenceSpan` flag.
pub fn reflection_flags(evidence: &Evidence) -> Vec<ReflectionFlag> {
    match evidence {
        Evidence::Span { .. } => Vec::new(),
        Evidence::Unknown => vec![ReflectionFlag::UnknownEvidenceSpan],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 5.1 — p_run_record: every extraction call writes exactly one run row
    // with episode id, extractor version, model id when present, timestamps,
    // and finish status.
    proptest! {
        #[test]
        fn every_call_writes_exactly_one_run_row(
            episode_id in "[a-z0-9-]{3,20}",
            extractor_version in "[0-9]+\\.[0-9]+\\.[0-9]+",
            has_model in proptest::bool::ANY,
            started_at in 0u64..1_000_000,
            finished_at in 1_000_000u64..2_000_000,
        ) {
            let mut store = ExtractionRunStore::default();
            assert_eq!(store.rows().count(), 0);

            let run = ExtractionRun {
                episode_id: episode_id.clone(),
                extractor_version: extractor_version.clone(),
                model_id: has_model.then(|| "test-model".to_string()),
                started_at,
                finished_at,
                finish: Finish::Succeeded,
                shape_filtered: Vec::new(),
            };
            store.record(run);

            let mut rows = store.rows();
            let row = rows.next().expect("exactly one run row per call");
            prop_assert!(rows.next().is_none(), "exactly one run row per call");
            prop_assert_eq!(&row.episode_id, &episode_id);
            prop_assert_eq!(&row.extractor_version, &extractor_version);
            prop_assert_eq!(row.model_id.as_deref(), if has_model { Some("test-model") } else { None });
            prop_assert_eq!(row.started_at, started_at);
            prop_assert_eq!(row.finished_at, finished_at);
        }
    }

    // 5.1 — parked episodes and gate rejections carry a machine-readable
    // reason; failure outcomes cannot exist without one (by construction).
    proptest! {
        #[test]
        fn failure_outcomes_carry_machine_readable_reasons(
            episode_id in "[a-z0-9-]{3,20}",
            parked in proptest::bool::ANY,
        ) {
            let reason = Reason::RepeatedCallFailure {
                attempts: 3,
                class: FailureClass::Transport,
            };
            let finish = if parked {
                Finish::Parked { reason: reason.clone() }
            } else {
                Finish::GateRejected { reason: reason.clone() }
            };
            let run = ExtractionRun {
                episode_id,
                extractor_version: "0.1.0".into(),
                model_id: None,
                started_at: 0,
                finished_at: 1,
                finish,
                shape_filtered: Vec::new(),
            };
            // Machine-readable: a stable snake_case code, serialized on the row.
            prop_assert_eq!(reason.code(), "repeated_call_failure");
            let serialized = serde_json::to_value(&run).expect("serialize");
            prop_assert!(serialized.to_string().contains("repeated_call_failure"));
        }
    }

    // 6.1 — p_evidence_containment at the typed gate: a candidate's span
    // evidence must survive whitespace-collapsed containment against the
    // episode text (all whitespace runs → single space, both sides); a
    // failing span is rejected with `evidence_not_contained`, never
    // repaired; the typed absent marker passes the gate (it persists
    // flagged by reflection, per ex_evidence_containment).
    proptest! {
        #[test]
        fn gate_rejects_spans_failing_collapsed_containment(
            episode_head in "[a-f]{3,20}",
            episode_tail in "[a-f]{3,20}",
            span in "[g-z]{5,30}",
            ws_episode in "[ \t\n]{1,4}",
            ws_span in "[ \t\n]{1,4}",
            locator in "[a-z:0-9]{3,15}",
        ) {
            let episode = format!("{episode_head}{ws_episode}{episode_tail}");
            let contained = Evidence::Span {
                text: format!("{episode_head}{ws_span}{episode_tail}"),
                locator: locator.clone(),
            };
            // Same span modulo whitespace runs: contained after collapse.
            prop_assert_eq!(
                check_evidence_containment(&contained, &episode),
                Ok(()),
            );

            // A span that does not locate in the episode is rejected, and
            // rejection is the reason — the span is never repaired.
            let foreign = Evidence::Span { text: span, locator };
            prop_assert_eq!(
                check_evidence_containment(&foreign, &episode),
                Err(Reason::EvidenceNotContained),
            );
        }
    }

    // 6.1 — whole-episode spans (bajan-15i review note): a span equal to
    // the entire episode text — byte-identical or modulo whitespace runs,
    // and with non-ASCII content — passes the gate after collapse; the
    // collapse never mangles accents (EDGE-001: language-neutral default).
    proptest! {
        #[test]
        fn gate_passes_whole_episode_spans(
            head in "[a-záéíóúñüß]{3,20}",
            tail in "[a-záéíóúñüß]{3,20}",
            ws in "[ \t\n]{1,4}",
        ) {
            let episode = format!("{head}{ws}{tail}");

            // Byte-identical whole-episode span: contained.
            let identical = Evidence::Span {
                text: episode.clone(),
                locator: "h:whole".into(),
            };
            prop_assert_eq!(check_evidence_containment(&identical, &episode), Ok(()));

            // Whole-episode span modulo a whitespace run: collapse on both
            // sides makes containment hold — accepted, never repaired.
            let wobbled = Evidence::Span {
                text: format!("{head} {tail}"),
                locator: "h:whole".into(),
            };
            prop_assert_eq!(check_evidence_containment(&wobbled, &episode), Ok(()));
        }
    }

    // 6.1 — the typed absent marker passes the gate: alignment-failure
    // episodes persist. Containment does not apply to absence.
    #[test]
    fn gate_passes_typed_absent_marker_through() {
        assert_eq!(
            check_evidence_containment(&Evidence::Unknown, "any episode text"),
            Ok(()),
        );
    }

    // 6.2 — unknown-span flagging rides the deterministic reflection pass
    // (ex_reflection extended): exactly one flag for an absent marker,
    // none for a real span; the pass returns flags only — it can never
    // reject or repair, so no second audit mechanism exists.
    proptest! {
        #[test]
        fn reflection_flags_unknown_spans_only(
            text in "[a-z ]{5,40}",
            locator in "[a-z:0-9]{3,15}",
        ) {
            let span = Evidence::Span { text, locator };
            prop_assert!(reflection_flags(&span).is_empty(), "contained spans are not flagged");

            let flags = reflection_flags(&Evidence::Unknown);
            prop_assert_eq!(flags.len(), 1);
            prop_assert_eq!(&flags[0], &ReflectionFlag::UnknownEvidenceSpan);
        }
    }

    // bajan-r1h — refusal classification at the wired gate: a store-level
    // gate refusal (hedge markers dropped, gm_hedge_anchor) maps to its
    // machine-readable reason; an infrastructure failure (SQL) is never a
    // gate rejection — it must propagate and never be counted as one.
    #[test]
    fn hedge_marker_drops_classify_as_gate_rejections() {
        let err = crate::store::StoreError::HedgeMarkerDropped {
            markers: vec!["maybe".into()],
            spec: crate::store::SPEC,
        };
        let reason = classify_refusal(&err).expect("a gate refusal");
        assert_eq!(reason.code(), "hedge_marker_dropped");
    }

    #[test]
    fn infrastructure_failures_are_never_gate_rejections() {
        let err = crate::store::StoreError::Sqlite {
            message: "disk I/O error".into(),
            spec: crate::store::SPEC,
        };
        assert!(
            classify_refusal(&err).is_err(),
            "SQL failures propagate as infrastructure errors, not gate rejections"
        );
    }

    // bajan-r1h — the report carries per-episode refusals with
    // machine-readable reasons (`ex_run_record`): the serialized shape
    // exposes the stable snake_case code, never a bare count.
    #[test]
    fn gate_rejections_serialize_machine_readable_reasons() {
        let rejection = GateRejection {
            episode_id: "ep-1".into(),
            reason: Reason::HedgeMarkerDropped,
        };
        let serialized = serde_json::to_value(&rejection).expect("serialize");
        assert_eq!(serialized["episode_id"], "ep-1");
        assert_eq!(serialized["reason"]["code"], "hedge_marker_dropped");
    }

    #[test]
    fn reason_codes_are_machine_readable() {
        assert_eq!(
            Reason::EvidenceNotContained.code(),
            "evidence_not_contained"
        );
    }

    // 5.1 — reasons live on run/rejection records, never on claim nodes:
    // the gm_schema_v2 field list stays closed.
    #[test]
    fn claim_nodes_expose_no_reason_field() {
        let node = crate::store::ClaimNode {
            text: "t".into(),
            valid_at: None,
            invalid_at: None,
            data_cutoff: None,
            status: crate::store::ClaimStatus::Staged,
            scope: "s".into(),
            source_type: "episode".into(),
            evidence: crate::store::Evidence::Unknown,
        };
        let serialized = serde_json::to_value(&node).unwrap();
        assert!(serialized.get("reason").is_none());
        assert!(
            !serialized.as_object().unwrap().contains_key("reason"),
            "claim nodes carry no reason field"
        );
    }

    // --- shape matcher (ex_shape_rejection, bajan-w17) -----------------

    #[test]
    fn interrogative_final_text_matches() {
        assert_eq!(
            non_claim_shape("Is bajan fast?", "Body text."),
            Some(ShapeRule::InterrogativeFinal)
        );
        // Whitespace collapse before the final-? test.
        assert_eq!(
            non_claim_shape("Is it   fast?\n", "Body text."),
            Some(ShapeRule::InterrogativeFinal)
        );
    }

    #[test]
    fn mid_sentence_question_is_prose() {
        assert_eq!(
            non_claim_shape("He asked \"why?\" then answered.", "Body text."),
            None
        );
    }

    #[test]
    fn heading_fragments_match_directly_and_by_alignment() {
        // Direct markdown heading text.
        assert_eq!(
            non_claim_shape("# Installation", "Body text."),
            Some(ShapeRule::HeadingFragment)
        );
        // Aligned to a converter-supplied heading anchor in the episode.
        let body = "# Installation Guide\n\nInstall steps follow here.\n";
        assert_eq!(
            non_claim_shape("Installation Guide", body),
            Some(ShapeRule::HeadingFragment)
        );
        // Same text with no heading anchor to align to stays prose — but
        // only once it asserts something (a bare two-word title is itself
        // a heading fragment per the refined rule set, task 3.2).
        assert_eq!(
            non_claim_shape(
                "The installation guide covers setup steps.",
                "Plain prose only."
            ),
            None
        );
    }

    #[test]
    fn navigation_and_list_boilerplate_matches() {
        assert_eq!(
            non_claim_shape("Home > Docs > API", "Body text."),
            Some(ShapeRule::NavBoilerplate)
        );
        assert_eq!(
            non_claim_shape("Home | Docs | API", "Body text."),
            Some(ShapeRule::NavBoilerplate)
        );
        for bare in ["-", "* ", "1.", "2)", "•", "• Getting Started"] {
            assert_eq!(
                non_claim_shape(bare, "Body text."),
                Some(ShapeRule::NavBoilerplate),
                "bare marker should match: {bare}"
            );
        }
    }

    #[test]
    fn assertion_content_in_list_form_is_prose() {
        // A numbered item carrying a real sentence asserts something —
        // the deny set is conservative and leaves it alone.
        assert_eq!(
            non_claim_shape("1. Bajan caches extraction output for reuse.", "Body text."),
            None
        );
        assert_eq!(
            non_claim_shape("- Bajan runs locally by default.", "Body text."),
            None
        );
    }

    #[test]
    fn plain_prose_matches_nothing() {
        assert_eq!(
            non_claim_shape(
                "Bajan extracts claims from persisted episodes.",
                "Bajan extracts claims from persisted episodes."
            ),
            None
        );
    }

    #[test]
    fn shape_filtered_reason_carries_stable_code_and_rule() {
        let reason = Reason::ShapeFiltered {
            rule: ShapeRule::InterrogativeFinal,
        };
        assert_eq!(reason.code(), "shape_filtered");
        let serialized = serde_json::to_value(&reason).unwrap();
        assert_eq!(serialized["code"], "shape_filtered");
        assert_eq!(serialized["rule"], "interrogative_final");
    }

    // --- gate-refined rules (bajan-w17 task 3.2, bought by the eval gate)

    #[test]
    fn heading_glued_leads_match() {
        // ALL-CAPS title flattened into the prose stream.
        assert_eq!(
            non_claim_shape(
                "UNDERSTANDING THE FOUNDATION Not long ago, two acquaintances ran a business.",
                "Body text."
            ),
            Some(ShapeRule::HeadingFragment)
        );
        // Title-case phrase + sentence-start pronoun at a clean boundary.
        assert_eq!(
            non_claim_shape(
                "Income Statement The income statement shows revenues and expenses.",
                "Body text."
            ),
            Some(ShapeRule::HeadingFragment)
        );
        assert_eq!(
            non_claim_shape(
                "Box Definitions We want to make finance as easy as possible.",
                "Body text."
            ),
            Some(ShapeRule::HeadingFragment)
        );
        // A bare title with nothing after the run.
        assert_eq!(
            non_claim_shape("Income Statement", "Body text."),
            Some(ShapeRule::HeadingFragment)
        );
    }

    #[test]
    fn structure_references_match_as_navigation() {
        assert_eq!(
            non_claim_shape("We explain the income statement in part 2.", "Body text."),
            Some(ShapeRule::NavBoilerplate)
        );
        assert_eq!(
            non_claim_shape("Part 3 takes up the balance sheet.", "Body text."),
            Some(ShapeRule::NavBoilerplate)
        );
        assert_eq!(
            non_claim_shape(
                "We'll consider the three main reports in parts 2, 3, and 4 of the book.",
                "Body text."
            ),
            Some(ShapeRule::NavBoilerplate)
        );
        assert_eq!(
            non_claim_shape(
                "In the following chapter we'll show you some specific examples.",
                "Body text."
            ),
            Some(ShapeRule::NavBoilerplate)
        );
    }

    #[test]
    fn discourse_prose_is_never_shape_filtered() {
        // Gate survivors: conversational glue and teaching meta are not in
        // the deny set — conservative by design (D1); subtler judgment
        // belongs to the reflection pass (`ex_reflection`), not this gate.
        for prose in [
            "Note that word: acquired.",
            "But that's not what we're talking about here.",
            "But how about you? Nobody gave you an exam in finance.",
            "But now is the time.",
            "Let's look at each one in turn.",
            "You can probably see where we're going with this cautionary tale.",
            "Please don't get the idea that we are impugning anybody's integrity.",
            "Fundamentally, financial intelligence boils down to three distinct skill sets.",
            "Their company seemed very successful—in fact, it was doubling its sales every year.",
            "Big companies sometimes throw the word consolidated in front of those phrases.",
        ] {
            assert_eq!(
                non_claim_shape(prose, "Body text."),
                None,
                "discourse prose must survive: {prose}"
            );
        }
    }
}
