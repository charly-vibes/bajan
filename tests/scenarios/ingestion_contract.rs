//! ingestion.contract scenario tests for properties without honest existing
//! coverage: `p_date_fidelity`, `p_id_canon`, `p_unique_id` (interleaved
//! streams), and `p_batch_resume`.
//!
//! Purpose: scenario-scoped contract tests (bajan-16o).
//! Responsibilities: exactly one test per property id below.
//! Rationale: predicates quoted from specs/ingestion-contract.md Properties.

use bajan::ingest::{self, EpisodeRecord, IngestOutcome, Locator, RejectReason, SourceMeta};
use bajan::store::sqlite::SqliteStore;

fn episode(id: &str, text: &str, cutoff: Option<&str>) -> EpisodeRecord {
    EpisodeRecord {
        id: id.into(),
        text: text.into(),
        locator: Locator::Span("heading:Notes".into()),
        source: SourceMeta {
            source_type: "episode".into(),
            data_cutoff: cutoff.map(|c| c.into()),
            authority_tier: 2,
            tags: vec!["workspace:dev".into()],
        },
    }
}

fn persisted_ids(db: &SqliteStore) -> Vec<String> {
    let mut ids: Vec<String> = db
        .episodes()
        .expect("read episodes")
        .into_iter()
        .map(|e| e.id)
        .collect();
    ids.sort();
    ids
}

/// p_date_fidelity — stored cutoff equals source cutoff when present, the
/// explicit absent marker otherwise; never ingest time.
#[test]
fn p_date_fidelity() {
    let db = SqliteStore::open_in_memory().expect("store");
    let records = vec![
        episode("ep-dated", "alpha beta gamma.", Some("2024-05-06")),
        episode("ep-undated", "delta epsilon zeta.", None),
    ];
    let outcomes = ingest::persist_stream(&db, &records);
    assert!(
        outcomes
            .iter()
            .all(|o| matches!(o, Ok(IngestOutcome::Persisted)))
    );

    let mut stored = db.episodes().expect("read episodes");
    stored.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(stored[0].id, "ep-dated");
    assert_eq!(stored[0].source.data_cutoff, Some("2024-05-06".into()));
    assert_eq!(stored[1].id, "ep-undated");
    assert_eq!(
        stored[1].source.data_cutoff, None,
        "absent stays the typed absent marker — never an ingest-time value"
    );
}

/// p_id_canon — normalization-variant ids (NFC vs NFD) resolve to one
/// episode idempotently; empty or whitespace-only ids are rejected.
#[test]
fn p_id_canon() {
    let db = SqliteStore::open_in_memory().expect("store");
    let nfc = "caf\u{e9}"; // é precomposed
    let nfd = "cafe\u{301}"; // e + combining acute

    let first = ingest::persist(&db, &episode(nfc, "alpha beta gamma.", Some("2024-05-06")))
        .expect("persist ok");
    assert!(matches!(first, IngestOutcome::Persisted));

    let variant = ingest::persist(&db, &episode(nfd, "alpha beta gamma.", Some("2024-05-06")))
        .expect("persist ok");
    assert!(
        matches!(variant, IngestOutcome::AlreadyPersisted),
        "NFD variant of a persisted NFC id must not create a second episode"
    );
    assert_eq!(db.episodes().expect("read").len(), 1);

    for bad in ["", "   "] {
        let out = ingest::persist(&db, &episode(bad, "alpha beta gamma.", Some("2024-05-06")))
            .expect("persist returns outcome, not error");
        assert!(matches!(
            out,
            IngestOutcome::Rejected {
                reason: RejectReason::MissingId
            }
        ));
    }
    assert_eq!(db.episodes().expect("read").len(), 1);
}

/// p_unique_id — exactly one persisted episode per stable id; the
/// interleaved final graph equals the sequential one.
#[test]
fn p_unique_id() {
    let stream: Vec<EpisodeRecord> = (1..=6)
        .map(|i| {
            episode(
                &format!("e{i}"),
                &format!("text {i} alpha."),
                Some("2024-05-06"),
            )
        })
        .collect();

    // Sequential: one uninterrupted submission.
    let sequential = SqliteStore::open_in_memory().expect("store");
    let outcomes = ingest::persist_stream(&sequential, &stream);
    assert!(
        outcomes
            .iter()
            .all(|o| matches!(o, Ok(IngestOutcome::Persisted)))
    );

    // Interleaved: alternating halves across three submissions, with an
    // overlap re-submission in the middle.
    let interleaved = SqliteStore::open_in_memory().expect("store");
    for batch in [
        vec![stream[0].clone(), stream[3].clone()],
        vec![stream[1].clone(), stream[4].clone(), stream[0].clone()],
        vec![stream[2].clone(), stream[5].clone()],
    ] {
        let outcomes = ingest::persist_stream(&interleaved, &batch);
        for (i, o) in outcomes.iter().enumerate() {
            let expected = if i == 2 {
                IngestOutcome::AlreadyPersisted
            } else {
                IngestOutcome::Persisted
            };
            assert_eq!(
                o.as_ref().unwrap(),
                &expected,
                "overlap re-submission is the only no-op"
            );
        }
    }

    assert_eq!(
        persisted_ids(&sequential),
        persisted_ids(&interleaved),
        "the interleaved final graph equals the sequential one"
    );
    assert_eq!(persisted_ids(&interleaved).len(), 6);
}

/// p_batch_resume — streams interrupted after each k of N episodes, then
/// fully re-submitted, end equal to a single uninterrupted submission; no
/// episode is half-persisted.
#[test]
fn p_batch_resume() {
    let stream: Vec<EpisodeRecord> = (1..=6)
        .map(|i| {
            episode(
                &format!("e{i}"),
                &format!("resume {i} beta."),
                Some("2024-05-06"),
            )
        })
        .collect();

    let uninterrupted = SqliteStore::open_in_memory().expect("store");
    ingest::persist_stream(&uninterrupted, &stream);

    // Interrupted after 3, then the full stream re-submitted.
    let resumed = SqliteStore::open_in_memory().expect("store");
    ingest::persist_stream(&resumed, &stream[..3]);
    let outcomes = ingest::persist_stream(&resumed, &stream);
    let resumed_count: usize = outcomes
        .iter()
        .map(|o| matches!(o, Ok(IngestOutcome::AlreadyPersisted)) as usize)
        .sum();
    assert_eq!(
        resumed_count, 3,
        "exactly the already-persisted prefix is a no-op; the rest persists"
    );

    assert_eq!(
        persisted_ids(&uninterrupted),
        persisted_ids(&resumed),
        "the resumed final graph equals the uninterrupted one"
    );
}
