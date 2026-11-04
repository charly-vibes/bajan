//! Episode tombstone + revive property tests (bajan-aod), red-first.
//!
//! Purpose: property coverage for `ic_deleted_tombstone` and
//! `ic_deleted_recreate` (specs/ingestion-contract.md, deployed mirror
//! p_deleted_tombstone / p_deleted_recreate) — the last two unwired
//! contracts blocking bajan-16o.
//! Responsibilities: ingest → delete → re-ingest equals plain ingest minus
//! the deleted episode in any submission order; without revive every
//! re-submission is rejected with the machine-readable deleted-id reason;
//! revive alone changes nothing; the first post-revive submission persists
//! exactly once; no automated path (re-ingest, dump-recreate) clears a
//! tombstone; deletion is refused while a claim supports the episode.
//! Rationale: written red before the implementation, per repo TDD policy.

use bajan::ingest::{self, EpisodeRecord, IngestOutcome, Locator, RejectReason, SourceMeta};
use bajan::store::sqlite::SqliteStore;
use bajan::store::{ClaimNode, ClaimStatus, Evidence, Lineage, StoreError};
use proptest::prelude::*;

fn episode(id: &str, text: &str) -> EpisodeRecord {
    EpisodeRecord {
        id: id.into(),
        text: text.into(),
        locator: Locator::Span("heading:Notes".into()),
        source: SourceMeta {
            source_type: "episode".into(),
            data_cutoff: Some("2026-01-01".into()),
            authority_tier: 2,
            tags: vec!["workspace:dev".into()],
        },
    }
}

/// Fisher-Yates over a xorshift64* PRNG — dependency-free deterministic
/// shuffle seeded by the proptest-generated `seed` (vertical_slice
/// precedent).
fn shuffle_with_seed(records: &mut [EpisodeRecord], seed: u64) {
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };
    for i in (1..records.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        records.swap(i, j);
    }
}

fn persisted_ids(db: &SqliteStore) -> Vec<String> {
    let mut ids: Vec<String> = db
        .episodes()
        .expect("episode read emits the envelope, never a panic")
        .into_iter()
        .map(|e| e.id)
        .collect();
    ids.sort();
    ids
}

fn assert_deleted_rejection(outcome: &Result<IngestOutcome, bajan::cli::BajanError>) {
    match outcome {
        Ok(IngestOutcome::Rejected {
            reason: RejectReason::DeletedId,
        }) => {}
        other => panic!("expected deleted-id rejection, got {other:?}"),
    }
}

// ---- ic_deleted_tombstone (p_deleted_tombstone): ingest → delete →
// re-ingest equals plain ingest exactly minus the deleted episode, in any
// re-submission order; the tombstone survives every re-submission. ----

proptest! {
    #[test]
    fn p_deleted_tombstone(
        n in 2usize..8,
        victim_pick in 0usize..8,
        seed in proptest::num::u64::ANY,
    ) {
        let victim_idx = victim_pick % n;
        let stream: Vec<EpisodeRecord> = (0..n)
            .map(|i| episode(&format!("ep-{i:03}"), &format!("Episode number {i} recorded verbatim.")))
            .collect();
        let victim_id = stream[victim_idx].id.clone();

        // Plain ingest: the reference graph.
        let plain = SqliteStore::open_in_memory().expect("open plain");
        ingest::persist_stream(&plain, &stream);
        let mut expected: Vec<String> = stream.iter().map(|e| e.id.clone()).collect();
        expected.retain(|id| *id != victim_id);
        expected.sort();

        // Ingest, delete, then re-ingest the same stream shuffled.
        let tomb = SqliteStore::open_in_memory().expect("open tomb");
        let outcomes = ingest::persist_stream(&tomb, &stream);
        prop_assert!(
            outcomes.iter().all(|o| matches!(o, Ok(IngestOutcome::Persisted))),
            "seed ingest persists everything"
        );
        tomb.delete_episode(&victim_id, 1_700_000_000)
            .expect("delete of a claim-free episode succeeds");

        for round in 0..2u64 {
            let mut resubmitted = stream.clone();
            shuffle_with_seed(&mut resubmitted, seed ^ (0xDEAD_BEEF ^ round));
            let outcomes = ingest::persist_stream(&tomb, &resubmitted);
            let mut rejections = 0usize;
            for outcome in &outcomes {
                match outcome {
                    Ok(IngestOutcome::AlreadyPersisted) => {}
                    Ok(IngestOutcome::Rejected {
                        reason: RejectReason::DeletedId,
                    }) => rejections += 1,
                    other => panic!("unexpected outcome on round {round}: {other:?}"),
                }
            }
            prop_assert_eq!(
                rejections, 1,
                "exactly the deleted id is rejected, every re-submission"
            );
        }

        prop_assert_eq!(
            persisted_ids(&tomb),
            expected,
            "the graph equals plain ingest minus the deleted episode — no resurrection"
        );
    }
}

// ---- ic_deleted_recreate (p_deleted_recreate): without revive every
// submission (unchanged or mutated) is rejected and the graph is unchanged;
// revive alone changes nothing; the first post-revive submission persists
// exactly once. ----

proptest! {
    #[test]
    fn p_deleted_recreate(
        n in 1usize..6,
        victim_pick in 0usize..8,
        corrected in proptest::bool::ANY,
    ) {
        let victim_idx = victim_pick % n;
        let stream: Vec<EpisodeRecord> = (0..n)
            .map(|i| episode(&format!("ep-{i:03}"), &format!("Episode number {i} recorded verbatim.")))
            .collect();
        let victim_id = stream[victim_idx].id.clone();

        let db = SqliteStore::open_in_memory().expect("open");
        ingest::persist_stream(&db, &stream);
        db.delete_episode(&victim_id, 1_700_000_000)
            .expect("delete succeeds");
        let after_delete = persisted_ids(&db);

        // Without revive: the unchanged submission is rejected.
        let unchanged = stream[victim_idx].clone();
        assert_deleted_rejection(&ingest::persist(&db, &unchanged));
        prop_assert_eq!(
            persisted_ids(&db),
            after_delete.clone(),
            "a rejected submission writes nothing"
        );

        // Without revive: the corrected (mutated) submission is rejected too.
        if corrected {
            let mut mutated = stream[victim_idx].clone();
            mutated.text = "mutated after deletion, still refused.".into();
            assert_deleted_rejection(&ingest::persist(&db, &mutated));
            prop_assert_eq!(
                persisted_ids(&db),
                after_delete.clone(),
                "mutation cannot bypass the tombstone"
            );
        }

        // Revive alone changes nothing.
        db.revive_episode(&victim_id).expect("revive of a tombstoned id succeeds");
        prop_assert_eq!(persisted_ids(&db), after_delete, "revive alone writes nothing");

        // The tombstone is spent: reviving again is an honest error.
        prop_assert!(
            matches!(db.revive_episode(&victim_id), Err(StoreError::NotTombstoned { .. })),
            "a cleared tombstone cannot be cleared again"
        );

        // The first post-revive submission persists through the normal path
        // exactly once; the immediate re-submission is a no-op.
        let outcome = ingest::persist(&db, &unchanged).expect("persist ok");
        prop_assert_eq!(outcome, IngestOutcome::Persisted, "first post-revive submit persists");
        let again = ingest::persist(&db, &unchanged).expect("persist ok");
        prop_assert_eq!(again, IngestOutcome::AlreadyPersisted, "second submit is the idempotent no-op");
        prop_assert_eq!(persisted_ids(&db).len(), n, "the stream is whole again — exactly once");
    }
}

// ---- delete refusal (ic_deleted_tombstone: "permitted only when no claim
// supports it, per gm_lineage_survives"). ----

#[test]
fn delete_is_refused_while_a_claim_supports_the_episode() {
    let db = SqliteStore::open_in_memory().expect("open");
    let supported = episode("ep-supported", "alpha beta gamma delta epsilon zeta.");
    let free = episode("ep-free", "Another episode with no claims over it.");
    ingest::persist_stream(&db, &[supported, free]);
    db.insert_claim(
        &ClaimNode {
            text: "alpha beta gamma".into(),
            valid_at: None,
            invalid_at: None,
            data_cutoff: Some("2026-01-01".into()),
            status: ClaimStatus::Staged,
            scope: "workspace:dev".into(),
            source_type: "episode".into(),
            evidence: Evidence::Span {
                text: "alpha beta gamma".into(),
                locator: "heading:Notes".into(),
            },
        },
        &[Lineage {
            episode_id: "ep-supported".into(),
            extractor_version: "0.1.0".into(),
        }],
        "The parser resolves spans deterministically.",
    )
    .expect("claim staged");

    let refused = db.delete_episode("ep-supported", 1_700_000_000);
    match refused {
        Err(StoreError::DeleteRefused { episode_id, .. }) => assert_eq!(episode_id, "ep-supported"),
        other => panic!("expected DeleteRefused, got {other:?}"),
    }
    assert!(
        persisted_ids(&db).contains(&"ep-supported".to_string()),
        "a refused deletion leaves the episode in place"
    );

    db.delete_episode("ep-free", 1_700_000_000)
        .expect("the claim-free episode deletes");
    assert_eq!(persisted_ids(&db), vec!["ep-supported".to_string()]);
}

// ---- ic_deleted_recreate: "no automated path (re-ingest, batch replay,
// dump-recreate rebuild) clears a tombstone" — the rebuild half. ----

#[test]
fn tombstone_survives_dump_recreate_and_gates_the_rebuilt_store() {
    let db = SqliteStore::open_in_memory().expect("open");
    let survivor = episode("ep-survivor", "alpha beta gamma delta epsilon zeta.");
    let deleted = episode("ep-deleted", "Another episode, deleted before the dump.");
    ingest::persist_stream(&db, &[survivor.clone(), deleted.clone()]);
    db.delete_episode("ep-deleted", 1_700_000_000)
        .expect("delete succeeds");

    let dump = db.dump_extraction_output().expect("dump read");
    assert!(
        dump.episode_tombstones
            .iter()
            .any(|t| t.episode_id == "ep-deleted"),
        "the dump carries the deletion tombstone"
    );

    // Rebuild: episodes first (the other half of the rebuild pair) — only
    // the SURVIVING episode's row re-enters; the deleted episode has no
    // claims, so nothing of it is restored except the tombstone itself.
    let fresh = SqliteStore::open_in_memory().expect("open fresh");
    ingest::persist_stream(&fresh, std::slice::from_ref(&survivor));
    fresh.restore_extraction_output(&dump).expect("restore ok");

    // The tombstone gates the rebuilt store: a re-submission of the
    // deleted id cannot resurrect the row (`ic_deleted_recreate` — the
    // dump-recreate path never clears a tombstone).
    assert_deleted_rejection(&ingest::persist(&fresh, &deleted));
    assert_eq!(persisted_ids(&fresh), vec!["ep-survivor".to_string()]);

    // The operator path still works on the rebuilt store: revive, then the
    // id re-enters the normal acceptance path.
    fresh
        .revive_episode("ep-deleted")
        .expect("revive works after rebuild");
    let outcome = ingest::persist(&fresh, &deleted).expect("persist ok");
    assert_eq!(outcome, IngestOutcome::Persisted);
}

// ---- delete of an unknown id is an honest error, never a silent ok. ----

#[test]
fn delete_of_an_unknown_episode_is_an_honest_error() {
    let db = SqliteStore::open_in_memory().expect("open");
    assert!(matches!(
        db.delete_episode("ep-never-ingested", 1_700_000_000),
        Err(StoreError::EpisodeNotFound { .. })
    ));
}
