//! Purpose: red-first tests for the extractor seam (bajan-9av) — the
//! `Extractor` trait boundary between persisted episodes and candidate
//! proposals. Responsibilities: trait dispatch through `run_extract` with
//! a scripted fake extractor; all-or-nothing candidate persistence (a
//! partial batch is refused, nothing staged — the gate is per-episode);
//! the default extractor is the legacy deterministic proposer
//! (byte-stable candidates, `model_id: None`); model id reaches the run
//! rows when an extractor carries one; deterministic extractors cache
//! identically (one call per episode per version).
//! Rationale: `ex_single_call`/`ex_run_record` name the extractor's
//! contract but not its implementation — the seam makes "proposer
//! differs, contract does not" structural instead of incidental.

use bajan::extract::{self, CandidateClaim, ExtractionFailure, Extractor};
use bajan::ingest::{EpisodeRecord, Locator, SourceMeta};
use bajan::store::sqlite::SqliteStore;
use bajan::store::{ClaimStatus, Evidence};
use proptest::prelude::*;

fn episode(id: &str, text: &str) -> EpisodeRecord {
    EpisodeRecord {
        id: id.into(),
        text: text.into(),
        locator: Locator::Span("heading:H".into()),
        source: SourceMeta {
            source_type: "markdown".into(),
            data_cutoff: Some("2026-01-01".into()),
            authority_tier: 2,
            tags: vec!["workspace:dev".into()],
        },
    }
}

fn seed_two_episodes(db: &SqliteStore) {
    ingest::persist(db, &episode("ep-1", "First episode text. Second sentence.")).expect("persist");
    ingest::persist(db, &episode("ep-2", "Other episode text.")).expect("persist");
}
use bajan::ingest;

/// A scripted extractor: fixed output per episode id, optional model id.
/// Fails for ids in `fail_for` (infrastructure failure class).
#[derive(Debug)]
struct FakeExtractor {
    version: &'static str,
    model_id: Option<&'static str>,
    /// Output per episode id; missing id = infrastructure failure.
    outputs:
        std::collections::HashMap<&'static str, Result<Vec<CandidateClaim>, ExtractionFailure>>,
}

impl Extractor for FakeExtractor {
    fn version(&self) -> &str {
        self.version
    }
    fn model_id(&self) -> Option<&str> {
        self.model_id
    }
    fn propose(&self, episode: &EpisodeRecord) -> Result<Vec<CandidateClaim>, ExtractionFailure> {
        self.outputs
            .get(episode.id.as_str())
            .cloned()
            .unwrap_or_else(|| {
                Err(ExtractionFailure::Infrastructure(
                    "no script for this episode".into(),
                ))
            })
    }
}

fn scripted_claim(text: &str) -> CandidateClaim {
    CandidateClaim {
        text: text.into(),
        valid_at: None,
        invalid_at: None,
        evidence: Evidence::Span {
            text: text.into(),
            locator: "heading:H".into(),
        },
    }
}

/// Staged claim texts after a run (via the full read-back).
fn staged_texts(db: &SqliteStore) -> Vec<String> {
    db.claims_with_lineage()
        .expect("claims read back")
        .into_iter()
        .filter(|(_, node, _)| node.status == ClaimStatus::Staged)
        .map(|(_, node, _)| node.text)
        .collect()
}

// --- dispatch ------------------------------------------------------------

/// A scripted extractor's candidates reach the store through run_extract:
/// two atomic claims per episode, staged, with the extractor's version on
/// their lineage rows.
#[test]
fn scripted_extractor_dispatches_through_run_extract() {
    let db = SqliteStore::open(":memory:").expect("store");
    seed_two_episodes(&db);
    let extractor = FakeExtractor {
        version: "2.0.0",
        model_id: Some("test-model"),
        outputs: [
            (
                "ep-1",
                Ok(vec![
                    scripted_claim("First episode text."),
                    scripted_claim("Second sentence."),
                ]),
            ),
            ("ep-2", Ok(vec![scripted_claim("Other episode text.")])),
        ]
        .into_iter()
        .collect(),
    };
    let report = extract::run_extract_with(&db, &extractor, &mut Default::default())
        .expect("extract succeeds");
    assert_eq!(report.episodes_processed, 2);
    assert_eq!(report.candidates_proposed, 3);
    let texts = staged_texts(&db);
    assert!(texts.contains(&"First episode text.".to_string()));
    assert!(texts.contains(&"Second sentence.".to_string()));
    assert!(texts.contains(&"Other episode text.".to_string()));
}

/// The default extractor is the legacy deterministic proposer: one
/// whole-episode candidate per pending episode, `model_id: None` —
/// byte-stable with the pre-seam behavior.
#[test]
fn default_extractor_is_legacy_whole_episode_proposer() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(&db, &episode("ep-1", "Whole episode text.")).expect("persist");
    let report = extract::run_extract(&db, &mut Default::default()).expect("extract");
    assert_eq!(report.candidates_proposed, 1);
    assert_eq!(staged_texts(&db), vec!["Whole episode text.".to_string()]);
}

/// Model id reaches every run row the extractor's calls produced.
#[test]
fn model_id_lands_on_run_rows() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(&db, &episode("ep-1", "Text.")).expect("persist");
    let extractor = FakeExtractor {
        version: "2.0.0",
        model_id: Some("test-model-7"),
        outputs: [("ep-1", Ok(vec![scripted_claim("Text.")]))]
            .into_iter()
            .collect(),
    };
    let mut runs = extract::ExtractionRunStore::default();
    extract::run_extract_with(&db, &extractor, &mut runs).expect("extract");
    let rows: Vec<_> = runs.rows().collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].model_id.as_deref(), Some("test-model-7"));
    assert_eq!(rows[0].extractor_version, "2.0.0");
}

// --- all-or-nothing batch semantics --------------------------------------

/// A candidate batch where ONE claim fails the gate persists NOTHING for
/// that episode — no partial staging. The gate outcome is per-episode.
#[test]
fn one_gate_failure_in_batch_stages_nothing() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(&db, &episode("ep-1", "Good sentence. Bad ~hedge~ gone.")).expect("persist");
    // "Good sentence." is contained; "Bad hedge gone." drops the hedge
    // marker → the store refuses it. All-or-nothing: neither lands.
    let extractor = FakeExtractor {
        version: "2.0.0",
        model_id: None,
        outputs: [(
            "ep-1",
            Ok(vec![
                scripted_claim("Good sentence."),
                scripted_claim("Bad hedge gone."),
            ]),
        )]
        .into_iter()
        .collect(),
    };
    let report = extract::run_extract_with(&db, &extractor, &mut Default::default())
        .expect("extract run itself succeeds");
    // Episode is gate-rejected as a whole.
    assert_eq!(report.gate_rejections.len(), 1);
    assert_eq!(report.gate_rejections[0].episode_id, "ep-1");
    assert_eq!(report.candidates_proposed, 0);
    assert!(staged_texts(&db).is_empty(), "no partial staging");
}

/// An infrastructure failure (not a gate refusal) fails the whole run —
/// it must never be swallowed into the report as a rejection.
#[test]
fn infrastructure_failure_fails_the_run() {
    let db = SqliteStore::open(":memory:").expect("store");
    seed_two_episodes(&db);
    let extractor = FakeExtractor {
        version: "2.0.0",
        model_id: None,
        outputs: [].into_iter().collect(), // everything fails
    };
    let err = extract::run_extract_with(&db, &extractor, &mut Default::default())
        .expect_err("infrastructure failure propagates");
    assert!(err.to_string().contains("no script for this episode"));
}

// --- caching -------------------------------------------------------------

/// Same extractor version, re-run: no new calls (cache hit), no duplicate
/// claims — cache economics keyed on (episode id, extractor version) are
/// extractor-agnostic (`ex_single_call`).
#[test]
fn same_version_reuses_cache_extractor_agnostically() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(&db, &episode("ep-1", "Text.")).expect("persist");
    let make = || FakeExtractor {
        version: "2.0.0",
        model_id: Some("m"),
        outputs: [("ep-1", Ok(vec![scripted_claim("Text.")]))]
            .into_iter()
            .collect(),
    };
    extract::run_extract_with(&db, &make(), &mut Default::default()).expect("first");
    let report = extract::run_extract_with(&db, &make(), &mut Default::default()).expect("second");
    assert_eq!(report.episodes_processed, 0, "cache hit: no re-extraction");
    assert_eq!(staged_texts(&db).len(), 1);
}

/// A version bump re-extracts under the new extractor's version key —
/// even when the old version came from a different extractor.
#[test]
fn version_bump_reextracts_across_extractors() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(&db, &episode("ep-1", "Text.")).expect("persist");
    let first = FakeExtractor {
        version: "2.0.0",
        model_id: Some("m"),
        outputs: [("ep-1", Ok(vec![scripted_claim("Text.")]))]
            .into_iter()
            .collect(),
    };
    extract::run_extract_with(&db, &first, &mut Default::default()).expect("first");
    let second = FakeExtractor {
        version: "3.0.0",
        model_id: Some("other"),
        outputs: [("ep-1", Ok(vec![scripted_claim("Text.")]))]
            .into_iter()
            .collect(),
    };
    let report = extract::run_extract_with(&db, &second, &mut Default::default())
        .expect("re-extract under v3");
    assert_eq!(report.episodes_processed, 1);
    assert_eq!(
        report.superseded, 1,
        "prior-version staged claim tombstoned"
    );
}

// --- proptest ------------------------------------------------------------

proptest! {
    /// Cache-key identity is over (episode id, extractor version) pairs
    /// for ANY extractor version string: same version never re-extracts,
    /// different version always re-extracts (ex_single_call).
    #[test]
    fn p_cache_keyed_on_version_pair(
        v1 in "[0-9a-z.]{1,12}",
        v2 in "[0-9a-z.]{1,12}",
        episode_id in "[a-z0-9-]{3,16}",
    ) {
        prop_assume!(v1 != v2, "distinct versions required");
        let db = SqliteStore::open(":memory:").expect("store");
        bajan::ingest::persist(&db, &episode(&episode_id, "Body.")).expect("persist");
        // Map the generated episode id into the scripted outputs key.
        let make_for = |v: &str, id: String| FakeExtractor {
            version: Box::leak(v.to_string().into_boxed_str()) as &'static str,
            model_id: None,
            outputs: std::iter::once((
                Box::leak(id.into_boxed_str()) as &'static str,
                Ok(vec![scripted_claim("Body.")]),
            ))
            .collect(),
        };
        let r1 = extract::run_extract_with(&db, &make_for(&v1, episode_id.clone()), &mut Default::default())
            .expect("first extract");
        prop_assert_eq!(r1.episodes_processed, 1);
        let r2 = extract::run_extract_with(&db, &make_for(&v1, episode_id.clone()), &mut Default::default())
            .expect("same version rerun");
        prop_assert_eq!(r2.episodes_processed, 0, "same version: cache hit");
        let r3 = extract::run_extract_with(&db, &make_for(&v2, episode_id.clone()), &mut Default::default())
            .expect("bumped version");
        prop_assert_eq!(r3.episodes_processed, 1, "new version: re-extracts");
    }
}
