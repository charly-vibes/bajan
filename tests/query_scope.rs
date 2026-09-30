//! Purpose: property + scenario tests for `qt_scope_filtering` (bajan-825)
//! — tag-projection scope filtering over the persisted claim graph.
//! Responsibilities: red-first derivation of specs/query-tools.md
//! `p_scope_filtering` — claims with overlapping, disjoint, and unioned
//! tag projections against varied requested scopes.
//! Rationale: the tag projection is the union of the supporting episodes'
//! tag sets via the lineage walk (`ex_tag_inheritance` — never stored on
//! the claim); a read command scoped to a tag set returns only claims
//! whose projection intersects the requested scope; out-of-scope claims
//! are excluded — never silently blended (WR-SCOPE.1). Reads stay
//! read-only (`qt_readonly`).

use bajan::ingest::{EpisodeRecord, Locator, SourceMeta};
use bajan::query::{self, BudgetStatus};
use bajan::store::sqlite::SqliteStore;
use bajan::store::{ClaimNode, ClaimStatus, Evidence, Lineage};
use proptest::prelude::*;
use std::collections::BTreeSet;

const PATTERN: &str = "alpha";

fn episode_with(id: &str, tags: &[&str]) -> EpisodeRecord {
    EpisodeRecord {
        id: id.into(),
        text: "alpha beta gamma delta epsilon zeta.".into(),
        locator: Locator::Span("heading:Notes".into()),
        source: SourceMeta {
            source_type: "episode".into(),
            data_cutoff: None,
            authority_tier: 1,
            tags: tags.iter().map(|t| t.to_string()).collect(),
        },
    }
}

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
fn seed(db: &SqliteStore, id: &str, tags: &[&str]) -> usize {
    let ep = episode_with(id, tags);
    bajan::ingest::persist(db, &ep).expect("persist");
    let (node, lineage) = matching_claim(id);
    db.insert_claim(&node, &[lineage], &ep.text)
        .expect("insert")
}

/// Seed one claim backed by two episodes (unioned projection).
fn seed_multi_source(
    db: &SqliteStore,
    id_a: &str,
    tags_a: &[&str],
    id_b: &str,
    tags_b: &[&str],
) -> usize {
    let ep_a = episode_with(id_a, tags_a);
    let ep_b = episode_with(id_b, tags_b);
    bajan::ingest::persist(db, &ep_a).expect("persist");
    bajan::ingest::persist(db, &ep_b).expect("persist");
    let (node, lineage_a) = matching_claim(id_a);
    let (_, lineage_b) = matching_claim(id_b);
    db.insert_claim(&node, &[lineage_a, lineage_b], &ep_a.text)
        .expect("insert")
}

// qt_scope_filtering: a claim is in scope when its resolved tag
// projection intersects the requested scope — overlapping projections
// split cleanly along the requested tag.
#[test]
fn scope_returns_only_intersecting_claims() {
    let db = SqliteStore::open_in_memory().expect("open");
    let with_api = seed(&db, "ep-api", &["workspace:dev", "topic:api"]);
    let ops_only = seed(&db, "ep-ops", &["topic:ops"]);

    let result =
        query::search_scoped(&db, PATTERN, 16, &["topic:api".to_string()]).expect("scoped search");
    assert!(matches!(result.status, BudgetStatus::Complete));
    let keys: Vec<usize> = result.hits.iter().map(|h| h.claim_key).collect();
    assert_eq!(
        keys,
        vec![with_api],
        "only the api-tagged claim is in scope"
    );
    assert!(!keys.contains(&ops_only), "out-of-scope claim excluded");
}

// Disjoint projections: an empty scoped answer is honest — the walk
// completed (complete status), nothing silently blended in.
#[test]
fn disjoint_projection_reports_complete_with_no_hits() {
    let db = SqliteStore::open_in_memory().expect("open");
    seed(&db, "ep-dev", &["workspace:dev"]);

    let result =
        query::search_scoped(&db, PATTERN, 16, &["topic:ops".to_string()]).expect("scoped search");
    assert!(matches!(result.status, BudgetStatus::Complete));
    assert!(result.hits.is_empty(), "disjoint scope excludes everything");
}

// Unioned projections (ex_tag_inheritance): a multi-source claim's
// projection is the union over its lineage edges — one in-scope episode
// puts the whole claim in scope.
#[test]
fn multi_source_claim_in_scope_via_union() {
    let db = SqliteStore::open_in_memory().expect("open");
    let key = seed_multi_source(&db, "ep-dev", &["workspace:dev"], "ep-ops", &["topic:ops"]);

    let result =
        query::search_scoped(&db, PATTERN, 16, &["topic:ops".to_string()]).expect("scoped search");
    let keys: Vec<usize> = result.hits.iter().map(|h| h.claim_key).collect();
    assert_eq!(
        keys,
        vec![key],
        "union projection intersects via one episode"
    );

    // And via the other side too — the union is symmetric.
    let result = query::search_scoped(&db, PATTERN, 16, &["workspace:dev".to_string()])
        .expect("scoped search");
    let keys: Vec<usize> = result.hits.iter().map(|h| h.claim_key).collect();
    assert_eq!(keys, vec![key]);
}

// The scope predicate is pure set intersection: an empty requested
// scope intersects no projection, so the scoped answer is honestly
// empty — the unfiltered read command (`search`) is the no-filter entry
// point and still returns everything.
#[test]
fn empty_scope_intersects_nothing() {
    let db = SqliteStore::open_in_memory().expect("open");
    seed(&db, "ep-dev", &["workspace:dev"]);
    seed(&db, "ep-ops", &["topic:ops"]);

    let scoped = query::search_scoped(&db, PATTERN, 16, &[]).expect("scoped search");
    assert!(matches!(scoped.status, BudgetStatus::Complete));
    assert!(scoped.hits.is_empty(), "empty scope intersects nothing");

    let unscoped = query::search(&db, PATTERN, 16).expect("unscoped search");
    assert_eq!(unscoped.hits.len(), 2, "unfiltered read is unchanged");
}

// qt_query-schema: the result record echoes the requested scope — the
// published `scope` field, empty when unscoped.
#[test]
fn result_echoes_requested_scope() {
    let db = SqliteStore::open_in_memory().expect("open");
    seed(&db, "ep-api", &["topic:api"]);

    let scoped =
        query::search_scoped(&db, PATTERN, 16, &["topic:api".to_string()]).expect("scoped search");
    assert_eq!(scoped.scope, vec!["topic:api".to_string()]);
    let unscoped = query::search(&db, PATTERN, 16).expect("unscoped search");
    assert!(
        unscoped.scope.is_empty(),
        "unscoped search echoes empty scope"
    );
}

// qt_readonly (regression with scope filtering in the path): scoped reads
// never mutate the store.
#[test]
fn scoped_read_stays_readonly() {
    let db = SqliteStore::open_in_memory().expect("open");
    seed(&db, "ep-dev", &["workspace:dev"]);
    seed(&db, "ep-ops", &["topic:ops"]);
    let before = db.dump_extraction_output().expect("dump read");
    let _ =
        query::search_scoped(&db, PATTERN, 16, &["topic:ops".to_string()]).expect("scoped search");
    assert_eq!(
        db.dump_extraction_output().expect("dump read"),
        before,
        "scope filtering must not write (qt_readonly)"
    );
}

proptest! {
    // p_scope_filtering: claims with overlapping, disjoint, and unioned
    // tag projections against varied requested scopes — results contain
    // exactly the claims whose projected tags intersect the scope;
    // out-of-scope claims never appear.
    #[test]
    fn scoped_results_are_exactly_the_projection_intersection(
        // Per claim: which of the four tagspace tags its episodes carry.
        projections in proptest::collection::vec(
            proptest::collection::hash_set(0usize..4, 0..4),
            1..8,
        ),
        // The requested scope: a subset of the same tagspace.
        scope in proptest::collection::hash_set(0usize..4, 0..4),
    ) {
        const TAGS: [&str; 4] = ["t:alpha", "t:beta", "t:gamma", "t:delta"];
        let db = SqliteStore::open_in_memory().expect("open");
        let scope_set: BTreeSet<usize> = scope.iter().copied().collect();

        let mut expected: Vec<usize> = Vec::new();
        for (i, proj) in projections.iter().enumerate() {
            let tag_names: Vec<&str> = proj.iter().map(|t| TAGS[*t]).collect();
            // Split the projection across two episodes (unioned lineage)
            // so the property also exercises multi-source claims.
            let split = proj.len() / 2;
            let tags_a: Vec<&str> =
                tag_names.iter().take(split).copied().collect();
            let tags_b: Vec<&str> =
                tag_names.iter().skip(split).copied().collect();
            let key = if proj.len() >= 2 && !tags_b.is_empty() {
                let ia = format!("ep-{i}-a");
                let ib = format!("ep-{i}-b");
                seed_multi_source(&db, &ia, &tags_a, &ib, &tags_b)
            } else {
                seed(&db, &format!("ep-{i}"), &tag_names)
            };
            let in_scope = proj.iter().any(|t| scope_set.contains(t));
            if in_scope {
                expected.push(key);
            }
        }
        expected.sort();

        let scope_names: Vec<String> =
            scope_set.iter().map(|t| TAGS[*t].to_string()).collect();
        let result = query::search_scoped(&db, PATTERN, 64, &scope_names)
            .expect("scoped search");
        let actual: Vec<usize> = result.hits.iter().map(|h| h.claim_key).collect();

        prop_assert_eq!(actual, expected, "exactly the in-scope claims, no out-of-scope blend");
        prop_assert_eq!(result.scope, scope_names, "requested scope echoed");
    }
}
