//! Purpose: property + scenario tests for the resolve command (bajan-7q8)
//! — claim identity resolution across versions on the persisted store.
//! Responsibilities: red-first derivation of `ex_supersession` on the
//! wired SQLite path (version-bump re-extraction tombstones prior-version
//! staged claims), the identity report (lineage, status, tombstones,
//! staged invalidation proposals joined per claim), and the explicit
//! re-stage action (the only path back to `staged`).
//! Rationale: governed by specs/extraction-claims.md — supersession is
//! automated (the re-extraction event), recovery never is: a superseded
//! claim re-enters `staged` only through an explicit re-stage action with
//! actor identity. The report is a read (qt_readonly): structure only,
//! never adjudication.

use bajan::extract::{self, Reason};
use bajan::ingest::{EpisodeRecord, Locator, SourceMeta};
use bajan::resolve;
use bajan::store::sqlite::SqliteStore;
use bajan::store::{ClaimNode, ClaimStatus, ClaimStore, Evidence, Lineage, StoreError};
use proptest::prelude::*;

const V1: &str = "0.1.0";
const V2: &str = "0.2.0";

fn episode(id: &str, text: &str) -> EpisodeRecord {
    EpisodeRecord {
        id: id.into(),
        text: text.into(),
        locator: Locator::Span("heading:Notes".into()),
        source: SourceMeta {
            source_type: "episode".into(),
            data_cutoff: None,
            authority_tier: 1,
            tags: vec!["workspace:dev".into()],
        },
    }
}

/// Seed an episode and one claim at the given version/status; returns
/// the claim key.
fn seed(db: &SqliteStore, id: &str, text: &str, version: &str, status: ClaimStatus) -> usize {
    let ep = episode(id, text);
    bajan::ingest::persist(db, &ep).expect("persist");
    let node = ClaimNode {
        text: text.into(),
        valid_at: None,
        invalid_at: None,
        data_cutoff: None,
        status,
        scope: "workspace:dev".into(),
        source_type: "episode".into(),
        evidence: Evidence::Span {
            text: text.into(),
            locator: "heading:Notes".into(),
        },
    };
    db.insert_claim(
        &node,
        &[Lineage {
            episode_id: id.into(),
            extractor_version: version.into(),
        }],
        &ep.text,
    )
    .expect("insert")
}

fn status_of(db: &SqliteStore, key: usize) -> ClaimStatus {
    db.claims_with_lineage()
        .expect("read")
        .into_iter()
        .find(|(k, _, _)| *k == key)
        .expect("claim exists")
        .1
        .status
}

// ex_supersession (wired): re-extracting at a bumped version tombstones
// only that episode's prior-version staged claims — active and rejected
// claims from any version are never mutated, other episodes untouched,
// lineage and evidence intact.
#[test]
fn version_bump_supersedes_only_prior_version_staged_claims() {
    let db = SqliteStore::open_in_memory().expect("open");
    let staged = seed(&db, "ep-a", "alpha claim text", V1, ClaimStatus::Staged);
    let active = seed(&db, "ep-a", "alpha active text", V1, ClaimStatus::Active);
    let gate_refused = seed(&db, "ep-a", "alpha refused text", V1, ClaimStatus::Rejected);
    // An episode NOT re-extracted at the bump (already cached at V2):
    // its claims are never touched — only re-extracted episodes supersede.
    let other = seed(&db, "ep-b", "beta claim text", V1, ClaimStatus::Staged);
    db.mark_extracted("ep-b", V2).expect("cache marker");

    let report =
        extract::run_extract_versioned(&db, V2, &mut Default::default()).expect("extract v2");
    assert_eq!(
        report.superseded, 1,
        "only the ep-a prior-version staged claim"
    );

    assert_eq!(status_of(&db, staged), ClaimStatus::Rejected);
    assert_eq!(status_of(&db, other), ClaimStatus::Staged);
    assert_eq!(status_of(&db, active), ClaimStatus::Active);
    assert_eq!(status_of(&db, gate_refused), ClaimStatus::Rejected);

    // Tombstone trail: exactly the one superseded claim, honest reason.
    let records = db.supersessions().expect("read");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].claim_key, staged);
    assert_eq!(records[0].reason, Reason::SupersededByReextraction);

    // Tombstone, not delete: lineage and evidence stay queryable.
    let (_, node, lineage) = db
        .claims_with_lineage()
        .expect("read")
        .into_iter()
        .find(|(k, _, _)| *k == staged)
        .expect("tombstoned claim persists");
    assert_eq!(node.text, "alpha claim text");
    assert_eq!(lineage.len(), 1);
    assert_eq!(lineage[0].extractor_version, V1);
}

// ex_supersession: a same-version re-extract supersedes nothing and
// resurrects nothing (cache economics, no provenance inequality).
#[test]
fn same_version_reextract_supersedes_nothing() {
    let db = SqliteStore::open_in_memory().expect("open");
    let ep = episode("ep-a", "alpha claim text");
    bajan::ingest::persist(&db, &ep).expect("persist");

    extract::run_extract_versioned(&db, V1, &mut Default::default()).expect("extract v1");
    assert!(db.supersessions().expect("read").is_empty());
    assert_eq!(db.claims_with_lineage().expect("read").len(), 1);

    extract::run_extract_versioned(&db, V1, &mut Default::default()).expect("re-extract v1");
    assert!(db.supersessions().expect("read").is_empty());
    assert_eq!(db.claims_with_lineage().expect("read").len(), 1);
}

// ex_supersession anti-automated-recovery: no automated path revives a
// tombstoned claim — reruns at the bumped version hit the cache, and a
// further bump only tombstones staged claims again.
#[test]
fn no_automated_recovery_of_superseded_claims() {
    let db = SqliteStore::open_in_memory().expect("open");
    let staged = seed(&db, "ep-a", "alpha claim text", V1, ClaimStatus::Staged);
    extract::run_extract_versioned(&db, V2, &mut Default::default()).expect("extract v2");
    assert_eq!(status_of(&db, staged), ClaimStatus::Rejected);

    // Cache hit: nothing happens at all.
    extract::run_extract_versioned(&db, V2, &mut Default::default()).expect("re-extract v2");
    assert_eq!(status_of(&db, staged), ClaimStatus::Rejected);

    // A further bump tombstones the (staged) v2 claim; the v1 tombstone
    // is never revived by it.
    extract::run_extract_versioned(&db, "0.3.0", &mut Default::default()).expect("extract v3");
    assert_eq!(status_of(&db, staged), ClaimStatus::Rejected);
    assert!(
        db.supersessions()
            .expect("read")
            .iter()
            .all(|r| r.claim_key != staged || r.reason == Reason::SupersededByReextraction)
    );
}

// The identity report joins every persisted claim's lineage, status,
// tombstones, and staged invalidation proposals — read-only, exactly one
// record per claim.
#[test]
fn report_joins_lineage_tombstones_and_proposals() {
    let db = SqliteStore::open_in_memory().expect("open");
    let staged = seed(&db, "ep-a", "alpha claim text", V1, ClaimStatus::Staged);
    let active = seed(&db, "ep-a", "alpha active text", V1, ClaimStatus::Active);
    extract::run_extract_versioned(&db, V2, &mut Default::default()).expect("extract v2");
    db.stage_invalidation_proposal(active, "ep-new")
        .expect("proposal");

    let report = resolve::resolve(&db).expect("resolve");
    assert_eq!(
        report.claims.len(),
        db.claims_with_lineage().expect("read").len()
    );
    assert_eq!(report.superseded, 1);

    let record = report
        .claims
        .iter()
        .find(|c| c.claim_key == staged)
        .expect("tombstoned claim reported");
    assert_eq!(record.status, ClaimStatus::Rejected);
    assert_eq!(
        record.lineage,
        vec![Lineage {
            episode_id: "ep-a".into(),
            extractor_version: V1.into(),
        }]
    );
    assert_eq!(record.supersessions.len(), 1);
    assert_eq!(
        record.supersessions[0].reason,
        Reason::SupersededByReextraction
    );
    assert!(record.invalidations.is_empty());

    let active_record = report
        .claims
        .iter()
        .find(|c| c.claim_key == active)
        .expect("active claim reported");
    assert_eq!(active_record.status, ClaimStatus::Active);
    assert!(active_record.supersessions.is_empty());
    // Two proposals join here: the manually staged one (causing ep-new)
    // plus the one the V2 re-extraction wired in (bajan-c4p): the V2
    // output conflicts with this ACTIVE claim — same locator, different
    // collapsed claim text — staging a proposal citing causing ep-a.
    assert_eq!(active_record.invalidations.len(), 2);
    let mut causing: Vec<&str> = active_record
        .invalidations
        .iter()
        .map(|p| p.causing_episode_id.as_str())
        .collect();
    causing.sort();
    assert_eq!(causing, vec!["ep-a", "ep-new"]);
    assert!(
        active_record
            .invalidations
            .iter()
            .all(|p| p.claim_key == active)
    );
}

// ex_supersession: the explicit re-stage is the only path from a
// superseded claim back to staged — with actor identity, recorded.
#[test]
fn explicit_restage_moves_superseded_claim_back_to_staged() {
    let db = SqliteStore::open_in_memory().expect("open");
    let staged = seed(&db, "ep-a", "alpha claim text", V1, ClaimStatus::Staged);
    extract::run_extract_versioned(&db, V2, &mut Default::default()).expect("extract v2");
    assert_eq!(status_of(&db, staged), ClaimStatus::Rejected);

    let record = resolve::restage(&db, staged, "sasha", 1_000).expect("restage");
    assert_eq!(record.claim_key, staged);
    assert_eq!(record.actor, "sasha");
    assert_eq!(record.restaged_at, 1_000);
    assert_eq!(status_of(&db, staged), ClaimStatus::Staged);

    // The tombstone history is retained (a tombstone, not a rewrite).
    assert!(
        db.supersessions()
            .expect("read")
            .iter()
            .any(|r| r.claim_key == staged)
    );

    // And the report reflects the new status honestly.
    let report = resolve::resolve(&db).expect("resolve");
    let record = report
        .claims
        .iter()
        .find(|c| c.claim_key == staged)
        .expect("restaged claim reported");
    assert_eq!(record.status, ClaimStatus::Staged);
}

// ex_supersession guard: re-stage applies to superseded (tombstoned)
// claims only — staged, active, gate-refused, and unknown claims are
// refused with a spec-traced error.
#[test]
fn restage_refused_without_tombstone() {
    let db = SqliteStore::open_in_memory().expect("open");
    let staged = seed(&db, "ep-a", "alpha claim text", V1, ClaimStatus::Staged);
    let active = seed(&db, "ep-a", "alpha active text", V1, ClaimStatus::Active);
    let refused = seed(&db, "ep-a", "alpha refused text", V1, ClaimStatus::Rejected);

    for (key, status) in [
        (staged, "staged"),
        (active, "active"),
        (refused, "rejected"),
    ] {
        let err = resolve::restage(&db, key, "sasha", 1_000).expect_err(status);
        assert!(
            matches!(err, bajan::cli::BajanError::Store(ref m) if m.contains("ex_supersession") || m.contains("re-stage")),
            "{status}: {err:?}"
        );
    }
    // Unknown claim key.
    let err = resolve::restage(&db, 999, "sasha", 1_000).expect_err("unknown");
    assert!(matches!(
        err,
        bajan::cli::BajanError::Store(ref m) if m.contains("999")
    ));
}

// gm_embedded_store: supersession tombstones persist as plain rows and
// survive dump-recreate — statuses and tombstone trail reproduce exactly.
#[test]
fn dump_recreate_preserves_supersession_tombstones() {
    let db = SqliteStore::open_in_memory().expect("open");
    let staged = seed(&db, "ep-a", "alpha claim text", V1, ClaimStatus::Staged);
    extract::run_extract_versioned(&db, V2, &mut Default::default()).expect("extract v2");

    let dump = db.dump_extraction_output().expect("dump");
    let fresh = SqliteStore::open_in_memory().expect("open");
    let ep = episode("ep-a", "alpha claim text");
    bajan::ingest::persist(&fresh, &ep).expect("persist");
    fresh.restore_extraction_output(&dump).expect("restore");

    assert_eq!(status_of(&fresh, staged), ClaimStatus::Rejected);
    let before = db.supersessions().expect("read");
    let after = fresh.supersessions().expect("read");
    assert_eq!(before, after, "tombstone trail reproduces exactly");
}

// p_supersession (wired): over random corpora, a version bump tombstones
// exactly the prior-version staged claims — never active or rejected
// claims — and the report joins every claim exactly once.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn p_wired_supersession(
        episodes in proptest::collection::vec(
            (0usize..4usize, "[a-z ]{10,60}"),
            1..4,
        ),
    ) {
        let db = SqliteStore::open_in_memory().expect("open");
        let mut staged_keys = Vec::new();
        let mut kept_keys = Vec::new();
        for (i, (_, text)) in episodes.iter().enumerate() {
            let id = format!("ep-{i}");
            let staged = seed(&db, &id, text, V1, ClaimStatus::Staged);
            staged_keys.push(staged);
            let active = seed(&db, &id, text, V1, ClaimStatus::Active);
            kept_keys.push(active);
        }

        extract::run_extract_versioned(&db, V2, &mut Default::default()).expect("extract v2");

        for key in &staged_keys {
            prop_assert_eq!(status_of(&db, *key), ClaimStatus::Rejected);
        }
        for key in &kept_keys {
            prop_assert_eq!(status_of(&db, *key), ClaimStatus::Active);
        }

        let records = db.supersessions().expect("read");
        prop_assert_eq!(records.len(), staged_keys.len());
        let report = resolve::resolve(&db).expect("resolve");
        prop_assert_eq!(report.claims.len(), db.claims_with_lineage().expect("read").len());
        prop_assert_eq!(report.superseded, staged_keys.len());
        for record in &report.claims {
            if staged_keys.contains(&record.claim_key) {
                prop_assert_eq!(record.supersessions.len(), 1);
            } else {
                prop_assert!(record.supersessions.is_empty());
            }
        }
    }
}

// The in-memory ClaimStore keeps its supersession semantics (the sqlite
// store mirrors them); the mirror is the honest same-semantics test
// surface — superseded keys are returned in stored row order.
#[test]
fn in_memory_store_semantics_unchanged() {
    let mut store = ClaimStore::default();
    let ep = episode("ep-a", "alpha claim text");
    let node = ClaimNode {
        text: "alpha claim text".into(),
        valid_at: None,
        invalid_at: None,
        data_cutoff: None,
        status: ClaimStatus::Staged,
        scope: "workspace:dev".into(),
        source_type: "episode".into(),
        evidence: Evidence::Unknown,
    };
    store
        .insert(
            node.clone(),
            Lineage {
                episode_id: "ep-a".into(),
                extractor_version: V1.into(),
            },
            &ep.text,
        )
        .expect("insert");
    let superseded = store.supersede_prior_versions("ep-a", V2, 42);
    assert_eq!(superseded, vec![0]);
    assert_eq!(store.nodes()[0].status, ClaimStatus::Rejected);
    let _ = StoreError::ClaimNotFound { claim_key: 0 }; // type still importable
}
