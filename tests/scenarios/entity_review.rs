//! entity.review scenario tests for properties without honest existing
//! coverage: `p_review_schema` (queue and audit records validate against
//! the published schema with the closed decision vocabulary).
//!
//! Purpose: scenario-scoped contract tests (bajan-16o).
//! Responsibilities: exactly one test per property id below.
//! Rationale: predicate quoted from specs/entity-review.md Properties.

use bajan::store::{ReviewDecision, ReviewStatus};

/// p_review_schema — every queue and audit record validates against the
/// published schema with the closed decision vocabulary: exactly
/// `approved`/`rejected` are representable decisions, and the queue
/// lifecycle is exactly `proposed`/`resolved_approved`/`resolved_rejected`.
#[test]
fn p_review_schema() {
    // The decision vocabulary is closed: round-trips preserve the variants,
    // and nothing outside the set deserializes.
    for (wire, decision) in [
        ("\"approved\"", ReviewDecision::Approved),
        ("\"rejected\"", ReviewDecision::Rejected),
    ] {
        let parsed: ReviewDecision = serde_json::from_str(wire).expect("closed vocab member");
        assert_eq!(parsed, decision);
        assert_eq!(serde_json::to_string(&decision).unwrap(), wire);
    }
    assert!(serde_json::from_str::<ReviewDecision>("\"merged\"").is_err());
    assert!(serde_json::from_str::<ReviewDecision>("\"approved quickly\"").is_err());

    // The queue lifecycle is likewise closed.
    for (wire, status) in [
        ("\"proposed\"", ReviewStatus::Proposed),
        ("\"resolved_approved\"", ReviewStatus::ResolvedApproved),
        ("\"resolved_rejected\"", ReviewStatus::ResolvedRejected),
    ] {
        let parsed: ReviewStatus = serde_json::from_str(wire).expect("closed lifecycle member");
        assert_eq!(parsed, status);
        assert_eq!(serde_json::to_string(&status).unwrap(), wire);
    }
    assert!(serde_json::from_str::<ReviewStatus>("\"resolved\"").is_err());
}
