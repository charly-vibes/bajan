//! Purpose: identity-resolution module — resolve claim identity across
//! versions (episode re-extractions, superseded claims, explicit
//! re-stage).
//! Responsibilities: the identity report — every persisted claim's
//! lineage, status, supersession tombstones, and staged invalidation
//! proposals joined into one read — and the explicit re-stage action,
//! the only path from a superseded claim back to `staged`.
//! Rationale: governed by specs/extraction-claims.md (`ex_supersession`,
//! `ex_mutation_proposal`); supersession on a version-bump re-extraction
//! is automated, but recovery never is — a tombstoned claim re-enters
//! `staged` only through an explicit re-stage action with actor
//! identity. The report is a read command (qt_readonly): structure only,
//! never adjudication; entity dedup is out of scope (entity.review HITL).

use crate::cli::BajanError;
use crate::store::sqlite::SqliteStore;
use crate::store::{ClaimStatus, InvalidationProposal, Lineage, RestageRecord, SupersessionRecord};
use serde::Serialize;

pub const SPEC: &str = "specs/extraction-claims.md";

/// One claim's identity, resolved across versions: the node's status and
/// lineage provenance, any supersession tombstones written against it
/// (`ex_supersession` — a tombstoned claim keeps lineage and evidence),
/// and any invalidation proposals staged against it
/// (`ex_mutation_proposal`). The tombstone list is history, not state: a
/// restaged claim keeps its tombstones and reports `staged` again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdentityRecord {
    pub claim_key: usize,
    pub text: String,
    pub status: ClaimStatus,
    pub lineage: Vec<Lineage>,
    pub supersessions: Vec<SupersessionRecord>,
    pub invalidations: Vec<InvalidationProposal>,
}

/// The identity report over the persisted claim graph: one record per
/// persisted claim in key order, plus the count of currently tombstoned
/// claims. Read-only (`qt_readonly`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolutionReport {
    pub claims: Vec<IdentityRecord>,
    pub superseded: usize,
}

/// Resolve claim identity across versions: join every persisted claim
/// with its lineage, supersession tombstones, and staged invalidation
/// proposals. Read-only — never mutates a claim, tombstone, or proposal.
pub fn resolve(db: &SqliteStore) -> Result<ResolutionReport, BajanError> {
    let tombstones = db.supersessions()?;
    let proposals = db.invalidations()?;
    let mut claims = Vec::new();
    for (claim_key, node, lineage) in db.claims_with_lineage()? {
        claims.push(IdentityRecord {
            claim_key,
            text: node.text,
            status: node.status,
            lineage,
            supersessions: tombstones
                .iter()
                .filter(|r| r.claim_key == claim_key)
                .cloned()
                .collect(),
            invalidations: proposals
                .iter()
                .filter(|p| p.claim_key == claim_key)
                .cloned()
                .collect(),
        });
    }
    let superseded = claims
        .iter()
        .filter(|c| c.status == ClaimStatus::Rejected && !c.supersessions.is_empty())
        .count();
    Ok(ResolutionReport { claims, superseded })
}

/// Explicit re-stage (`ex_supersession`): move a superseded claim back
/// to `staged`. The store refuses anything that is not a tombstoned
/// rejected claim — staged, active, and gate-refused claims are never
/// touched, and no automated path can re-stage. Returns the written
/// restage record (actor identity and timestamp).
pub fn restage(
    db: &SqliteStore,
    claim_key: usize,
    actor: &str,
    restaged_at: u64,
) -> Result<RestageRecord, BajanError> {
    db.restage(claim_key, actor, restaged_at)?;
    Ok(RestageRecord {
        claim_key,
        actor: actor.to_string(),
        restaged_at,
    })
}
