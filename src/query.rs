//! Purpose: read-side query module — commands over the reified claim graph
//! (claim nodes, lineage edges to episode and entity nodes).
//! Responsibilities: read-only graph projection for CLI consumers.
//! Rationale: governed by specs/graph-model.md; this scaffold ships the
//! module seam only (bajan-2xv), behavior arrives with the gated
//! implementation tickets (bajan-ahs spec pending).

use crate::cli::BajanError;

pub const SPEC: &str = "specs/graph-model.md";

/// Run a read-side query over the claim graph.
///
/// Scaffold: always `NotImplemented`.
pub fn run() -> Result<(), BajanError> {
    Err(BajanError::NotImplemented {
        module: "query",
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
                module: "query",
                ..
            })
        ));
    }
}
