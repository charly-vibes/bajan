//! Purpose: the eval harness (specs/eval-claims.md, bajan-2k4) — scores a
//! model extraction run against a versioned golden corpus with
//! deterministic precision/recall and hedge-fidelity reporting.
//! Responsibilities: load and validate golden-corpus fixtures and run
//! manifests (`ev_golden_corpus`), score candidates by
//! normalization-equivalence + whitespace-collapsed span containment
//! (`ev_deterministic_scoring`), publish per-case judgments with the
//! closed reason vocabulary and aggregate metrics with raw counts
//! (`ev_precision_recall`, `ev_hedge_scoring`, `ev_results-schema`),
//! emit unknown metrics as explicit unknowns instead of invented
//! estimates, and produce a reproducible, version-stamped results row
//! for `evals/results.md` (`ev_results_checked_in`).
//! Rationale: these numbers decide the storage-stack question — scoring
//! is a pure function of (corpus, run) with no store access, no wall
//! clock, no randomness, and no LLM judgment anywhere, so identical
//! inputs reproduce identical records and evaluation can never
//! contaminate a real graph (`ev_readonly` by construction).

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::store::{HEDGE_MARKERS, collapse, hedge_markers_in};

// ---------- fixture schemas (the checked-in formats) ----------

/// A versioned golden corpus (`ev_golden_corpus`): episodes with known
/// claim sets — each golden claim carries its source count N, its
/// evidence span within the episode text, and its hedge markers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoldenCorpus {
    pub id: String,
    pub version: String,
    pub episodes: Vec<GoldenEpisode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoldenEpisode {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub golden_claims: Vec<GoldenClaim>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoldenClaim {
    pub text: String,
    /// Source count N (the `p_reified` contract): how many sources are
    /// known to state this claim.
    pub source_count: u32,
    /// The claim's evidence span within the episode text.
    pub evidence_span: String,
    /// Hedge markers adjudicated present in the supporting episode within
    /// this claim's coverage (`gm_hedge_anchor` vocabulary). Absent → no
    /// hedging expected.
    #[serde(default)]
    pub hedges: Vec<String>,
}

/// An extraction run manifest: the candidates a model run emitted, with
/// the run and extractor identities stamped on the results record
/// (`ev_precision_recall` — one record per (corpus version, extractor
/// version) pair).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalRun {
    pub run_id: String,
    pub extractor_version: String,
    pub claims: Vec<RunCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunCandidate {
    pub episode_id: String,
    pub text: String,
    pub evidence_span: String,
}

// ---------- published results record (`ev_results-schema`) ----------

/// Judgment reason for one candidate — the closed published vocabulary,
/// kebab-case in output (`matched`, `content-miss`, `spurious`,
/// `hedge-dropped`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    /// Candidate matches a golden claim: content equivalence + contained
    /// span + all golden hedges preserved.
    Matched,
    /// Candidate text equals a golden claim but the evidence span does
    /// not locate in the episode — never a match
    /// (`p_deterministic_scoring`).
    ContentMiss,
    /// Candidate matches no unmatched golden claim (invented text,
    /// paraphrase, or a duplicate of an already-consumed golden claim).
    Spurious,
    /// Candidate matched golden content but dropped a hedge marker the
    /// golden claim adjudicates as present (`ev_hedge_scoring`) — a
    /// precision failure distinct from a content miss.
    HedgeDropped,
}

/// The kebab-case code of a judgment reason (the published vocabulary).
pub fn reason_code(reason: Reason) -> &'static str {
    match reason {
        Reason::Matched => "matched",
        Reason::ContentMiss => "content-miss",
        Reason::Spurious => "spurious",
        Reason::HedgeDropped => "hedge-dropped",
    }
}

/// One per-candidate judgment (the MUST gate: per-case judgments are
/// always reported, aggregates never replace them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaseJudgment {
    /// Index of the candidate in the run manifest (run order).
    pub candidate: usize,
    pub episode_id: String,
    pub reason: Reason,
    /// Index of the matched golden claim within its episode's golden
    /// list; `None` unless `reason == Matched`.
    pub golden_claim: Option<usize>,
}

/// Raw counts — every published ratio is recomputable from these
/// (`ev_precision_recall`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalCounts {
    pub emitted: usize,
    pub matched: usize,
    pub golden: usize,
    pub golden_unmatched: usize,
    pub content_miss: usize,
    pub spurious: usize,
    pub hedge_dropped: usize,
    /// Golden claims carrying at least one adjudicated hedge marker.
    pub hedge_bearing_golden: usize,
    /// Matched candidates whose golden claim's hedges were all preserved.
    pub preserved_hedge_matches: usize,
}

/// The published results record. A metric whose denominator is zero is
/// `None` (serialized null) AND named in `unknown_metrics` — missing
/// metrics are reported as unknown, never invented (ticket EARS).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalOutcome {
    pub corpus_id: String,
    pub corpus_version: String,
    pub run_id: String,
    pub extractor_version: String,
    pub judgments: Vec<CaseJudgment>,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
    pub hedge_preservation: Option<f64>,
    pub unknown_metrics: Vec<String>,
    pub counts: EvalCounts,
    /// Candidates removed by the deterministic shape post-pass before
    /// scoring (`ex_shape_rejection`, bajan-w17): the harness replays the
    /// post-pass over the recorded call output — the same pass a live
    /// extraction applies after the cached call — so published emission
    /// counts reflect post-pass output. Additive telemetry; `score` stays
    /// pure math over the post-pass candidate set.
    #[serde(default)]
    pub shape_filtered: usize,
    /// Candidates whose `episode_id` matches no corpus episode (bajan-t6z
    /// cross-check): each one silently scores `spurious`, so a run scored
    /// against the wrong corpus yields plausible low numbers — the count
    /// makes the mismatch visible in the published record. The CLI
    /// additionally hard-refuses when this equals `counts.emitted` (and
    /// emitted > 0): a run referencing nothing known is the wrong-corpus
    /// failure mode outright.
    pub unknown_episode_ids: usize,
}

// ---------- loading + validation (`ev_golden_corpus`) ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalError(pub String);

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn parse<T: for<'de> Deserialize<'de>>(json: &str, what: &str) -> Result<T, EvalError> {
    serde_json::from_str(json).map_err(|e| EvalError(format!("invalid {what} fixture: {e}")))
}

fn non_empty(value: &str, field: &str) -> Result<(), EvalError> {
    if value.trim().is_empty() {
        Err(EvalError(format!(
            "invalid fixture: {field} must be non-empty"
        )))
    } else {
        Ok(())
    }
}

/// Load a golden corpus. A corpus without a version is rejected
/// (`p_golden_corpus`); hedge markers must come from the
/// `gm_hedge_anchor` vocabulary so adjudication stays checkable.
pub fn load_corpus(json: &str) -> Result<GoldenCorpus, EvalError> {
    let corpus: GoldenCorpus = parse(json, "corpus")?;
    non_empty(&corpus.id, "corpus id")?;
    non_empty(&corpus.version, "corpus version")?;
    let mut seen = HashSet::new();
    for episode in &corpus.episodes {
        non_empty(&episode.id, "episode id")?;
        if !seen.insert(episode.id.clone()) {
            return Err(EvalError(format!(
                "invalid corpus: duplicate episode id {}",
                episode.id
            )));
        }
        for claim in &episode.golden_claims {
            non_empty(&claim.text, "golden claim text")?;
            non_empty(&claim.evidence_span, "golden claim evidence span")?;
            if claim.source_count == 0 {
                return Err(EvalError(format!(
                    "invalid corpus: golden claim source_count must be >= 1 (episode {})",
                    episode.id
                )));
            }
            for hedge in &claim.hedges {
                let lower = hedge.to_lowercase();
                if !HEDGE_MARKERS.iter().any(|m| m.to_lowercase() == lower) {
                    return Err(EvalError(format!(
                        "invalid corpus: hedge marker '{hedge}' is outside the \
                         gm_hedge_anchor vocabulary"
                    )));
                }
            }
        }
    }
    Ok(corpus)
}

/// Load an extraction run manifest.
pub fn load_run(json: &str) -> Result<EvalRun, EvalError> {
    let run: EvalRun = parse(json, "run")?;
    non_empty(&run.run_id, "run id")?;
    non_empty(&run.extractor_version, "extractor version")?;
    for claim in &run.claims {
        non_empty(&claim.episode_id, "candidate episode_id")?;
        non_empty(&claim.text, "candidate text")?;
        non_empty(&claim.evidence_span, "candidate evidence span")?;
    }
    Ok(run)
}

// ---------- scoring (`ev_deterministic_scoring`) ----------

/// The content-equivalence form used to MATCH candidates to golden
/// claims: whitespace-collapsed lowercase text with hedge-marker words
/// removed. A candidate that adds or drops a hedge still locates its
/// golden claim — the hedge delta is scored separately as
/// `hedge-dropped`/preserved (`ev_hedge_scoring`), while every
/// non-hedge token must match exactly (`p_deterministic_scoring`:
/// paraphrases without content equivalence score as misses).
fn content_form(text: &str) -> String {
    text.split_whitespace()
        .filter(|tok| {
            let bare = tok.trim_matches(|c: char| !c.is_alphanumeric());
            !HEDGE_MARKERS.iter().any(|m| m.eq_ignore_ascii_case(bare))
        })
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Span containment (`ex_evidence_containment` predicate): the span
/// locates in the episode after whitespace collapse.
fn span_contained(span: &str, episode_text: &str) -> bool {
    collapse(episode_text).contains(&collapse(span))
}

/// Hedge preservation for a candidate against its golden claim: every
/// hedge marker adjudicated on the golden claim must be present
/// (case-insensitive, word-boundary) in the candidate's text. A span
/// that locates verbatim in the episode cannot have dropped anything
/// within its coverage, so the check that matters here is text-level
/// fidelity — the store gate (`gm_hedge_anchor`) already polices spans.
fn hedges_preserved(candidate_text: &str, golden_hedges: &[String]) -> bool {
    let present = hedge_markers_in(candidate_text);
    golden_hedges
        .iter()
        .all(|h| present.iter().any(|p| p.eq_ignore_ascii_case(h)))
}

/// Score a run against a corpus. Pure: no store, no clock, no randomness
/// — identical inputs produce identical outputs (`p_deterministic_scoring`).
///
/// Matching is greedy in candidate (run) order against golden claims in
/// fixture order within the candidate's episode: the first unmatched
/// golden claim with normalization-equal text is the match target; a
/// candidate never consumes a golden claim it failed to match
/// (span-violating or hedge-dropping), so later clean candidates can
/// still match it.
pub fn score(corpus: &GoldenCorpus, run: &EvalRun) -> EvalOutcome {
    let mut counts = EvalCounts {
        emitted: run.claims.len(),
        golden: corpus.episodes.iter().map(|e| e.golden_claims.len()).sum(),
        ..EvalCounts::default()
    };
    counts.hedge_bearing_golden = corpus
        .episodes
        .iter()
        .flat_map(|e| e.golden_claims.iter())
        .filter(|c| !c.hedges.is_empty())
        .count();

    let mut judgments = Vec::with_capacity(run.claims.len());
    let mut unknown_episode_ids = 0usize;
    // consumed[episode][i] = whether that golden claim was matched — only
    // a MATCHED candidate consumes its golden claim.
    let mut consumed: Vec<Vec<bool>> = corpus
        .episodes
        .iter()
        .map(|e| vec![false; e.golden_claims.len()])
        .collect();

    for (idx, candidate) in run.claims.iter().enumerate() {
        let candidate_norm = content_form(&candidate.text);
        let ep_pos = corpus
            .episodes
            .iter()
            .position(|e| e.id == candidate.episode_id);
        if ep_pos.is_none() {
            unknown_episode_ids += 1;
        }
        let (contained, target) = match ep_pos {
            None => (false, None),
            Some(pos) => {
                let episode = &corpus.episodes[pos];
                let contained = span_contained(&candidate.evidence_span, &episode.text);
                // First unmatched golden claim (fixture order) with equal
                // content form — hedge tokens ignored on both sides.
                let target = episode
                    .golden_claims
                    .iter()
                    .enumerate()
                    .find(|(i, g)| !consumed[pos][*i] && content_form(&g.text) == candidate_norm)
                    .map(|(i, _)| i);
                (contained, target)
            }
        };

        let reason = match (ep_pos, target) {
            (None, _) | (_, None) => Reason::Spurious,
            (Some(_), Some(_)) if !contained => Reason::ContentMiss,
            (Some(pos), Some(gi)) => {
                let golden = &corpus.episodes[pos].golden_claims[gi];
                if hedges_preserved(&candidate.text, &golden.hedges) {
                    consumed[pos][gi] = true;
                    if !golden.hedges.is_empty() {
                        counts.preserved_hedge_matches += 1;
                    }
                    counts.matched += 1;
                    Reason::Matched
                } else {
                    Reason::HedgeDropped
                }
            }
        };
        judgments.push(CaseJudgment {
            candidate: idx,
            episode_id: candidate.episode_id.clone(),
            reason,
            golden_claim: (reason == Reason::Matched)
                .then(|| target.expect("Matched implies a golden target was found")),
        });
        match reason {
            Reason::Matched => {}
            Reason::ContentMiss => counts.content_miss += 1,
            Reason::Spurious => counts.spurious += 1,
            Reason::HedgeDropped => counts.hedge_dropped += 1,
        }
    }

    counts.golden_unmatched = counts.golden - counts.matched;

    // Metrics: a zero denominator makes the metric unknown — null in the
    // record AND an explicit key in unknown_metrics (never 0/0, never an
    // invented estimate).
    let mut unknown_metrics = Vec::new();
    let precision = if counts.emitted > 0 {
        Some(counts.matched as f64 / counts.emitted as f64)
    } else {
        unknown_metrics.push("precision".into());
        None
    };
    let recall = if counts.golden > 0 {
        Some(counts.matched as f64 / counts.golden as f64)
    } else {
        unknown_metrics.push("recall".into());
        None
    };
    let hedge_preservation = if counts.hedge_bearing_golden > 0 {
        Some(counts.preserved_hedge_matches as f64 / counts.hedge_bearing_golden as f64)
    } else {
        unknown_metrics.push("hedge_preservation".into());
        None
    };

    EvalOutcome {
        corpus_id: corpus.id.clone(),
        corpus_version: corpus.version.clone(),
        run_id: run.run_id.clone(),
        shape_filtered: 0,
        extractor_version: run.extractor_version.clone(),
        judgments,
        precision,
        recall,
        hedge_preservation,
        unknown_metrics,
        counts,
        unknown_episode_ids,
    }
}

// ---------- results row (`ev_results_checked_in`) ----------

/// The `evals/results.md` table row for this outcome: version-stamped,
/// deterministic, no wall clock — rerunning at identical inputs
/// reproduces the row byte-for-byte (`p_results_checked_in`); the date
/// cell is filled by whoever appends the row.
pub fn results_row(outcome: &EvalOutcome) -> String {
    let c = &outcome.counts;
    let metric = |m: Option<f64>| match m {
        Some(v) => format!("{v:.3}"),
        None => "unknown".to_string(),
    };
    format!(
        "| (date) | {}@v{} | {} | {} | {} | {} | emitted={} matched={} golden={} \
         content-miss={} spurious={} hedge-dropped={} |",
        outcome.corpus_id,
        outcome.corpus_version,
        outcome.extractor_version,
        metric(outcome.precision),
        metric(outcome.recall),
        metric(outcome.hedge_preservation),
        c.emitted,
        c.matched,
        c.golden,
        c.content_miss,
        c.spurious,
        c.hedge_dropped,
    )
}

/// Replay the deterministic shape post-pass (`ex_shape_rejection`,
/// bajan-w17) over a recorded run manifest: candidates whose text matches
/// the fixed-in-code non-claim deny set are removed exactly as a live
/// extraction removes them after the cached call (`ex_single_call` —
/// deny-set changes are forward-only, never a re-extraction). Heading
/// alignment consumes the corpus episode text: heading anchors are
/// converter-supplied structure (`ex_deterministic_structure`). Episodes
/// absent from the corpus align nothing — the candidate is still judged
/// on its own text (interrogative-final, direct heading, boilerplate).
pub fn apply_shape_post_pass(corpus: &GoldenCorpus, run: &EvalRun) -> EvalRun {
    let episode_text = |episode_id: &str| {
        corpus
            .episodes
            .iter()
            .find(|e| e.id == episode_id)
            .map(|e| e.text.as_str())
            .unwrap_or_default()
    };
    let claims = run
        .claims
        .iter()
        .filter(|candidate| {
            crate::extract::non_claim_shape(&candidate.text, episode_text(&candidate.episode_id))
                .is_none()
        })
        .cloned()
        .collect();
    EvalRun {
        run_id: run.run_id.clone(),
        extractor_version: run.extractor_version.clone(),
        claims,
    }
}
