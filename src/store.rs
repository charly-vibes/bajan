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

    // 2.1 — p_schema_v2: stored claim nodes expose exactly the eight schema
    // fields with types preserved.
    proptest! {
        #[test]
        fn claim_nodes_expose_exactly_the_v2_field_set(n in 0u8..5) {
            let node = v2_node();
            let serialized = serde_json::to_value(&node).expect("serialize");
            let obj = serialized.as_object().expect("claim node is a JSON object");
            let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
            keys.sort_unstable();
            let mut expected = V2_FIELDS;
            expected.sort_unstable();
            prop_assert_eq!(keys, expected, "field set must be exactly v2 (8 fields, no extras)");

            // Types preserved across a roundtrip.
            let round: ClaimNode = serde_json::from_value(serialized).expect("roundtrip");
            prop_assert_eq!(round, node);
            prop_assert_eq!(n, n); // deterministic generator anchor
        }
    }

    #[test]
    fn schema_rejects_extra_missing_and_mistyped_fields() {
        let mut extra = serde_json::to_value(v2_node()).unwrap();
        extra["confidence"] = json!(0.9); // anti-goal: no confidence field
        assert!(serde_json::from_value::<ClaimNode>(extra).is_err(), "extra field must be rejected");

        let mut missing = serde_json::to_value(v2_node()).unwrap();
        missing.as_object_mut().unwrap().remove("evidence");
        assert!(serde_json::from_value::<ClaimNode>(missing).is_err(), "missing evidence must be rejected");

        let mut mistyped = serde_json::to_value(v2_node()).unwrap();
        mistyped["status"] = json!("bogus-status");
        assert!(serde_json::from_value::<ClaimNode>(mistyped).is_err(), "mistyped status must be rejected");
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
        assert!(rendered.contains('3'), "count must be reported loudly, got: {rendered}");
        // No re-extraction scheduling: the report carries no such surface.
        assert_eq!(report.reextraction_scheduled, 0);
    }

    // 2.3 — gm_hedge_anchor: an evidence span must not drop a hedge marker
    // present in the supporting episode text within the span's coverage.
    proptest! {
        #[test]
        fn span_persistence_rejects_dropped_hedge_markers(
            subject in "[a-z ]{3,20}",
            hedge in 0usize..HEDGE_MARKERS.len(),
            tail in "[a-z ]{3,20}",
        ) {
            let hedge = HEDGE_MARKERS[hedge];
            let episode = format!("{subject} {hedge} {tail}");
            let verbatim = episode.clone();

            // A verbatim span drops nothing.
            prop_assert!(dropped_hedge_markers(&verbatim, &episode).is_empty());

            // A span that silently drops the hedge marker is caught.
            let mutilated = format!("{subject}  {tail}");
            let dropped = dropped_hedge_markers(&mutilated, &episode);
            prop_assert!(dropped.iter().any(|m| m == &hedge), "expected {hedge} in {dropped:?}");
        }
    }

    #[test]
    fn span_persistence_hedge_free_spans_and_unknown_pass() {
        let mut store = ClaimStore::default();
        let episode = "Throughput improved by 12 percent.";
        let ok = ClaimNode {
            evidence: Evidence::Span { text: episode.into(), locator: "page:1".into() },
            ..v2_node()
        };
        store.insert(ok).expect("hedge-free verbatim span persists");

        let unknown = ClaimNode { evidence: Evidence::Unknown, ..v2_node() };
        store.insert(unknown).expect("typed-absent evidence persists flagged, not rejected");
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
        let err = store.insert(node).expect_err("dropped hedge marker must be rejected");
        assert!(err.to_string().contains("may"), "error names the dropped marker: {err}");
    }
}
