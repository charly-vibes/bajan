//! Purpose: claim-graph store — claim nodes typed against `gm_schema_v2`
//! (specs/graph-model.md), the v1→v2 evidence backfill migration, and
//! hedge-marker preservation at span persistence (`gm_hedge_anchor`).
//! Responsibilities: the exact eight-field claim-node schema; loud,
//! non-silent migration of pre-v2 nodes; rejection of evidence spans that
//! drop hedge markers present in the supporting episode text.
//! Rationale: governed by specs/graph-model.md (`gm_schema_v2`,
//! `gm_hedge_anchor`); schema changes are explicit revisions, never silent
//! pickups, and migrated unknown spans are flagged for the reflection pass,
//! never treated as violations and never scheduled for re-extraction.

use serde::{Deserialize, Serialize};
use std::fmt;

pub const SPEC: &str = "specs/graph-model.md";

/// Hedge markers anchored by `gm_hedge_anchor` / `p_hedge_anchor`
/// (episode texts containing hedged statements: may, signals, estimates).
/// Matched case-insensitively on word boundaries.
pub const HEDGE_MARKERS: &[&str] = &[
    "may",
    "might",
    "could",
    "possibly",
    "appears",
    "seems",
    "signals",
    "estimates",
    "reportedly",
    "tentative",
    "uncertain",
];

/// Claim lifecycle status (specs/extraction-claims.md model: staged,
/// active, rejected; `staged` is that spec's dual view of `proposed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    Staged,
    Active,
    Rejected,
}

/// Evidence carried by a v2 claim node (`gm_schema_v2`): a verbatim span
/// of the supporting episode text plus its episode locator, or the typed
/// absent marker when sentence alignment failed.
///
/// The absent marker is a tagged unit variant — it can never collide with
/// a real span value, per the ingestion-contract absent-marker convention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Evidence {
    /// Verbatim span + episode locator.
    Span {
        text: String,
        locator: String,
    },
    /// Typed absent marker: sentence alignment failed. Flagged, actionable
    /// later by the reflection pass — never a violation, never auto-repaired.
    Unknown,
}

impl Evidence {
    pub fn is_unknown(&self) -> bool {
        matches!(self, Evidence::Unknown)
    }

    /// Unknown spans are flagged by the reflection pass, not treated as
    /// containment violations (`ex_evidence_containment`).
    pub fn is_violation(&self) -> bool {
        false
    }
}

/// A claim node under `gm_schema_v2` — exactly eight fields, no confidence
/// field and no vector blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimNode {
    pub text: String,
    pub valid_at: Option<String>,
    pub invalid_at: Option<String>,
    pub data_cutoff: Option<String>,
    pub status: ClaimStatus,
    pub scope: String,
    pub source_type: String,
    pub evidence: Evidence,
}

/// A pre-v2 claim node — the seven frozen v1 fields (`gm_schema_v1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V1ClaimNode {
    pub text: String,
    pub valid_at: Option<String>,
    pub invalid_at: Option<String>,
    pub data_cutoff: Option<String>,
    pub status: ClaimStatus,
    pub scope: String,
    pub source_type: String,
}

/// Result of the v1 → v2 migration. The backfill count is carried in data
/// so callers can report it loudly (`to_string` renders it); migration
/// never schedules re-extraction — `reextraction_scheduled` is pinned to
/// zero by test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    pub nodes: Vec<ClaimNode>,
    pub backfilled: usize,
    pub reextraction_scheduled: usize,
}

impl fmt::Display for MigrationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "schema v1→v2 migration: backfilled evidence=unknown on {} claim node(s); \
             unknown spans flagged for reflection, re-extraction NOT scheduled",
            self.backfilled
        )
    }
}

/// Migrate pre-v2 claim nodes to v2: backfill `evidence = unknown` on every
/// node and report the count loudly. No node is dropped or rewritten apart
/// from the evidence backfill; no re-extraction is scheduled.
pub fn migrate_v1_to_v2(nodes: Vec<V1ClaimNode>) -> MigrationReport {
    let backfilled = nodes.len();
    let nodes = nodes
        .into_iter()
        .map(|v1| ClaimNode {
            evidence: Evidence::Unknown,
            text: v1.text,
            valid_at: v1.valid_at,
            invalid_at: v1.invalid_at,
            data_cutoff: v1.data_cutoff,
            status: v1.status,
            scope: v1.scope,
            source_type: v1.source_type,
        })
        .collect();
    MigrationReport {
        nodes,
        backfilled,
        reextraction_scheduled: 0,
    }
}

/// Hedge markers present in `text` (case-insensitive, word-boundary match).
fn hedge_markers_in(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut found = Vec::new();
    for marker in HEDGE_MARKERS {
        let is_present = lower.match_indices(marker).any(|(i, _)| {
            let before = lower[..i].ends_with(|c: char| !c.is_alphanumeric());
            let after = lower[i + marker.len()..].starts_with(|c: char| !c.is_alphanumeric());
            before && after
        });
        if is_present {
            found.push((*marker).to_string());
        }
    }
    found
}

/// Whitespace-collapsed form (the `ex_evidence_containment` normalization).
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Hedge markers present in the episode text within the span's coverage
/// but missing from the span (`gm_hedge_anchor`). A verbatim span drops
/// nothing; a paraphrasing span that does not locate within the episode
/// is checked against the whole episode — every hedge marker the span
/// lacks is reported. Containment enforcement itself (`evidence-not-
/// contained`) arrives with the typed gate (bajan-0hs.7) and supersedes.
pub fn dropped_hedge_markers(span: &str, episode: &str) -> Vec<String> {
    let span_c = collapse(span);
    let episode_c = collapse(episode);

    // A span that locates verbatim (whitespace-collapsed) in the episode
    // cannot have dropped anything within its coverage.
    if episode_c.contains(&span_c) {
        return Vec::new();
    }

    hedge_markers_in(&episode_c)
        .into_iter()
        .filter(|m| !hedge_markers_in(&span_c).contains(m))
        .collect()
}

/// Store-side error: every variant carries the governing spec so errors
/// stay traceable to the invariant they guard.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(
        "evidence span drops hedge marker(s) {markers:?} present in the supporting episode text \
         (see {spec}, gm_hedge_anchor)"
    )]
    HedgeMarkerDropped { markers: Vec<String>, spec: &'static str },
}

/// The claim-graph store: persists claim nodes, enforcing hedge-marker
/// preservation at span persistence (`gm_hedge_anchor`).
#[derive(Debug, Default, Clone)]
pub struct ClaimStore {
    nodes: Vec<ClaimNode>,
}

impl ClaimStore {
    /// Persist a claim node. `supporting_episode` is the verbatim text of
    /// the episode backing the node's evidence span; span persistence
    /// rejects spans that drop hedge markers present in it.
    pub fn insert(&mut self, node: ClaimNode, supporting_episode: &str) -> Result<(), StoreError> {
        if let Evidence::Span { text, .. } = &node.evidence {
            let dropped = dropped_hedge_markers(text, supporting_episode);
            if !dropped.is_empty() {
                return Err(StoreError::HedgeMarkerDropped {
                    markers: dropped,
                    spec: SPEC,
                });
            }
        }
        self.nodes.push(node);
        Ok(())
    }

    pub fn nodes(&self) -> &[ClaimNode] {
        &self.nodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::json;

    /// The published v2 field set: seven v1 fields + `evidence`.
    const V2_FIELDS: [&str; 8] = [
        "text",
        "valid_at",
        "invalid_at",
        "data_cutoff",
        "status",
        "scope",
        "source_type",
        "evidence",
    ];

    fn v2_node() -> ClaimNode {
        ClaimNode {
            text: "The parser may fail on empty input.".into(),
            valid_at: Some("2026-01-01".into()),
            invalid_at: None,
            data_cutoff: Some("2025-12-31".into()),
            status: ClaimStatus::Staged,
            scope: "workspace:dev".into(),
            source_type: "episode".into(),
            evidence: Evidence::Span {
                text: "The parser may fail on empty input.".into(),
                locator: "heading:Errors".into(),
            },
        }
    }

    fn v1_node() -> V1ClaimNode {
        V1ClaimNode {
            text: "Cache hits scale linearly.".into(),
            valid_at: None,
            invalid_at: None,
            data_cutoff: None,
            status: ClaimStatus::Active,
            scope: "workspace:dev".into(),
            source_type: "episode".into(),
        }
    }

    /// Fields that must be present on every stored claim node. The three
    /// dated fields (valid_at, invalid_at, data_cutoff) are Option-valued:
    /// an absent key is semantically the explicit-null (undated) value, so
    /// exactly-eight is enforced by serialization exposing all 8 keys.
    const REQUIRED_FIELDS: [&str; 5] = ["text", "status", "scope", "source_type", "evidence"];

    // 2.1 — p_schema_v2: stored claim nodes expose exactly the eight schema
    // fields with types preserved; extra, missing, and mistyped fields are
    // rejected at the schema boundary.
    proptest! {
        #[test]
        fn claim_nodes_expose_exactly_the_v2_field_set(
            extra_key in "[a-zA-Z_]{1,20}",
            remove_idx in 0usize..REQUIRED_FIELDS.len(),
        ) {
            let node = v2_node();
            let serialized = serde_json::to_value(&node).expect("serialize");
            let obj = serialized.as_object().expect("claim node is a JSON object");
            let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
            keys.sort_unstable();
            let mut expected = V2_FIELDS;
            expected.sort_unstable();
            prop_assert_eq!(keys, expected, "field set must be exactly v2 (8 fields, no extras)");

            // Types preserved across a roundtrip.
            let round: ClaimNode = serde_json::from_value(serialized.clone()).expect("roundtrip");
            prop_assert_eq!(round, node);

            // An extra field is rejected (skip names that collide with real fields).
            prop_assume!(!V2_FIELDS.contains(&extra_key.as_str()));
            let mut extra = serialized.clone();
            extra[&extra_key] = json!(null);
            prop_assert!(
                serde_json::from_value::<ClaimNode>(extra).is_err(),
                "extra field {extra_key} must be rejected"
            );

            // A missing required field is rejected — including missing
            // evidence.
            let mut missing = serialized.clone();
            missing.as_object_mut().unwrap().remove(REQUIRED_FIELDS[remove_idx]);
            prop_assert!(
                serde_json::from_value::<ClaimNode>(missing).is_err(),
                "missing {} must be rejected",
                REQUIRED_FIELDS[remove_idx]
            );

            // An absent dated field is the explicit-null value.
            let mut undated = serialized;
            undated.as_object_mut().unwrap().remove("valid_at");
            let undated: ClaimNode = serde_json::from_value(undated).expect("absent == null");
            prop_assert_eq!(undated.valid_at, None);
        }
    }

    #[test]
    fn schema_rejects_mistyped_fields() {
        let mut mistyped = serde_json::to_value(v2_node()).unwrap();
        mistyped["status"] = json!("bogus-status");
        assert!(
            serde_json::from_value::<ClaimNode>(mistyped).is_err(),
            "mistyped status must be rejected"
        );
    }

    #[test]
    fn evidence_is_span_with_locator_or_typed_absent_marker() {
        let span = serde_json::to_value(Evidence::Span {
            text: "verbatim".into(),
            locator: "page:3".into(),
        })
        .unwrap();
        assert_eq!(span["kind"], "span");

        let absent = serde_json::to_value(Evidence::Unknown).unwrap();
        // Typed absent marker: can never collide with a real span value.
        assert_eq!(absent["kind"], "unknown");
        assert!(absent.get("text").is_none());
    }

    // 2.2 — migration backfills evidence = unknown and reports the count
    // loudly; unknown spans are flagged, actionable later; no node dropped
    // or rewritten; migration does NOT schedule re-extraction.
    #[test]
    fn migration_backfills_unknown_and_reports_count_loudly() {
        let v1_nodes = vec![v1_node(), v1_node(), v1_node()];
        let report = migrate_v1_to_v2(v1_nodes);

        assert_eq!(report.nodes.len(), 3, "no node dropped");
        assert_eq!(report.backfilled, 3);
        for node in &report.nodes {
            assert_eq!(node.evidence, Evidence::Unknown, "backfilled as typed absent");
            assert!(node.evidence.is_unknown(), "unknown spans are flagged, actionable later");
            assert!(!node.evidence.is_violation(), "unknown is not a containment violation");
        }
        // Loud: the count is rendered in the report's text form.
        let rendered = report.to_string();
        assert!(rendered.contains("3"), "count must be reported loudly, got: {rendered}");
        // No re-extraction scheduling: the report carries no such surface.
        assert_eq!(report.reextraction_scheduled, 0);
    }

    // 2.3 — gm_hedge_anchor: an evidence span must not drop a hedge marker
    // present in the supporting episode text within the span's coverage.
    // Subject/tail use the alphabet [a-f]: no hedge marker can occur in
    // them, so any dropped marker is the generated one.
    proptest! {
        #[test]
        fn span_persistence_rejects_dropped_hedge_markers(
            subject in "[a-f]{3,20}",
            hedge in 0usize..HEDGE_MARKERS.len(),
            tail in "[a-f]{3,20}",
        ) {
            let hedge = HEDGE_MARKERS[hedge];
            let episode = format!("{subject} {hedge} {tail}");
            let verbatim = episode.clone();

            // A verbatim span drops nothing.
            prop_assert!(dropped_hedge_markers(&verbatim, &episode).is_empty());

            // A span that silently drops the hedge marker is caught.
            let mutilated = format!("{subject} {tail}");
            let dropped = dropped_hedge_markers(&mutilated, &episode);
            prop_assert!(dropped.iter().any(|m| m == hedge), "expected {hedge} in {dropped:?}");
        }
    }

    #[test]
    fn span_persistence_hedge_free_spans_and_unknown_pass() {
        let mut store = ClaimStore::default();
        let episode = "Throughput improved by 12 percent.";
        let ok = ClaimNode {
            evidence: Evidence::Span {
                text: episode.into(),
                locator: "page:1".into(),
            },
            ..v2_node()
        };
        store
            .insert(ok, episode)
            .expect("hedge-free verbatim span persists");

        let unknown = ClaimNode {
            evidence: Evidence::Unknown,
            ..v2_node()
        };
        store
            .insert(unknown, episode)
            .expect("typed-absent evidence persists flagged, not rejected");
    }

    fn staged_node() -> ClaimNode {
        ClaimNode { status: ClaimStatus::Staged, ..v2_node() }
    }

    // 3.1 — p_human_adopt: only explicit human accept actions move a claim
    // to `active`. `ClaimStore::adopt` is the sole Active-setting API; every
    // future automated path (supersession, reflection, re-ingest) mutates
    // status only through non-Active transitions, so the refusal is
    // structural — there is no other public route to Active.
    #[test]
    fn human_adopt_moves_staged_to_active_with_one_audit_record() {
        let mut store = ClaimStore::default();
        store.insert(staged_node(), "episode text").unwrap();

        let audit = store.adopt(0, "sasha", 1_000).expect("human adopt");
        assert_eq!(audit.claim_key, 0);
        assert_eq!(store.nodes()[0].status, ClaimStatus::Active);
        assert_eq!(store.audit().len(), 1, "one audit record per claim");
        assert_eq!(store.audit()[0].actor, "sasha");
        assert_eq!(store.audit()[0].adopted_at, 1_000);
        assert_eq!(store.audit()[0].claim_key, 0);
        let _ = audit;
    }

    #[test]
    fn adopt_refuses_non_staged_claims_without_audit() {
        let mut store = ClaimStore::default();
        let active = ClaimNode { status: ClaimStatus::Active, ..staged_node() };
        let rejected = ClaimNode { status: ClaimStatus::Rejected, ..staged_node() };
        store.insert(active, "e").unwrap();
        store.insert(rejected, "e").unwrap();

        assert!(store.adopt(0, "sasha", 1).is_err(), "active stays active");
        assert!(store.adopt(1, "sasha", 1).is_err(), "no resurrection from rejected");
        assert_eq!(store.nodes()[0].status, ClaimStatus::Active);
        assert_eq!(store.nodes()[1].status, ClaimStatus::Rejected);
        assert!(store.audit().is_empty(), "refused attempts write no audit");
    }

    // 3.2 — batch accept loops with per-claim audit records: one session,
    // N claims, N records each carrying actor and timestamp.
    proptest! {
        #[test]
        fn batch_adopt_writes_one_audit_record_per_claim(n in 1usize..6) {
            let mut store = ClaimStore::default();
            for _ in 0..n {
                store.insert(staged_node(), "e").unwrap();
            }
            let keys: Vec<usize> = (0..n).collect();
            let audits = store.adopt_batch(&keys, "sasha", 7_000).expect("batch adopt");
            prop_assert_eq!(audits.len(), n);
            prop_assert_eq!(store.audit().len(), n);
            for (i, audit) in store.audit().iter().enumerate() {
                prop_assert_eq!(audit.claim_key, i);
                prop_assert_eq!(&audit.actor, "sasha");
                prop_assert_eq!(audit.adopted_at, 7_000);
                prop_assert_eq!(store.nodes()[i].status, ClaimStatus::Active);
            }
        }
    }

    #[test]
    fn span_persistence_rejects_hedge_dropping_span() {
        let mut store = ClaimStore::default();
        let episode = "The system may fail under load.";
        let node = ClaimNode {
            evidence: Evidence::Span {
                text: "The system fail under load.".into(),
                locator: "page:1".into(),
            },
            ..v2_node()
        };
        let err = store
            .insert(node, episode)
            .expect_err("dropped hedge marker must be rejected");
        assert!(err.to_string().contains("may"), "error names the dropped marker: {err}");
        assert!(err.to_string().contains(SPEC), "error carries the governing spec: {err}");
    }
}
