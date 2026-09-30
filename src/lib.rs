//! Purpose: library root for bajan — exposes the pipeline module seams.
//! Responsibilities: declare the ingest/extract/resolve/query seams and the
//! CLI layer so both the binary and unit tests share one implementation.
//! Rationale: keeping dispatch in the library (not the binary) makes the
//! envelope contract unit-testable without spawning the binary.

pub mod cli;
pub mod contradict;
pub mod extract;
pub mod ingest;
pub mod llm;
pub mod query;
pub mod resolve;
pub mod review;
pub mod store;
