//! Purpose: claim-node types for the claim graph — the eight-field claim
//! node schema, lineage provenance, and the hedge/containment helpers.
//! Responsibilities: the frozen node schema (`gm_schema_v2`), migration of
//! pre-v2 nodes, hedge-marker detection (`gm_hedge_anchor`), and the
//! spec-traced store error type.
//! Rationale: governed by specs/graph-model.md; split from the store
//! module so persistence (SQLite), the in-memory store, and node typing
//! are separate concerns (bajan-6hz refactor).
use serde::{Deserialize, Serialize};
use std::fmt;

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
    Span { text: String, locator: String },
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

/// Lineage provenance for a stored claim (`gm_reified`): the episode it
/// derives from and the extractor version that produced it. Kept parallel
/// to the node — the claim-node schema (`gm_schema_v2`) stays closed, and
/// supersession targets claims by (episode, version) provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lineage {
    pub episode_id: String,
    pub extractor_version: String,
}

/// Tombstone record for a superseded claim (`ex_supersession`): the
/// machine-readable reason lives here, never on the claim node (the
/// schema field list stays closed). The tombstoned node keeps its
/// lineage and evidence — a tombstone, not a delete (`gm_lineage_survives`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupersessionRecord {
    pub claim_key: usize,
    pub reason: crate::extract::Reason,
    pub superseded_at: u64,
}

/// An invalidation proposal staged by a candidate claim that conflicts
/// with an existing active claim (`ex_mutation_proposal`): carries the
/// causing-episode lineage; the active claim itself is never mutated by
/// the extractor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvalidationProposal {
    pub claim_key: usize,
    pub causing_episode_id: String,
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

/// Whitespace-collapsed form (the `ex_evidence_containment` normalization:
/// all whitespace runs become a single space). Shared by the typed gate
/// (extract) and span persistence (`gm_hedge_anchor`).
pub fn collapse(text: &str) -> String {
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
    #[error("store operation failed: {message} (see {spec}, gm_embedded_store)")]
    Sqlite { message: String, spec: &'static str },

    #[error(
        "evidence span drops hedge marker(s) {markers:?} present in the supporting episode text \
         (see {spec}, gm_hedge_anchor)"
    )]
    HedgeMarkerDropped {
        markers: Vec<String>,
        spec: &'static str,
    },

    #[error("no claim node keyed {claim_key} in this store")]
    ClaimNotFound { claim_key: usize },

    #[error(
        "claim {claim_key} is in status {current:?}; adopt moves proposed/staged claims only \
         (see {spec}, gm_human_adopt)"
    )]
    AdoptRefused {
        claim_key: usize,
        current: ClaimStatus,
        spec: &'static str,
    },
}
