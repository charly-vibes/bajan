//! Purpose: read-side query module — the first read command over the
//! persisted claim graph: a bounded, lineage-traceable search.
//! Responsibilities: honest-stopping traversal over stored rows; lineage
//! traceability filtering; read-only discipline.
//! Rationale: governed by specs/query-tools.md (`qt_bounded_traversal`,
//! `qt_lineage_traceable`, `qt_readonly`, `qt_query-schema`): every hit
//! resolves through lineage to a persisted episode, the traversal runs
//! under a budget with a typed exhausted marker and honest stopping
//! (`complete` vs `budget-exhausted`, never a silent partial answer), and
//! reads never mutate the graph. Ranking (`qt_ranking`), scope filtering
//! (`qt_scope_filtering`), and the contradiction query arrive with their
//! own tickets — this module is the first thin read path (bajan-6hz).

use crate::store::ClaimStatus;
use crate::store::sqlite::SqliteStore;
use serde::Serialize;

pub const SPEC: &str = "specs/query-tools.md";

/// Typed budget status (`qt_bounded_traversal`, `qt_query-schema`
/// vocabulary): `complete` when the traversal finished within budget —
/// an empty answer then means honest not-found; `budget-exhausted` when
/// the walk stopped early with claims remaining — never a silent partial
/// answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BudgetStatus {
    Complete,
    BudgetExhausted,
}

/// One search hit (`qt_query-schema`): the claim, its stored status
/// verbatim (`qt_readonly` — nothing is dropped from results), and the
/// persisted episodes its lineage resolves to (`qt_lineage_traceable`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hit {
    pub claim_key: usize,
    pub text: String,
    pub status: ClaimStatus,
    pub scope: String,
    pub source_type: String,
    pub data_cutoff: Option<String>,
    /// Persisted episodes reachable through the claim's lineage edges.
    pub episodes: Vec<String>,
}

/// A search answer: the honest stopping status plus the hits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchResult {
    /// Typed traversal outcome (`qt_bounded_traversal`).
    pub status: BudgetStatus,
    pub hits: Vec<Hit>,
}

/// Search the persisted claim graph: every claim whose text contains
/// `pattern` (case-insensitive) **and** whose lineage resolves, at query
/// time, to at least one persisted episode (`qt_lineage_traceable` —
/// lineage-broken claims are invisible). The walk visits claim nodes in
/// key order under `budget` (`qt_bounded_traversal`): when the budget
/// stops the walk with claims remaining the answer is `budget-exhausted`;
/// a completed walk reports `complete`. Reads never write (`qt_readonly`).
pub fn search(
    db: &SqliteStore,
    pattern: &str,
    budget: usize,
) -> Result<SearchResult, BajanQueryError> {
    let mut persisted = db.episode_ids();
    persisted.sort();
    let needle = pattern.to_lowercase();
    let mut hits = Vec::new();
    let mut truncated = false;

    for (walked, (claim_key, node, lineage)) in db.claims_with_lineage().into_iter().enumerate() {
        if walked >= budget {
            truncated = true;
            break;
        }
        // qt_lineage_traceable: at least one lineage edge must resolve to
        // a persisted episode at query time.
        let resolves = lineage
            .iter()
            .any(|edge| persisted.binary_search(&edge.episode_id).is_ok());
        if !resolves {
            continue;
        }
        if node.text.to_lowercase().contains(&needle) {
            let episodes: Vec<String> = lineage
                .iter()
                .filter(|edge| persisted.binary_search(&edge.episode_id).is_ok())
                .map(|edge| edge.episode_id.clone())
                .collect();
            hits.push(Hit {
                claim_key,
                text: node.text.clone(),
                status: node.status,
                scope: node.scope.clone(),
                source_type: node.source_type.clone(),
                data_cutoff: node.data_cutoff.clone(),
                episodes,
            });
        }
    }

    let status = if truncated {
        BudgetStatus::BudgetExhausted
    } else {
        BudgetStatus::Complete
    };
    Ok(SearchResult { status, hits })
}

/// Query-surface error: the search itself is deterministic, so the only
/// failure is a broken store. Kept as a distinct type so the read path
/// cannot be confused with write-path errors.
#[derive(Debug, thiserror::Error)]
#[error("query failed: {0}")]
pub struct BajanQueryError(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    // qt_query-schema: the budget-status vocabulary is exactly the two
    // published variants — complete and budget-exhausted.
    #[test]
    fn budget_status_vocabulary_is_closed() {
        assert_eq!(
            serde_json::to_value(BudgetStatus::Complete).unwrap(),
            serde_json::json!("complete")
        );
        assert_eq!(
            serde_json::to_value(BudgetStatus::BudgetExhausted).unwrap(),
            serde_json::json!("budget-exhausted")
        );
    }
}
