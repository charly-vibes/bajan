//! Purpose: ingest pipeline module — persist a normalized episode stream
//! verbatim with its immutable source metadata.
//! Responsibilities: boundary for converter submissions; persistence of the
//! episode record exactly as submitted.
//! Rationale: governed by specs/ingestion-contract.md; this scaffold ships
//! the module seam only (bajan-2xv), behavior arrives with the gated
//! implementation tickets.

use crate::cli::BajanError;

pub const SPEC: &str = "specs/ingestion-contract.md";

/// Run the ingest pipeline.
///
/// Scaffold: always `NotImplemented`.
pub fn run() -> Result<(), BajanError> {
    Err(BajanError::NotImplemented {
        module: "ingest",
        spec: SPEC,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_returns_not_implemented() {
        assert!(matches!(
            run(),
            Err(BajanError::NotImplemented { module: "ingest", .. })
        ));
    }
}