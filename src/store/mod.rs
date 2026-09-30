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

pub mod nodes;
pub mod sqlite;

pub use nodes::{
    ClaimNode, ClaimStatus, EdgeLabel, Evidence, HEDGE_MARKERS, InvalidationProposal, Lineage,
    MigrationReport, StoreError, SupersessionRecord, V1ClaimNode, collapse, dropped_hedge_markers,
    migrate_v1_to_v2,
};

pub const SPEC: &str = "specs/graph-model.md";

/// Audit record for one human accept action (`gm_human_adopt`): exactly
/// one per adopted claim, carrying the operator identity and timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    pub claim_key: usize,
    pub actor: String,
    pub adopted_at: u64,
}

/// The claim-graph store: persists claim nodes, enforcing hedge-marker
/// preservation at span persistence (`gm_hedge_anchor`) and human-only
/// adoption (`gm_human_adopt`).
///
/// Status discipline: `adopt`/`adopt_batch` are the *only* public API that
/// sets `ClaimStatus::Active` — every future automated path (supersession,
/// reflection, re-ingest) must leave `active` unreachable, making the
/// refusal structural rather than a convention.
#[derive(Debug, Default, Clone)]
pub struct ClaimStore {
    nodes: Vec<ClaimNode>,
    lineage: Vec<Lineage>,
    audits: Vec<AuditRecord>,
    supersessions: Vec<SupersessionRecord>,
    invalidations: Vec<InvalidationProposal>,
}

impl ClaimStore {
    /// Persist a claim node with its lineage provenance. The node is
    /// stored under (episode id, extractor version) provenance so a
    /// version-bump re-extract can target it (`ex_supersession`);
    /// `supporting_episode` is the verbatim text of the episode backing
    /// the node's evidence span, and span persistence rejects spans that
    /// drop hedge markers present in it.
    pub fn insert(
        &mut self,
        node: ClaimNode,
        lineage: Lineage,
        supporting_episode: &str,
    ) -> Result<(), StoreError> {
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
        self.lineage.push(lineage);
        Ok(())
    }

    pub fn nodes(&self) -> &[ClaimNode] {
        &self.nodes
    }

    /// Lineage provenance of a stored claim, by claim key (`gm_reified`).
    pub fn lineage_of(&self, claim_key: usize) -> Option<&Lineage> {
        self.lineage.get(claim_key)
    }

    /// Tombstone trail of superseded claims (`ex_supersession`).
    pub fn supersessions(&self) -> &[SupersessionRecord] {
        &self.supersessions
    }

    /// Staged invalidation proposals (`ex_mutation_proposal`).
    pub fn invalidations(&self) -> &[InvalidationProposal] {
        &self.invalidations
    }

    /// Audit trail of human accept actions, one record per adopted claim
    /// (`gm_human_adopt`).
    pub fn audit(&self) -> &[AuditRecord] {
        &self.audits
    }

    /// Supersede on version-bump re-extraction (`ex_supersession`):
    /// tombstones — moves to `rejected` — only this episode's `staged`
    /// claims derived from extractor versions other than the re-extract
    /// version, writing one supersession record per claim with reason
    /// `superseded_by_reextraction`. Lineage and evidence stay intact.
    /// `active` and `rejected` claims (any episode, any version) and other
    /// episodes' claims are never touched; a superseded claim re-enters
    /// `staged` only through an explicit re-stage action. Same-version
    /// provenance is never superseded — re-ingest at the same version
    /// reuses cached output and resurrects nothing. Claims newer than the
    /// re-extract version cannot exist (the pipeline re-extracts in
    /// version order), so provenance inequality is the honest prior-version
    /// test.
    pub fn supersede_prior_versions(
        &mut self,
        episode_id: &str,
        extractor_version: &str,
        superseded_at: u64,
    ) -> Vec<usize> {
        let mut superseded = Vec::new();
        for key in 0..self.nodes.len() {
            let lineage = &self.lineage[key];
            if lineage.episode_id != episode_id || lineage.extractor_version == extractor_version {
                continue;
            }
            if self.nodes[key].status == ClaimStatus::Staged {
                self.nodes[key].status = ClaimStatus::Rejected;
                self.supersessions.push(SupersessionRecord {
                    claim_key: key,
                    reason: crate::extract::Reason::SupersededByReextraction,
                    superseded_at,
                });
                superseded.push(key);
            }
        }
        superseded
    }

    /// Route a candidate's conflict with an existing active claim through
    /// the invalidation-proposal mechanism (`ex_mutation_proposal`): the
    /// proposal carries the causing-episode lineage and the active claim
    /// is not mutated here. Unknown claim keys are refused.
    pub fn stage_invalidation_proposal(
        &mut self,
        claim_key: usize,
        causing_episode_id: &str,
    ) -> Result<(), StoreError> {
        if claim_key >= self.nodes.len() {
            return Err(StoreError::ClaimNotFound { claim_key });
        }
        self.invalidations.push(InvalidationProposal {
            claim_key,
            causing_episode_id: causing_episode_id.to_string(),
        });
        Ok(())
    }

    /// Explicit human accept action: the sole path from `proposed`/`staged`
    /// to `active` (`gm_human_adopt`). Writes exactly one audit record for
    /// the adopted claim. Non-staged claims are refused — `active` and
    /// `rejected` claims are never touched (per-claim reversibility; no
    /// resurrection from `rejected`).
    pub fn adopt(
        &mut self,
        claim_key: usize,
        actor: &str,
        adopted_at: u64,
    ) -> Result<AuditRecord, StoreError> {
        let node = self
            .nodes
            .get_mut(claim_key)
            .ok_or(StoreError::ClaimNotFound { claim_key })?;
        if node.status != ClaimStatus::Staged {
            return Err(StoreError::AdoptRefused {
                claim_key,
                current: node.status,
                spec: SPEC,
            });
        }
        node.status = ClaimStatus::Active;
        let record = AuditRecord {
            claim_key,
            actor: actor.to_string(),
            adopted_at,
        };
        self.audits.push(record.clone());
        Ok(record)
    }

    /// Batch accept of multiple claims in one session (`gm_human_adopt`):
    /// a loop over `adopt`, so each claim is individually audited and
    /// individually reversible. Not transactional — a refusal aborts the
    /// batch with earlier adoptions standing, each already audited.
    pub fn adopt_batch(
        &mut self,
        claim_keys: &[usize],
        actor: &str,
        adopted_at: u64,
    ) -> Result<Vec<AuditRecord>, StoreError> {
        claim_keys
            .iter()
            .map(|&key| self.adopt(key, actor, adopted_at))
            .collect()
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
            assert_eq!(
                node.evidence,
                Evidence::Unknown,
                "backfilled as typed absent"
            );
            assert!(
                node.evidence.is_unknown(),
                "unknown spans are flagged, actionable later"
            );
            assert!(
                !node.evidence.is_violation(),
                "unknown is not a containment violation"
            );
        }
        // Loud: the count is rendered in the report's text form.
        let rendered = report.to_string();
        assert!(
            rendered.contains("3"),
            "count must be reported loudly, got: {rendered}"
        );
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
        let lineage = Lineage {
            episode_id: "ep-a".into(),
            extractor_version: "0.1.0".into(),
        };
        let ok = ClaimNode {
            evidence: Evidence::Span {
                text: episode.into(),
                locator: "page:1".into(),
            },
            ..v2_node()
        };
        store
            .insert(ok, lineage.clone(), episode)
            .expect("hedge-free verbatim span persists");

        let unknown = ClaimNode {
            evidence: Evidence::Unknown,
            ..v2_node()
        };
        store
            .insert(unknown, lineage, episode)
            .expect("typed-absent evidence persists flagged, not rejected");
    }

    fn staged_node() -> ClaimNode {
        ClaimNode {
            status: ClaimStatus::Staged,
            ..v2_node()
        }
    }

    fn test_lineage() -> Lineage {
        Lineage {
            episode_id: "ep-a".into(),
            extractor_version: "0.1.0".into(),
        }
    }

    // 3.1 — p_human_adopt: only explicit human accept actions move a claim
    // to `active`. `ClaimStore::adopt` is the sole Active-setting API; every
    // future automated path (supersession, reflection, re-ingest) mutates
    // status only through non-Active transitions, so the refusal is
    // structural — there is no other public route to Active.
    #[test]
    fn human_adopt_moves_staged_to_active_with_one_audit_record() {
        let mut store = ClaimStore::default();
        store
            .insert(staged_node(), test_lineage(), "episode text")
            .unwrap();

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
        let active = ClaimNode {
            status: ClaimStatus::Active,
            ..staged_node()
        };
        let rejected = ClaimNode {
            status: ClaimStatus::Rejected,
            ..staged_node()
        };
        store.insert(active, test_lineage(), "e").unwrap();
        store.insert(rejected, test_lineage(), "e").unwrap();

        assert!(store.adopt(0, "sasha", 1).is_err(), "active stays active");
        assert!(
            store.adopt(1, "sasha", 1).is_err(),
            "no resurrection from rejected"
        );
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
            for i in 0..n {
                store
                    .insert(
                        staged_node(),
                        Lineage {
                            episode_id: format!("ep-{i}"),
                            extractor_version: "0.1.0".into(),
                        },
                        "e",
                    )
                    .unwrap();
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

    // 4.1 — p_supersession: a version-bump re-extract tombstones ONLY that
    // episode's `staged` claims derived from prior extractor versions;
    // active and rejected claims (any episode, any version) are never
    // mutated; other episodes' claims are never mutated; lineage and
    // evidence survive tombstoning (rejected, never deleted).
    proptest! {
        #[test]
        fn version_bump_supersedes_only_prior_version_staged_claims(
            episode in "[a-b]",
            version_idx in 0usize..2,
            status in 0usize..3,
            superseded_at in 1_000u64..2_000,
        ) {
            let episode_id = format!("ep-{episode}");
            let versions = ["0.1.0", "0.2.0"];
            let extractor_version = versions[version_idx];
            let status = [
                ClaimStatus::Staged,
                ClaimStatus::Active,
                ClaimStatus::Rejected,
            ][status];

            let mut store = ClaimStore::default();
            let node = ClaimNode { status, ..v2_node() };
            let evidence_before = node.evidence.clone();
            store
                .insert(
                    node,
                    Lineage {
                        episode_id: episode_id.clone(),
                        extractor_version: extractor_version.into(),
                    },
                    "episode text",
                )
                .unwrap();

            // Re-extract episode a at the bumped version 0.2.0.
            let superseded =
                store.supersede_prior_versions("ep-a", "0.2.0", superseded_at);

            let is_target = episode_id == "ep-a"
                && extractor_version != "0.2.0"
                && status == ClaimStatus::Staged;
            prop_assert_eq!(
                superseded.len(),
                usize::from(is_target),
                "only prior-version staged claims of the re-extracted episode are superseded"
            );

            // Tombstone, not delete: the node survives with evidence and
            // lineage intact; only its status moved to rejected.
            prop_assert_eq!(store.nodes().len(), 1);
            prop_assert_eq!(store.nodes()[0].evidence.clone(), evidence_before);
            prop_assert_eq!(
                store.lineage_of(0).map(|l| l.extractor_version.as_str()),
                Some(extractor_version)
            );
            let expected_status = if is_target {
                ClaimStatus::Rejected
            } else {
                status
            };
            prop_assert_eq!(store.nodes()[0].status, expected_status);

            // Tombstone reason is machine-readable and only exists for
            // superseded claims — never stored on the claim node.
            prop_assert_eq!(store.supersessions().len(), usize::from(is_target));
            if is_target {
                let record = &store.supersessions()[0];
                prop_assert_eq!(record.claim_key, 0);
                prop_assert_eq!(record.reason.code(), "superseded_by_reextraction");
                prop_assert_eq!(record.superseded_at, superseded_at);
                prop_assert!(
                    serde_json::to_value(&store.nodes()[0])
                        .unwrap()
                        .get("reason")
                        .is_none(),
                    "tombstone reason lives on the record, never on the claim node"
                );
            }
        }
    }

    // 4.1 — no resurrection: a same-version re-ingest supersedes nothing
    // (cache reuse, no new work) and re-running the bump never revives a
    // tombstoned claim; re-entry to staged is explicit re-stage only.
    #[test]
    fn same_version_reingest_and_repeat_bump_resurrect_nothing() {
        let mut store = ClaimStore::default();
        store
            .insert(
                v2_node(),
                Lineage {
                    episode_id: "ep-a".into(),
                    extractor_version: "0.1.0".into(),
                },
                "e",
            )
            .unwrap();
        assert_eq!(store.supersede_prior_versions("ep-a", "0.2.0", 1), vec![0]);
        assert_eq!(store.nodes()[0].status, ClaimStatus::Rejected);

        // Same version as the tombstoned claim's own: no work, no change.
        assert!(
            store
                .supersede_prior_versions("ep-a", "0.1.0", 2)
                .is_empty()
        );
        // Re-running the bump: nothing left to supersede, nothing revived.
        assert!(
            store
                .supersede_prior_versions("ep-a", "0.2.0", 3)
                .is_empty()
        );
        assert_eq!(store.nodes()[0].status, ClaimStatus::Rejected);
        assert_eq!(store.supersessions().len(), 1, "no duplicate tombstones");
    }

    // 4.2 — active-claim conflicts route through the invalidation-proposal
    // mechanism (ex_mutation_proposal): the proposal carries causing-episode
    // lineage; the active claim itself is never mutated by the extractor.
    #[test]
    fn active_conflict_stages_invalidation_proposal_without_mutating_claim() {
        let mut store = ClaimStore::default();
        store
            .insert(
                ClaimNode {
                    status: ClaimStatus::Active,
                    ..v2_node()
                },
                Lineage {
                    episode_id: "ep-old".into(),
                    extractor_version: "0.1.0".into(),
                },
                "e",
            )
            .unwrap();

        store
            .stage_invalidation_proposal(0, "ep-new")
            .expect("conflict stages a proposal");
        assert_eq!(
            store.nodes()[0].status,
            ClaimStatus::Active,
            "claim untouched"
        );
        assert_eq!(store.invalidations().len(), 1);
        assert_eq!(store.invalidations()[0].claim_key, 0);
        assert_eq!(store.invalidations()[0].causing_episode_id, "ep-new");
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
            .insert(node, test_lineage(), episode)
            .expect_err("dropped hedge marker must be rejected");
        assert!(
            err.to_string().contains("may"),
            "error names the dropped marker: {err}"
        );
        assert!(
            err.to_string().contains(SPEC),
            "error carries the governing spec: {err}"
        );
    }
}
