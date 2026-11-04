//! extraction.claims scenario tests for properties without honest existing
//! coverage: `p_no_llm_post` (deterministic post-pass, no LLM after the
//! extract step) and `p_reflection` (deterministic flagging, never
//! introduces claims).
//!
//! Purpose: scenario-scoped contract tests (bajan-16o).
//! Responsibilities: exactly one test per property id below.
//! Rationale: predicates quoted from specs/extraction-claims.md Properties.

use bajan::extract::{
    CandidateClaim, ExtractionFailure, ExtractionRunStore, Extractor, run_extract_with,
};
use bajan::ingest::{EpisodeRecord, Locator, SourceMeta};
use bajan::store::Evidence;
use bajan::store::sqlite::SqliteStore;
use std::collections::HashMap;

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

#[derive(Debug)]
/// Counts `propose` invocations: the property under test is that the count
/// never advances after the extract step (no LLM in the post-pass).
struct CountingExtractor {
    calls: std::cell::Cell<usize>,
}

impl Extractor for CountingExtractor {
    fn version(&self) -> &str {
        "0.1.0"
    }
    fn propose(&self, _episode: &EpisodeRecord) -> Result<Vec<CandidateClaim>, ExtractionFailure> {
        self.calls.set(self.calls.get() + 1);
        // A near-duplicate pair plus an unnormalized variant: the post-pass
        // (normalize/dedup) must decide deterministically without calling
        // back into the extractor.
        let dup = |t: &str| CandidateClaim {
            text: t.into(),
            valid_at: None,
            invalid_at: None,
            evidence: Evidence::Span {
                text: t.into(),
                locator: "heading:Notes".into(),
            },
        };
        Ok(vec![dup("alpha beta"), dup("alpha  beta")])
    }
}

/// p_no_llm_post — identical post-pass inputs produce identical outputs and
/// no LLM call occurs after the extraction step.
#[test]
fn p_no_llm_post() {
    let episodes = vec![
        episode("ep-1", "alpha beta gamma delta epsilon zeta."),
        episode("ep-2", "alpha beta gamma delta epsilon zeta."),
    ];
    let mut staged_a = Vec::new();
    let mut staged_b = Vec::new();
    for target in [&mut staged_a, &mut staged_b] {
        let db = SqliteStore::open_in_memory().expect("store");
        for e in &episodes {
            let out = bajan::ingest::persist(&db, e).expect("persist ok");
            assert!(matches!(out, bajan::ingest::IngestOutcome::Persisted));
        }
        let ex = CountingExtractor {
            calls: std::cell::Cell::new(0),
        };
        let mut runs = ExtractionRunStore::default();
        let report = run_extract_with(&db, &ex, &mut runs).expect("extract ok");
        // One extractor call per pending episode — the extract step only.
        assert_eq!(ex.calls.get(), report.episodes_processed);
        let nodes = db.claims_with_lineage().expect("read nodes");
        target.extend(nodes.iter().map(|(_, n, _)| n.text.clone()));
        assert_eq!(
            nodes.len(),
            report.candidates_proposed,
            "no claim is introduced or dropped after the extract step"
        );
    }
    assert_eq!(
        staged_a, staged_b,
        "identical post-pass inputs produce identical staged output"
    );
}

/// p_reflection — the deterministic reflection pass flags the typed-absent
/// evidence marker and never introduces new claims.
#[test]
fn p_reflection() {
    use bajan::extract::{ReflectionFlag, reflection_flags};

    // Real spans are never flagged; the typed absent marker yields exactly
    // one flag (the implemented advisory surface of `ex_reflection`).
    let span = Evidence::Span {
        text: "alpha beta gamma".into(),
        locator: "heading:Notes".into(),
    };
    assert!(reflection_flags(&span).is_empty());
    let unknown = reflection_flags(&Evidence::Unknown);
    assert_eq!(unknown.len(), 1);
    assert!(matches!(unknown[0], ReflectionFlag::UnknownEvidenceSpan));

    // Pipeline level: a candidate with typed-absent evidence stages exactly
    // one claim — the pass flags it, never repairs or duplicates it.
    let db = SqliteStore::open_in_memory().expect("store");
    let ep = episode("ep-absent", "alpha beta gamma delta epsilon zeta.");
    bajan::ingest::persist(&db, &ep).expect("persist ok");
    #[derive(Debug)]
    struct UnknownEvidenceExtractor;
    impl Extractor for UnknownEvidenceExtractor {
        fn version(&self) -> &str {
            "0.1.0"
        }
        fn propose(
            &self,
            _episode: &EpisodeRecord,
        ) -> Result<Vec<CandidateClaim>, ExtractionFailure> {
            Ok(vec![CandidateClaim {
                text: "alpha beta gamma".into(),
                valid_at: None,
                invalid_at: None,
                evidence: Evidence::Unknown,
            }])
        }
    }
    let mut runs = ExtractionRunStore::default();
    let report = run_extract_with(&db, &UnknownEvidenceExtractor, &mut runs).expect("extract ok");
    let nodes = db.claims_with_lineage().expect("read nodes");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes.len(), report.candidates_proposed);
    let _ = HashMap::<String, usize>::new(); // keep imports honest if shapes shift
}
