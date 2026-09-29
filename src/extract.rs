//! Purpose: extraction module — propose candidate claims typed against the
//! published claim schema, with lineage edges to their source episode.
//! Responsibilities: boundary between persisted episodes and proposed
//! claims; claim-schema conformance at the typed gate.
//! Rationale: governed by specs/extraction-claims.md; this scaffold ships
//! the module seam only (bajan-2xv), behavior arrives with the gated
//! implementation tickets.

use crate::cli::BajanError;
use crate::store::Evidence;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use proptest::prelude::*;

pub const SPEC: &str = "specs/extraction-claims.md";

/// Machine-readable reason for a failure outcome (`ex_run_record`).
///
/// Reasons are stable snake_case codes stored on the run or rejection
/// record — never on a claim node (`gm_schema_v2` field list stays
/// closed). Each variant names the invariant that produces it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum Reason {
    /// Episode parked in `rejected` after repeated call failure
    /// (`ex_single_call`).
    RepeatedCallFailure { attempts: u32 },
    /// Candidate rejected at the validation gate because its evidence span
    /// fails whitespace-collapsed containment (`ex_evidence_containment`,
    /// enforced from bajan-0hs.7).
    EvidenceNotContained,
    /// Prior-version `staged` claim tombstoned by a version-bump
    /// re-extraction (`ex_supersession`). Carried on the supersession
    /// record in the claim store — never on the claim node.
    SupersededByReextraction,
}

impl Reason {
    /// Stable machine-readable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            Reason::RepeatedCallFailure { .. } => "repeated_call_failure",
            Reason::EvidenceNotContained => "evidence_not_contained",
            Reason::SupersededByReextraction => "superseded_by_reextraction",
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractionRun {
    pub episode_id: String,
    pub extractor_version: String,
    pub model_id: Option<String>,
    pub started_at: u64,
    pub finished_at: u64,
    pub finish: Finish,
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

/// Run extraction over persisted episodes.
///
/// Scaffold: always `NotImplemented`.
pub fn run() -> Result<(), BajanError> {
    Err(BajanError::NotImplemented {
        module: "extract",
        spec: SPEC,
    })
}

/// Typed-gate containment check (`ex_evidence_containment`): a candidate's
/// span evidence is accepted only if it survives whitespace-collapsed
/// containment against the episode text — all whitespace runs become a
/// single space on both sides (exact containment is unimplementable over
/// converter-extracted text). A failing span is rejected with reason
/// `evidence_not_contained`, never repaired. The typed absent marker
/// passes: alignment-failure episodes persist, flagged by the reflection
/// pass — containment does not apply to absence.
pub fn check_evidence_containment(
    evidence: &Evidence,
    episode_text: &str,
) -> Result<(), Reason> {
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

    #[test]
    fn stub_returns_not_implemented() {
        assert!(matches!(
            run(),
            Err(BajanError::NotImplemented { module: "extract", .. })
        ));
    }

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
            let reason = Reason::RepeatedCallFailure { attempts: 3 };
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
}