//! Purpose: dependency-audit tests for the bajan-15i pinned NLP/IR crates.
//! Responsibilities: prove the pinned simhash crate orders near-duplicate
//! text closer than unrelated text (hamming-distance ordering), the
//! acceptance-criteria smoke test for the dedup pick.
//! Rationale: the deterministic-first pipeline (Chalef, `tv` talk 206) pins
//! simhash/entropy dedup over LLM passes; the pin is only useful if the
//! crate's distance metric behaves as the post-persist pass will require.

use simhash::{hamming_distance, simhash};

/// A long-ish English episode text.
const BASE: &str = "alpha beta gamma delta epsilon zeta eta theta \
                    iota kappa lambda mu nu xi omicron pi rho";

#[test]
fn near_duplicate_orders_closer_than_unrelated() {
    // Near-duplicate: same text with a small, local edit (two words swapped
    // for close neighbors — small edit distance).
    let near = "alpha beta gamma delta epsilon zeta eta theta \
                iota kappa lambda mu nu xi omicron sigma rho";
    // Unrelated: different vocabulary and length.
    let unrelated = "quantum ferrofluid turbidity accelerates under \
                     cyclical diminished brine coatings overnight";

    let h_base = simhash(BASE);
    let h_near = simhash(near);
    let h_unrelated = simhash(unrelated);

    let d_near = hamming_distance(h_base, h_near);
    let d_unrelated = hamming_distance(h_base, h_unrelated);

    assert!(
        d_near < d_unrelated,
        "near-duplicate distance ({d_near}) must be strictly smaller than \
         unrelated distance ({d_unrelated})"
    );
}

#[test]
fn identical_text_hashes_identically() {
    // Determinism baseline: same input, same hash, zero distance.
    assert_eq!(simhash(BASE), simhash(BASE));
    assert_eq!(hamming_distance(simhash(BASE), simhash(BASE)), 0);
}
