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
/// `possible_duplicate_of` edge into the review queue. The walk is
/// budget-bounded — the budget bounds CANDIDATE comparisons (bajan-9pm):
/// pairs excluded by the sound pre-filters below are proven non-matches,
/// never examining them loses no information, so honest exhaustion means
/// "candidates remained unseen", not "all raw pairs visited".
///
/// Candidate pre-filters (both sound — they never exclude a true match):
///
/// 1. Normalization-equal pairs come from exact grouping on the
///    normalized text (computed once per hit, O(n)) — every pair inside
///    a group matches by construction.
/// 2. Containment candidates: for a hit `s`, a container `L` must
///    contain every character run of `s`, in particular every 4-gram of
///    `s`. The candidates for `s` are therefore the posting of `s`'s
///    rarest 4-gram (smallest posting; ties broken lexicographically),
///    filtered to strictly longer texts (equal-length containment
///    implies equality, already covered). For texts too short to have a
///    4-gram the filter falls back to all strictly longer hits.
///
/// `from`/`to` orientation: the contained (strictly shorter) text is the
/// suspected duplicate; on exact equality the lower claim key is `from`.
/// Evidence fingerprints order texts by ranked hit position — identical
/// to the pre-9pm walk, so a rejected pair's repropose guard compares
/// like for like across the change.
pub fn run_er_pass(db: &SqliteStore, budget: usize, now: u64) -> Result<ErReport, BajanQueryError> {
    // Ranked candidates (`qt_ranking`): the pass walks hits in ranked
    // order — an empty pattern matches every lineage-resolved claim.
    let result = query::search(db, "", budget)?;
    let hits = result.hits;
    // Normalization computed ONCE per hit (bajan-9pm): the old walk
    // re-normalized both sides inside the inner loop — quadratic recompute
    // on top of the quadratic pair walk.
    let norms: Vec<String> = hits.iter().map(|h| normalize(&h.text)).collect();

    let mut proposed = Vec::new();
    let mut refused = 0usize;
    let mut examined = 0usize;
    let mut exhausted = false;

    // One enqueue for a candidate pair {i, j} (i < j in ranked order):
    // orientation and fingerprint exactly as the pre-9pm walk computed
    // them, so queue order and repropose fingerprints are unchanged.
    let enqueue_pair = |i: usize,
                        j: usize,
                        from: usize,
                        to: usize,
                        evidence: &str,
                        proposed: &mut Vec<(usize, usize)>,
                        refused: &mut usize|
     -> Result<(), BajanQueryError> {
        let mut episodes: Vec<String> = hits[i].episodes.clone();
        episodes.extend(hits[j].episodes.clone());
        episodes.sort();
        episodes.dedup();
        let fingerprint = serde_json::json!({
            "texts": [hits[i].text, hits[j].text],
            "episodes": episodes,
        })
        .to_string();
        match db
            .enqueue_candidate(from, to, evidence, &fingerprint, now)
            .map_err(|e| BajanQueryError(e.to_string()))?
        {
            crate::store::EnqueueOutcome::Queued => proposed.push((from, to)),
            _ => *refused += 1,
        }
        Ok(())
    };

    // --- equality candidates: exact groups on the normalized text ---
    // Groups ordered by first member (ranked order); pairs within a
    // group by ranked position — the same order the old walk emitted
    // equal pairs in.
    let groups: Vec<(usize, Vec<usize>)> = {
        let mut by_text: std::collections::HashMap<&str, Vec<usize>> =
            std::collections::HashMap::new();
        for (idx, norm) in norms.iter().enumerate() {
            by_text.entry(norm.as_str()).or_default().push(idx);
        }
        let mut groups: Vec<(usize, Vec<usize>)> = by_text
            .into_values()
            .filter(|g| g.len() > 1)
            .map(|g| (*g.first().expect("non-empty"), g))
            .collect();
        groups.sort_by_key(|(first, _)| *first);
        groups
    };
    'equality: for (_, members) in &groups {
        for a in 0..members.len() {
            for b in (a + 1)..members.len() {
                if examined >= budget {
                    exhausted = true;
                    break 'equality;
                }
                examined += 1;
                let (i, j) = (members[a], members[b]);
                let (from, to) = if hits[i].claim_key <= hits[j].claim_key {
                    (hits[i].claim_key, hits[j].claim_key)
                } else {
                    (hits[j].claim_key, hits[i].claim_key)
                };
                enqueue_pair(
                    i,
                    j,
                    from,
                    to,
                    "normalization-equal",
                    &mut proposed,
                    &mut refused,
                )?;
            }
        }
    }

    // --- containment candidates: rarest-4-gram postings ---
    // Postings: 4-gram (within normalized tokens) -> ranked hit indices,
    // sorted. Soundness: if norm_s is a substring of norm_L, every
    // within-token 4-gram of s is a substring of L (a space-free gram
    // contained in L can never span L's token boundaries), so L is in
    // EVERY posting of s's grams — in particular the rarest one.
    let mut postings: std::collections::HashMap<String, Vec<u32>> =
        std::collections::HashMap::new();
    let mut grams_of: Vec<Vec<String>> = Vec::with_capacity(norms.len());
    for norm in &norms {
        // Distinct within-token 4-grams of the normalized text (chars,
        // not bytes — gram slices must not split UTF-8 code points).
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut grams: Vec<String> = Vec::new();
        for token in norm.split_whitespace() {
            let chars: Vec<char> = token.chars().collect();
            if chars.len() >= 4 {
                for win in chars.windows(4) {
                    let gram: String = win.iter().collect();
                    if seen.insert(gram.clone()) {
                        grams.push(gram);
                    }
                }
            }
        }
        for gram in &grams {
            postings.entry(gram.clone()).or_default();
        }
        grams_of.push(grams);
    }
    for (idx, grams) in grams_of.iter().enumerate() {
        for gram in grams {
            postings
                .get_mut(gram)
                .expect("gram was inserted")
                .push(idx as u32);
        }
    }
    for posting in postings.values_mut() {
        posting.sort_unstable();
        posting.dedup();
    }

    'containment: for si in 0..norms.len() {
        // Candidates: the rarest gram's posting (ties lexicographic),
        // filtered to strictly longer texts; fallback to all longer hits
        // when s has no 4-gram (too-short text, still sound).
        let candidates: Vec<u32> = match grams_of[si]
            .iter()
            .min_by_key(|g| (postings.get(*g).map_or(usize::MAX, |p| p.len()), g.as_str()))
            .and_then(|g| postings.get(g))
        {
            Some(posting) => posting
                .iter()
                .copied()
                .filter(|&li| norms[li as usize].len() > norms[si].len())
                .collect(),
            None => (0..norms.len() as u32)
                .filter(|&li| norms[li as usize].len() > norms[si].len())
                .collect(),
        };
        for li in candidates {
            if examined >= budget {
                exhausted = true;
                break 'containment;
            }
            examined += 1;
            let li = li as usize;
            if !norms[li].contains(&norms[si]) {
                continue;
            }
            // The contained (shorter) text is the suspected duplicate.
            let (i, j) = if si < li { (si, li) } else { (li, si) };
            enqueue_pair(
                i,
                j,
                hits[si].claim_key,
                hits[li].claim_key,
                "normalization-contains",
                &mut proposed,
                &mut refused,
            )?;
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
