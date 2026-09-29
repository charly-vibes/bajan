//! Purpose: identity-resolution module — resolve claim identity across
//! versions (proposed vs persisted, supersession on re-extraction).
//! Responsibilities: decide which candidate claim corresponds to which
//! persisted claim node across extraction runs.
//! Rationale: governed by specs/extraction-claims.md (proposed≡staged seam,
//! supersession semantics); this scaffold ships the module seam only
//! (bajan-2xv), behavior arrives with the gated implementation tickets.

use crate::cli::BajanError;

pub const SPEC: &str = "specs/extraction-claims.md";

/// Resolve claim identity across versions.
///
/// Scaffold: always `NotImplemented`.
pub fn run() -> Result<(), BajanError> {
    Err(BajanError::NotImplemented {
        module: "resolve",
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
            Err(BajanError::NotImplemented {
                module: "resolve",
                ..
            })
        ));
    }
}
