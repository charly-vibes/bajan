//! Purpose: the deterministic entity-resolution pass feeding the HITL
//! review queue (bajan-6j1) — the only producer of merge candidates.
//! Responsibilities: ranked-candidate walk over the persisted claim graph,
//! normalization-based duplicate detection (whitespace-collapse +
//! case-fold equality or containment — deterministic scoring, no LLM, no
//! embedding), and honest enqueue reporting through the store's
//! `enqueue_candidate` guards.
//! Rationale: governed by specs/entity-review.md — `er_queue_entry` makes
//! this pass the only writer of candidate pairs (the queue drains, it
//! never grows by human invention); `gm_deterministic_er` keeps ER
//! deterministic (normalization plus curated alias lists). The walk is
//! budget-bounded with honest exhaustion, mirroring the `qt_bounded_traversal`
//! discipline of the read queries.

use crate::query::{self, BajanQueryError};
use crate::store::sqlite::SqliteStore;

pub use crate::store::EnqueueOutcome;

/// Whitespace-collapsed, case-folded form of a claim text — the
/// deterministic normalization duplicate detection operates on (the same
/// normalization family as `ex_evidence_containment`'s `collapse`).
pub fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Report of one deterministic ER pass over the persisted claim graph:
/// the honest traversal status, the pairs proposed into the review queue,
/// and the count of candidate pairs refused by the store's queue guards
/// (already open / rejected without new evidence).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ErReport {
    /// Typed traversal outcome (`qt_bounded_traversal` discipline).
    pub status: query::BudgetStatus,
    /// Candidate pairs proposed (edge + queue row written).
    pub proposed: usize,
    /// Candidate pairs the store refused (already open, or rejected
    /// without new evidence) — refused honestly, never silently.
    pub refused: usize,
    /// The proposed pairs, as (from_claim, to_claim), in proposal order.
    pub pairs: Vec<(usize, usize)>,
}

/// Run the deterministic ER pass over ranked candidates
/// (`er_queue_entry`): every pair of ranked hits whose normalized texts
/// are content-equivalent (equal) or containment-related proposes a
/// `possible_duplicate_of` edge into the review queue. The pair walk is
/// budget-bounded — exhaustion is reported, never swallowed.
///
/// `from`/`to` orientation: the contained (shorter) text is the suspected
/// duplicate; on exact equality the lower claim key is `from`.
pub fn run_er_pass(db: &SqliteStore, budget: usize, now: u64) -> Result<ErReport, BajanQueryError> {
    // Ranked candidates (`qt_ranking`): the pass walks hits in ranked
    // order — an empty pattern matches every lineage-resolved claim.
    let result = query::search(db, "", budget)?;
    let hits = result.hits;
    let mut proposed = Vec::new();
    let mut refused = 0usize;
    let mut examined = 0usize;
    let mut exhausted = false;

    'outer: for i in 0..hits.len() {
        for j in (i + 1)..hits.len() {
            if examined >= budget {
                exhausted = true;
                break 'outer;
            }
            examined += 1;
            let (a, b) = (&hits[i], &hits[j]);
            let na = normalize(&a.text);
            let nb = normalize(&b.text);
            let (from, to, evidence) = if na == nb {
                // Content-equivalent: deterministic order — lower key
                // first.
                if a.claim_key <= b.claim_key {
                    (a.claim_key, b.claim_key, "normalization-equal")
                } else {
                    (b.claim_key, a.claim_key, "normalization-equal")
                }
            } else if na.contains(&nb) {
                // The contained text is the suspected duplicate.
                (b.claim_key, a.claim_key, "normalization-contains")
            } else if nb.contains(&na) {
                (a.claim_key, b.claim_key, "normalization-contains")
            } else {
                continue;
            };
            // Evidence fingerprint (`er_repropose_guard`): both sides'
            // texts and supporting episodes at queue time — a rejected
            // pair re-enters only when this changes.
            let mut episodes: Vec<String> = a.episodes.clone();
            episodes.extend(b.episodes.clone());
            episodes.sort();
            episodes.dedup();
            let fingerprint = serde_json::json!({
                "texts": [a.text, b.text],
                "episodes": episodes,
            })
            .to_string();
            match db
                .enqueue_candidate(from, to, evidence, &fingerprint, now)
                .map_err(|e| BajanQueryError(e.to_string()))?
            {
                crate::store::EnqueueOutcome::Queued => proposed.push((from, to)),
                _ => refused += 1,
            }
        }
    }

    // Honest exhaustion (qt_bounded_traversal discipline): the budget
    // bounds both the ranked-candidate walk inside `search` AND the pair
    // walk below — either exhausting means the pass did not see every
    // candidate, and a silent `complete` would be a lie.
    let status = if exhausted || result.status == query::BudgetStatus::BudgetExhausted {
        query::BudgetStatus::BudgetExhausted
    } else {
        query::BudgetStatus::Complete
    };
    Ok(ErReport {
        status,
        proposed: proposed.len(),
        refused,
        pairs: proposed,
    })
}
