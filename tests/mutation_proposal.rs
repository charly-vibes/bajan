//! Purpose: red-first tests for `ex_mutation_proposal` wiring into the
//! extract path (bajan-c4p) — a re-extraction whose new output conflicts
//! with an existing ACTIVE claim stages an invalidation proposal citing
//! the causing episode; the active claim is never mutated.
//! Responsibilities: pin the conflict predicate operationally — both
//! sides carry `Span` evidence at the SAME locator (same evidence
//! position) and the whitespace-collapsed CLAIM texts DIFFER;
//! `Evidence::Unknown` never conflicts (no position); staged prior
//! claims keep the supersession path (no proposal); a duplicate
//! (claim, causing-episode) proposal on a later conflicting pass is an
//! idempotent skip, never an infrastructure failure.
//! Rationale: `ex_mutation_proposal` names the invariant but nothing in
//! the wired pipeline called `stage_invalidation_proposal` — extract is
//! the event source ("the re-extraction output conflicts"), the store
//! stays the only writer.

use bajan::extract::{self, CandidateClaim, Extractor};
use bajan::ingest::{self, EpisodeRecord, Locator, SourceMeta};
use bajan::store::sqlite::SqliteStore;
use bajan::store::{ClaimStatus, Evidence};

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

/// A scripted extractor: one fixed candidate per call, tagged with a
/// version. Enough to drive a version-bump re-extraction deterministically.
#[derive(Debug)]
struct ScriptedExtractor {
    version: String,
    candidate: CandidateClaim,
}

impl Extractor for ScriptedExtractor {
    fn version(&self) -> &str {
        &self.version
    }
    fn model_id(&self) -> Option<&str> {
        None
    }
    fn propose(
        &self,
        _episode: &EpisodeRecord,
    ) -> Result<Vec<CandidateClaim>, extract::ExtractionFailure> {
        Ok(vec![self.candidate.clone()])
    }
}

fn span_candidate(text: &str, evidence: &str, locator: &str) -> CandidateClaim {
    CandidateClaim {
        text: text.into(),
        valid_at: None,
        invalid_at: None,
        evidence: Evidence::Span {
            text: evidence.into(),
            locator: locator.into(),
        },
    }
}

/// Seed one episode, extract it at v1 with the given candidate, and
/// adopt the resulting claim. Returns the active claim key.
fn seed_active_claim(db: &SqliteStore, candidate: &CandidateClaim) -> usize {
    ingest::persist(db, &episode("ep-1", "The sky is green today.")).expect("persist");
    let runs = &mut extract::ExtractionRunStore::default();
    let report = run_at(db, "v1", candidate.clone(), runs);
    assert_eq!(report.candidates_proposed, 1);
    let key = db
        .claims_with_lineage()
        .expect("claims")
        .first()
        .expect("one claim")
        .0;
    db.adopt(key, "tester", 1_000).expect("adopt");
    key
}

fn run_at(
    db: &SqliteStore,
    version: &str,
    candidate: CandidateClaim,
    runs: &mut extract::ExtractionRunStore,
) -> extract::ExtractReport {
    let version = version.to_string();
    let ex = ScriptedExtractor { version, candidate };
    extract::run_extract_with(db, &ex, runs).expect("extract pass")
}

#[test]
fn active_conflict_stages_invalidation_proposal() {
    let db = SqliteStore::open_in_memory().expect("db");
    // v1: active claim "The sky is green", evidence at span:0
    let key = seed_active_claim(
        &db,
        &span_candidate("The sky is green", "The sky is green", "span:0"),
    );
    // v2: conflicting output at the SAME evidence position — same
    // verbatim span, different claim text (evidence containment still
    // holds: the span IS in the episode text).
    let runs = &mut extract::ExtractionRunStore::default();
    let report = run_at(
        &db,
        "v2",
        span_candidate("The sky is red", "The sky is green", "span:0"),
        runs,
    );

    // The proposal exists, citing the causing episode.
    let inv = db.invalidations().expect("invalidations");
    assert_eq!(inv.len(), 1, "exactly one invalidation proposal");
    assert_eq!(inv[0].claim_key, key, "against the active claim");
    assert_eq!(
        inv[0].causing_episode_id, "ep-1",
        "citing the causing episode"
    );
    // The report counts it.
    assert_eq!(report.proposals_staged, 1, "reported on the extract report");
    // The active claim is never mutated (ex_mutation_proposal).
    let node = &db.claims_with_lineage().expect("claims")[0].1;
    assert_eq!(node.status, ClaimStatus::Active, "active claim untouched");
}

#[test]
fn same_position_same_text_no_proposal() {
    let db = SqliteStore::open_in_memory().expect("db");
    let key = seed_active_claim(
        &db,
        &span_candidate("The sky is green", "The sky is green", "span:0"),
    );
    // v2: same position, whitespace-variant of the same claim text — a
    // re-statement, not a conflict.
    let runs = &mut extract::ExtractionRunStore::default();
    let report = run_at(
        &db,
        "v2",
        span_candidate("The  sky is\tgreen ", "The sky is green", "span:0"),
        runs,
    );
    assert!(
        db.invalidations().expect("invalidations").is_empty(),
        "collapsed-equal text never conflicts"
    );
    assert_eq!(report.proposals_staged, 0);
    let _ = key;
}

#[test]
fn different_position_no_proposal() {
    let db = SqliteStore::open_in_memory().expect("db");
    seed_active_claim(
        &db,
        &span_candidate("The sky is green", "The sky is green", "span:0"),
    );
    // v2: different text at a DIFFERENT position — new claim territory,
    // not a mutation conflict.
    let runs = &mut extract::ExtractionRunStore::default();
    let report = run_at(
        &db,
        "v2",
        span_candidate("The sea is red", "The sky is green", "span:1"),
        runs,
    );
    assert!(
        db.invalidations().expect("invalidations").is_empty(),
        "different evidence position never conflicts"
    );
    assert_eq!(report.proposals_staged, 0);
}

#[test]
fn unknown_evidence_never_conflicts() {
    let db = SqliteStore::open_in_memory().expect("db");
    seed_active_claim(
        &db,
        &span_candidate("The sky is green", "The sky is green", "span:0"),
    );
    // v2: typed-absent marker has no position — never a conflict.
    let runs = &mut extract::ExtractionRunStore::default();
    let report = run_at(
        &db,
        "v2",
        CandidateClaim {
            text: "The sky is red".into(),
            valid_at: None,
            invalid_at: None,
            evidence: Evidence::Unknown,
        },
        runs,
    );
    assert!(
        db.invalidations().expect("invalidations").is_empty(),
        "Evidence::Unknown carries no position"
    );
    assert_eq!(report.proposals_staged, 0);
}

#[test]
fn staged_prior_claim_superseded_not_proposed() {
    let db = SqliteStore::open_in_memory().expect("db");
    // v1 stays STAGED (no adopt).
    ingest::persist(&db, &episode("ep-1", "The sky is green today.")).expect("persist");
    let runs = &mut extract::ExtractionRunStore::default();
    run_at(
        &db,
        "v1",
        span_candidate("The sky is green", "The sky is green", "span:0"),
        runs,
    );
    // v2 conflicts with it.
    let report = run_at(
        &db,
        "v2",
        span_candidate("The sky is red", "The sky is green", "span:0"),
        runs,
    );
    // Supersession semantics unchanged: staged prior version tombstoned,
    // NO invalidation proposal (staged claims are refused by the
    // active-only guard — the pipeline must not even try).
    assert_eq!(report.superseded, 1, "staged claim superseded as before");
    assert!(
        db.invalidations().expect("invalidations").is_empty(),
        "staged claims go through supersession, not mutation proposals"
    );
}

#[test]
fn duplicate_proposal_on_later_conflicting_pass_is_idempotent() {
    let db = SqliteStore::open_in_memory().expect("db");
    seed_active_claim(
        &db,
        &span_candidate("The sky is green", "The sky is green", "span:0"),
    );
    let runs = &mut extract::ExtractionRunStore::default();
    run_at(
        &db,
        "v2",
        span_candidate("The sky is red", "The sky is green", "span:0"),
        runs,
    );
    assert_eq!(db.invalidations().expect("inv").len(), 1);
    // v3 conflicts with the SAME active claim again: the (claim,
    // causing-episode) pair already exists — idempotent skip, the pass
    // succeeds, still exactly one proposal.
    let report = run_at(
        &db,
        "v3",
        span_candidate("The sky is blue", "The sky is green", "span:0"),
        runs,
    );
    assert_eq!(report.proposals_staged, 0, "duplicate not re-counted");
    assert_eq!(db.invalidations().expect("inv").len(), 1, "still one row");
}
