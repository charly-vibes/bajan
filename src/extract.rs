//! Purpose: extraction module — propose candidate claims typed against the
//! published claim schema, with lineage edges to their source episode.
//! Responsibilities: boundary between persisted episodes and proposed
//! claims; claim-schema conformance at the typed gate.
//! Rationale: governed by specs/extraction-claims.md; this scaffold ships
//! the module seam only (bajan-2xv), behavior arrives with the gated
//! implementation tickets.

use crate::cli::BajanError;

pub const SPEC: &str = "specs/extraction-claims.md";

/// Run extraction over persisted episodes.
///
/// Scaffold: always `NotImplemented`.
pub fn run() -> Result<(), BajanError> {
    Err(BajanError::NotImplemented {
        module: "extract",
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
            Err(BajanError::NotImplemented { module: "extract", .. })
        ));
    }
}