//! Purpose: the deterministic contradiction-scan pass (bajan-3hg) — the
//! first in-pipeline producer of `contradicts` edges.
//! Responsibilities: a budget-bounded pass over ranked claims that
//! proposes `contradicts` edges for pairs whose normalized texts differ
//! only inside a closed edit class — a negation marker present on exactly
//! one side, or a single differing numeral token — and writes each edge
//! with pass identity + detection rule provenance through
//! `insert_edge_provenanced`; reporting mirrors `ErReport` (honest
//! traversal status, proposed, refused, pairs).
//! Rationale: governed by `gm_contradicts_provenance` — only this pass
//! writes `contradicts` edges in-pipeline, every edge carries its
//! producer, and the detection scope is CLOSED (no fuzzy subject
//! detection, per the bajan-3hg issue-review note): a candidate pair must
//! be identical outside the edit class by construction, via sound
//! grouping on the negation-stripped and numeral-masked forms
//! (bajan-9pm pre-filtering discipline). Contradiction edges are graph
//! structure only (`qt_contradiction_query` never adjudicates); the HITL
//! question for contradicts pairs is a deliberate follow-up.

use crate::query::{self, BajanQueryError};
use crate::review::normalize;
use crate::store::EdgeLabel;
use crate::store::sqlite::SqliteStore;

/// The pass identity recorded on every edge this pass writes
/// (`gm_contradicts_provenance`).
pub const PASS_ID: &str = "contradiction-scan";

/// The closed negation-marker vocabulary: a text token that negates its
/// statement. Closed on purpose — a new marker is a spec change, not a
/// code tweak.
const NEGATION_TOKENS: [&str; 3] = ["not", "no", "never"];

/// The numeral mask used for the numeric edit class: identical outside
/// the masked positions by construction.
const NUMERAL_MASK: &str = "\u{0}num\u{0}";

/// Rule names recorded in edge provenance.
const RULE_NEGATION: &str = "negation-marker";
const RULE_NUMERIC: &str = "numeric-mismatch";

/// A token that is a plain numeral: ASCII digits, optionally with a
/// single fractional part (`30`, `30.5`) — no signs, no exponents, no
/// separators: the edit class is closed, not a general number parser.
fn is_numeral(token: &str) -> bool {
    let (int_part, frac) = match token.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (token, None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(int_part) {
        return false;
    }
    frac.is_none_or(digits)
}

/// The negation-stripped form: normalized tokens with every negation
/// marker removed. Sound grouping key for the negation class — a
/// negation pair must share this form by construction.
fn stripped_form(tokens: &[&str]) -> String {
    tokens
        .iter()
        .filter(|t| !NEGATION_TOKENS.contains(t))
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether the text carries at least one negation marker.
fn has_negation(tokens: &[&str]) -> bool {
    tokens.iter().any(|t| NEGATION_TOKENS.contains(t))
}

/// The numeral-masked form: every numeral token replaced by one mask
/// token. Sound grouping key for the numeric class — a numeric-mismatch
/// pair must share this form by construction.
fn masked_form(tokens: &[&str]) -> String {
    tokens
        .iter()
        .map(|t| if is_numeral(t) { NUMERAL_MASK } else { t })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Report of one contradiction-scan pass, mirroring `ErReport`: the
/// honest traversal status, the `contradicts` pairs proposed (each
/// written with pass identity + rule provenance), and the pairs the
/// store refused as duplicate edges (a re-run over an already-scanned
/// graph refuses rather than doubles).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ContradictionReport {
    /// Typed traversal outcome (`qt_bounded_traversal` discipline).
    pub status: query::BudgetStatus,
    /// Contradicts edges proposed (edge written with provenance).
    pub proposed: usize,
    /// Pairs the store refused (`DuplicateEdge` — already scanned).
    pub refused: usize,
    /// The proposed pairs, as (from_claim, to_claim) with the lower
    /// claim key first, in proposal order.
    pub pairs: Vec<(usize, usize)>,
}

/// Run the deterministic contradiction-scan pass over ranked claims:
/// candidate pairs come from two sound groupings over the normalized
/// texts, so every candidate is identical to its partner outside one
/// closed edit class by construction:
///
/// 1. Negation class: pairs inside a negation-stripped group where
///    exactly one side carries a negation marker (`X is Y` vs
///    `X is not Y`, `X` vs `no X`). Both sides negated (or neither) is
///    the same claim or a different claim — never proposed.
/// 2. Numeric class: pairs inside a numeral-masked group differing in
///    exactly one token position where both tokens are numerals (same
///    subject, same unit, different number). Two differing numerals or
///    any non-numeral difference is outside the closed class.
///
/// The budget bounds candidate examinations (grouped pairs, bajan-9pm
/// discipline: non-candidates are proven non-matches by their group and
/// never examined); `search`'s own walk shares the budget. Re-runs hit
/// the store's `DuplicateEdge` guard and are reported as `refused`,
/// never doubled.
pub fn run_contradiction_pass(
    db: &SqliteStore,
    budget: usize,
    now: u64,
) -> Result<ContradictionReport, BajanQueryError> {
    // Ranked candidates (`qt_ranking`): the pass walks hits in ranked
    // order — an empty pattern matches every lineage-resolved claim.
    let result = query::search(db, "", budget)?;
    let hits = result.hits;
    // Normalized texts owned once per hit (bajan-9pm); token sequences
    // borrow them for the rest of the pass.
    let norms: Vec<String> = hits.iter().map(|h| normalize(&h.text)).collect();
    let token_lists: Vec<Vec<&str>> = norms
        .iter()
        .map(|n| n.split_whitespace().collect())
        .collect();

    let mut proposed = Vec::new();
    let mut refused = 0usize;
    let mut examined = 0usize;
    let mut exhausted = false;

    // One edge write per proposed pair: lower claim key first (the
    // orientation the contradiction query treats symmetrically), provenance
    // carrying pass identity + rule (`gm_contradicts_provenance`).
    let propose = |i: usize,
                   j: usize,
                   rule: &str,
                   proposed: &mut Vec<(usize, usize)>,
                   refused: &mut usize|
     -> Result<(), BajanQueryError> {
        let (from, to) = if hits[i].claim_key <= hits[j].claim_key {
            (hits[i].claim_key, hits[j].claim_key)
        } else {
            (hits[j].claim_key, hits[i].claim_key)
        };
        let provenance = serde_json::json!({
            "pass": PASS_ID,
            "version": env!("CARGO_PKG_VERSION"),
            "rule": rule,
            "at": now,
        })
        .to_string();
        match db.insert_edge_provenanced(from, EdgeLabel::Contradicts, to, Some(&provenance)) {
            Ok(()) => proposed.push((from, to)),
            Err(crate::store::StoreError::DuplicateEdge { .. }) => *refused += 1,
            Err(e) => return Err(BajanQueryError(e.to_string())),
        }
        Ok(())
    };

    // Groups ordered by first member (ranked order), pairs within a group
    // by ranked position — the same deterministic order as the ER pass.
    let grouped = |key: &dyn Fn(usize) -> String| -> Vec<(usize, Vec<usize>)> {
        let mut by_key: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();
        for (idx, _) in token_lists.iter().enumerate() {
            by_key.entry(key(idx)).or_default().push(idx);
        }
        let mut groups: Vec<(usize, Vec<usize>)> = by_key
            .into_values()
            .filter(|g| g.len() > 1)
            .map(|g| (*g.first().expect("non-empty"), g))
            .collect();
        groups.sort_by_key(|(first, _)| *first);
        groups
    };

    // --- negation class: stripped-form groups, cross pairs neg/pos ---
    {
        let has_neg: Vec<bool> = token_lists.iter().map(|t| has_negation(t)).collect();
        let groups = grouped(&|idx| stripped_form(&token_lists[idx]));
        'negation: for (_, members) in &groups {
            let neg: Vec<usize> = members.iter().copied().filter(|i| has_neg[*i]).collect();
            let pos: Vec<usize> = members.iter().copied().filter(|i| !has_neg[*i]).collect();
            for &i in &neg {
                for &j in &pos {
                    if examined >= budget {
                        exhausted = true;
                        break 'negation;
                    }
                    examined += 1;
                    propose(i, j, RULE_NEGATION, &mut proposed, &mut refused)?;
                }
            }
        }
    }

    // --- numeric class: masked-form groups, single differing numeral ---
    {
        let groups = grouped(&|idx| masked_form(&token_lists[idx]));
        'numeric: for (_, members) in &groups {
            for a in 0..members.len() {
                for b in (a + 1)..members.len() {
                    if examined >= budget {
                        exhausted = true;
                        break 'numeric;
                    }
                    examined += 1;
                    let (i, j) = (members[a], members[b]);
                    let diffs: Vec<usize> = token_lists[i]
                        .iter()
                        .zip(token_lists[j].iter())
                        .enumerate()
                        .filter(|(_, (x, y))| x != y)
                        .map(|(k, _)| k)
                        .collect();
                    // Closed class: exactly one differing position, both
                    // sides numerals there. (Masked equality already
                    // guarantees the token counts match.)
                    if diffs.len() == 1 {
                        let k = diffs[0];
                        if is_numeral(token_lists[i][k]) && is_numeral(token_lists[j][k]) {
                            propose(i, j, RULE_NUMERIC, &mut proposed, &mut refused)?;
                        }
                    }
                }
            }
        }
    }

    // Honest exhaustion (qt_bounded_traversal discipline): the budget
    // bounds both the ranked-candidate walk inside `search` AND the
    // grouped-pair walk below — either exhausting means candidates
    // remained unseen, and a silent `complete` would be a lie.
    let status = if exhausted || result.status == query::BudgetStatus::BudgetExhausted {
        query::BudgetStatus::BudgetExhausted
    } else {
        query::BudgetStatus::Complete
    };
    Ok(ContradictionReport {
        status,
        proposed: proposed.len(),
        refused,
        pairs: proposed,
    })
}
