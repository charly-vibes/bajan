//! Purpose: property + scenario tests for `qt_ranking` (bajan-9dv) —
//! authority-then-recency ordering with staleness demotion over the
//! persisted claim graph.
//! Responsibilities: red-first derivation of specs/query-tools.md
//! `p_ranking` — hit sets mixing authority tiers, cutoffs spanning the
//! staleness window, and equal-rank ties.
//! Rationale: ranking uses the supporting episodes' metadata reachable
//! through lineage (WR-RANK.1/.2, WR-TIME.3): authority tier first, then
//! data-cutoff recency; a stale high-authority hit demotes below fresh
//! lower-authority hits; every hit surfaces staleness; the best-scoring
//! hit carries the staleness warning when its cutoff is stale. Reads stay
//! read-only (`qt_readonly`).

use bajan::ingest::{EpisodeRecord, Locator, SourceMeta};
use bajan::query::{self, BudgetStatus};
use bajan::store::sqlite::SqliteStore;
use bajan::store::{ClaimNode, ClaimStatus, Evidence, Lineage};
use proptest::prelude::*;

const PATTERN: &str = "alpha";

/// A fresh cutoff: one day before today (UTC). Computed from the same
/// clock the implementation reads, so the 365-day window can never
/// misclassify it within a single test run.
fn fresh_cutoff() -> String {
    query::iso_from_days(query::today_days_utc() - 1)
}

/// A stale cutoff: far beyond the 365-day window.
const STALE_CUTOFF: &str = "2019-06-01";

fn episode_with(id: &str, tier: u8, cutoff: Option<String>) -> EpisodeRecord {
    EpisodeRecord {
        id: id.into(),
        text: "alpha beta gamma delta epsilon zeta.".into(),
        locator: Locator::Span("heading:Notes".into()),
        source: SourceMeta {
            source_type: "episode".into(),
            data_cutoff: cutoff,
            authority_tier: tier,
            tags: vec!["workspace:dev".into()],
        },
    }
}

/// A staged claim matching `PATTERN`, backed by the named episode. The
/// claim text is a whitespace-collapsed-contained span of the episode
/// text (typed gate passes) with no hedge marker.
fn matching_claim(episode_id: &str) -> (ClaimNode, Lineage) {
    (
        ClaimNode {
            text: "alpha beta gamma".into(),
            valid_at: None,
            invalid_at: None,
            data_cutoff: None,
            status: ClaimStatus::Staged,
            scope: "workspace:dev".into(),
            source_type: "episode".into(),
            evidence: Evidence::Span {
                text: "alpha beta gamma".into(),
                locator: "heading:Notes".into(),
            },
        },
        Lineage {
            episode_id: episode_id.into(),
            extractor_version: "0.1.0".into(),
        },
    )
}

/// Seed one (episode, claim) pair; returns the inserted claim key.
fn seed(db: &SqliteStore, id: &str, tier: u8, cutoff: Option<String>) -> usize {
    let ep = episode_with(id, tier, cutoff);
    bajan::ingest::persist(db, &ep).expect("persist");
    let (node, lineage) = matching_claim(id);
    db.insert_claim(&node, &[lineage], &ep.text)
        .expect("insert")
}

// WR-RANK.1: authority first — among comparably-fresh hits, the higher
// authority (lower tier value) ranks first.
#[test]
fn authority_outranks_when_comparably_fresh() {
    let db = SqliteStore::open_in_memory().expect("open");
    let fresh = fresh_cutoff();
    let low = seed(&db, "ep-low", 3, Some(fresh.clone()));
    let high = seed(&db, "ep-high", 0, Some(fresh));

    let result = query::search(&db, PATTERN, 16).expect("search");
    assert!(matches!(result.status, BudgetStatus::Complete));
    let keys: Vec<usize> = result.hits.iter().map(|h| h.claim_key).collect();
    assert_eq!(keys, vec![high, low], "tier 0 outranks tier 3");
    assert!(
        result.staleness_warning.is_none(),
        "no warning when the best hit is fresh"
    );
}

// WR-RANK.1: within one authority tier, recency decides — most recent
// data cutoff first.
#[test]
fn recency_breaks_ties_within_one_tier() {
    let db = SqliteStore::open_in_memory().expect("open");
    let older = seed(&db, "ep-old", 1, Some("2025-01-01".into()));
    let newer = seed(&db, "ep-new", 1, Some("2026-06-01".into()));

    let result = query::search(&db, PATTERN, 16).expect("search");
    let keys: Vec<usize> = result.hits.iter().map(|h| h.claim_key).collect();
    assert_eq!(keys, vec![newer, older], "newer cutoff first within a tier");
}

// WR-RANK.2: authority never overrides staleness — a stale high-authority
// hit demotes below a fresh lower-authority hit; every hit surfaces its
// staleness; the (fresh) best hit carries no warning.
#[test]
fn stale_high_authority_demotes_below_fresh_low_authority() {
    let db = SqliteStore::open_in_memory().expect("open");
    let stale_high = seed(&db, "ep-stale-high", 0, Some(STALE_CUTOFF.into()));
    let fresh_low = seed(&db, "ep-fresh-low", 3, Some(fresh_cutoff()));

    let result = query::search(&db, PATTERN, 16).expect("search");
    let keys: Vec<usize> = result.hits.iter().map(|h| h.claim_key).collect();
    assert_eq!(
        keys,
        vec![fresh_low, stale_high],
        "fresh low authority outranks stale high authority"
    );
    let by_key = |k: usize| result.hits.iter().find(|h| h.claim_key == k).unwrap();
    assert!(by_key(stale_high).stale, "stale hit surfaces stale=true");
    assert!(!by_key(fresh_low).stale, "fresh hit surfaces stale=false");
    assert!(
        result.staleness_warning.is_none(),
        "the best hit is fresh — no staleness warning"
    );
}

// WR-TIME.3: the best-scoring hit carries the staleness warning when its
// cutoff is stale.
#[test]
fn best_hit_carries_staleness_warning_when_stale() {
    let db = SqliteStore::open_in_memory().expect("open");
    let key = seed(&db, "ep-only", 0, Some(STALE_CUTOFF.into()));

    let result = query::search(&db, PATTERN, 16).expect("search");
    assert_eq!(result.hits.len(), 1);
    assert!(result.hits[0].stale);
    let warning = result
        .staleness_warning
        .expect("stale best hit carries the warning");
    assert_eq!(warning.claim_key, key);
    assert_eq!(warning.data_cutoff, STALE_CUTOFF);
}

// Missing cutoff: honest absence — never guessed fresh, never guessed
// stale; surfaces verbatim and sorts least-fresh within its tier. The
// dated hit is fresh so recency is what separates them (WR-RANK.2
// demotion would dominate otherwise).
#[test]
fn absent_cutoff_sorts_least_fresh_without_warning() {
    let db = SqliteStore::open_in_memory().expect("open");
    let with_cutoff = seed(&db, "ep-dated", 1, Some(fresh_cutoff()));
    let absent = seed(&db, "ep-undated", 1, None);

    let result = query::search(&db, PATTERN, 16).expect("search");
    let keys: Vec<usize> = result.hits.iter().map(|h| h.claim_key).collect();
    assert_eq!(keys, vec![with_cutoff, absent], "dated hit before undated");
    assert!(!result.hits[1].stale, "absent cutoff is not asserted stale");
}

// qt_readonly (regression with ranking in the path): ranked reads never
// mutate the store.
#[test]
fn ranked_read_stays_readonly() {
    let db = SqliteStore::open_in_memory().expect("open");
    seed(&db, "ep-a", 0, Some(STALE_CUTOFF.into()));
    seed(&db, "ep-b", 3, Some(fresh_cutoff()));
    let before = db.dump_extraction_output().expect("dump read");
    let _ = query::search(&db, PATTERN, 16).expect("search");
    assert_eq!(
        db.dump_extraction_output().expect("dump read"),
        before,
        "ranking must not write (qt_readonly)"
    );
}

proptest! {
    // p_ranking: hit sets mixing authority tiers, cutoffs spanning the
    // staleness window, and equal-rank ties — results order
    // authority-first then recency; stale flags are per-hit; the warning
    // appears exactly when the best hit is stale.
    #[test]
    fn ranked_order_is_authority_then_recency(
        support in proptest::collection::vec(
            (0u8..=3, proptest::option::of(0i64..=4_000)),
            1..8,
        ),
    ) {
        let db = SqliteStore::open_in_memory().expect("open");
        let today = query::today_days_utc();
        let mut expected: Vec<(usize, u8, i64)> = Vec::new();
        for (i, (tier, cutoff_age_days)) in support.iter().enumerate() {
            let id = format!("ep-{i}");
            // cutoff_age_days: days before today; None = absent cutoff.
            let cutoff = cutoff_age_days.map(|age| query::iso_from_days(today - age));
            let key = seed(&db, &id, *tier, cutoff);
            let cutoff_days = cutoff_age_days.map(|age| today - age);
            expected.push((key, *tier, cutoff_days.unwrap_or(i64::MIN)));
        }

        let result = query::search(&db, PATTERN, 64).expect("search");
        prop_assert_eq!(result.hits.len(), expected.len());

        // Expected order (WR-RANK.2): fresh before stale (an absent
        // cutoff is never stale — not a MIN-vs-window comparison), then
        // tier ascending, cutoff recency descending (absent cutoff least
        // fresh), claim key ascending on ties.
        let is_stale = |cutoff: i64| cutoff != i64::MIN && cutoff < today - 365;
        expected.sort_by(|a, b| {
            is_stale(a.2)
                .cmp(&is_stale(b.2))
                .then(a.1.cmp(&b.1))
                .then(b.2.cmp(&a.2))
                .then(a.0.cmp(&b.0))
        });
        let actual: Vec<(usize, u8, i64)> = result
            .hits
            .iter()
            .map(|h| (h.claim_key, h.authority_tier, h.cutoff_days.unwrap_or(i64::MIN)))
            .collect();
        prop_assert_eq!(actual, expected, "authority-then-recency order");

        // Per-hit staleness: cutoff older than the window (365 days).
        for hit in &result.hits {
            let want_stale = hit
                .cutoff_days
                .map(|d| d < today - query::STALENESS_WINDOW_DAYS)
                .unwrap_or(false);
            prop_assert_eq!(hit.stale, want_stale, "stale flag per hit");
        }

        // Warning iff the best (first) hit is stale, and it names that hit.
        match (&result.hits[0].stale, &result.staleness_warning) {
            (true, Some(w)) => prop_assert_eq!(w.claim_key, result.hits[0].claim_key),
            (false, None) => {}
            other => prop_assert!(
                false,
                "warning must track the best hit's staleness, got {other:?}"
            ),
        }
    }
}
