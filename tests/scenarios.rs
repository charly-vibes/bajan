//! Scenario tests, grouped per capability.
//!
//! Filters used by espectacular property contracts (`ah check --run-tests`)
//! match these module paths: `<cap>::<property id>` (gently precedent:
//! one test per Properties row, named for its id).
//!
//! Purpose: bind every deployed corpus property scenario to an executable
//! test. Responsibilities: the scenario-scoped tests no existing test file
//! covers honestly (the rest are wired to their existing tests in
//! `.espectacular/<spec>/<p-*.toml>`). Rationale: bajan-16o — the corpus
//! deploy requires one scenario-scoped contract test per property.

#[path = "scenarios/extraction_claims.rs"]
mod extraction_claims;

#[path = "scenarios/ingestion_contract.rs"]
mod ingestion_contract;

#[path = "scenarios/entity_review.rs"]
mod entity_review;
