//! Purpose: red-first tests for the deterministic claim atomizer
//! (bajan-5zy) — the upgraded DEFAULT no-LLM extractor. Responsibilities:
//! UAX #29 sentence-boundary pre-split (unicode-segmentation, pinned),
//! grouping into atomic claim-sized units (merge below min length, split
//! run-ons above max at semicolon connectors — simple and documented),
//! verbatim-span evidence per unit, byte-stable `legacy` whole-episode
//! mode behind config, cache identity distinct from the legacy proposer.
//! Rationale: makes the offline/free path produce atomic claims
//! (`ex_no_llm_post`-compliant, fully deterministic) and provides the
//! deterministic baseline the LLM extractor is evaluated against
//! (eval.claims content-equivalence scoring).

use bajan::extract::{
    Extractor, ExtractorConfig, SentenceAtomizer, atomic_claim_units, atomic_default_version,
};
use bajan::ingest::{self, EpisodeRecord, Locator, SourceMeta};
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

/// Whitespace-stripped form: the "modulo whitespace" equivalence used by
/// the re-concatenation property — every whitespace character removed from
/// both sides, then compared exactly.
fn ws_stripped(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Staged claim texts after a run.
fn staged_texts(db: &SqliteStore) -> Vec<String> {
    db.claims_with_lineage()
        .expect("claims read back")
        .into_iter()
        .filter(|(_, node, _)| node.status == ClaimStatus::Staged)
        .map(|(_, node, _)| node.text)
        .collect()
}

// --- default selection ----------------------------------------------------

/// The default (no env) extractor is now the deterministic sentence
/// atomizer: kind `atomic`, distinct cache version, no model id.
#[test]
fn default_config_selects_the_atomizer() {
    let config = ExtractorConfig::from_env_with(&|_| None);
    assert_eq!(config.kind, "atomic");
    let extractor = config.select().expect("atomic selects");
    assert_eq!(extractor.version(), atomic_default_version());
    assert!(
        extractor.version().starts_with("atomic-"),
        "atomizer cache version must be distinct from the legacy proposer's \
         plain package version: {}",
        extractor.version()
    );
    assert_eq!(extractor.model_id(), None, "deterministic: no model id");
}

/// `legacy` stays selectable and byte-stable: the whole-episode proposer,
/// exactly one candidate carrying the full episode text.
#[test]
fn legacy_kind_stays_selectable_and_byte_stable() {
    let config = ExtractorConfig::from_env_with(&lookup("BAJAN_EXTRACTOR", "legacy"));
    let extractor = config.select().expect("legacy selects");
    let ep = episode("ep-1", "First part here. Second part here too.");
    let candidates = extractor.propose(&ep).expect("propose");
    assert_eq!(candidates.len(), 1, "legacy: one whole-episode candidate");
    assert_eq!(
        candidates[0].text, ep.text,
        "byte-stable whole-episode text"
    );
}

fn lookup<'a>(key: &'a str, value: &'a str) -> impl Fn(&str) -> Option<String> + 'a {
    move |k: &str| (k == key).then(|| value.to_string())
}

// --- sentence splitting corpora -------------------------------------------

/// Abbreviations: UAX #29 splits `Dr.` as a short sentence — the merge
/// rule reattaches it to the following sentence. Decimals (`3.5`) never
/// split mid-number.
#[test]
fn abbreviations_merge_decimals_stay_intact() {
    let text = "Dr. Smith went to Washington. He arrived at 3.5 p.m. on time.";
    let units = atomic_claim_units(text);
    assert_eq!(
        units,
        vec![
            "Dr. Smith went to Washington.".to_string(),
            "He arrived at 3.5 p.m. on time.".to_string(),
        ]
    );
}

/// A trailing short sentence merges back into the previous unit (no
/// orphan fragments below the minimum length).
#[test]
fn trailing_short_sentence_merges_back() {
    let text = "The value is 3.5 units. A second sentence.";
    let units = atomic_claim_units(text);
    assert_eq!(units, vec![text.to_string()]);
}

/// A trailing sentence long enough to stand alone stays its own unit.
#[test]
fn trailing_long_sentence_stays_separate() {
    let text = "The value is 3.5 units. A second sentence arrived later.";
    let units = atomic_claim_units(text);
    assert_eq!(
        units,
        vec![
            "The value is 3.5 units.".to_string(),
            "A second sentence arrived later.".to_string(),
        ]
    );
}

/// Non-English text (EDGE-001 locale-bias note): ¿¡ punctuation, accents —
/// sentence splitting is script-neutral and units stay verbatim.
#[test]
fn non_english_sentence_terminators_split() {
    let text = "¿Cómo estás? ¡Muy bien! Español.";
    let units = atomic_claim_units(text);
    assert_eq!(
        units,
        vec![text.to_string()],
        "all fragments below min: one merged unit"
    );
    assert!(text.contains(&units[0]));
}

/// A lone short sentence stays a unit (nothing to merge with).
#[test]
fn lone_short_sentence_stays_a_unit() {
    assert_eq!(atomic_claim_units("Go."), vec!["Go.".to_string()]);
}

/// Run-on sentences above the max length split at semicolon connectors —
/// never mid-word; the semicolon stays with the left part; the
/// concatenation of the parts equals the sentence modulo whitespace.
#[test]
fn run_on_sentences_split_at_semicolons() {
    let clause = "x".repeat(110);
    let text = format!("{clause}; {clause}; {clause}");
    let units = atomic_claim_units(&text);
    assert_eq!(units.len(), 3, "two semicolon splits: {units:?}");
    assert!(
        units[0].ends_with(';'),
        "semicolon stays with the left part"
    );
    assert!(
        units[1].ends_with(';'),
        "semicolon stays with the left part"
    );
    for unit in &units {
        assert!(text.contains(unit.as_str()), "verbatim substring: {unit:?}");
    }
    let rejoined: String = units.concat();
    assert_eq!(ws_stripped(&rejoined), ws_stripped(&text));
}

/// A run-on without semicolons stays whole — splitting is best-effort,
/// never a hard cut mid-word (documented honest behavior).
#[test]
fn run_on_without_semicolons_stays_whole() {
    let text = format!("{}.", "x".repeat(400));
    assert_eq!(atomic_claim_units(&text), vec![text.clone()]);
}

// --- properties ------------------------------------------------------------

proptest! {
    /// Every unit is a verbatim substring of the episode text — and the
    /// units re-concatenate to the episode text modulo whitespace
    /// (ic_verbatim: atomized claims never paraphrase).
    #[test]
    fn p_units_reconcatenate_and_stay_verbatim(
        sentences in proptest::collection::vec(
            "[a-záéíóúñüß]{5,60}[.!¡?¿]?", 1..7,
        ),
        ws in proptest::collection::vec("[ \t\n]{1,3}", 0..6),
    ) {
        let mut text = String::new();
        for (i, s) in sentences.iter().enumerate() {
            text.push_str(s);
            text.push_str(". ");
            if i < ws.len() {
                text.push_str(&ws[i]);
            }
        }
        let units = atomic_claim_units(&text);
        prop_assert!(!units.is_empty() || text.trim().is_empty());
        let rejoined: String = units.iter().map(String::as_str).collect();
        prop_assert_eq!(ws_stripped(&rejoined), ws_stripped(&text));
        for unit in &units {
            prop_assert!(text.contains(unit.as_str()), "verbatim substring: {unit:?}");
        }
    }

    /// Determinism / idempotence: the same episode text always yields the
    /// same units — the atomizer is a pure function (no time, no order
    /// dependence, no state).
    #[test]
    fn p_atomizer_is_idempotent(
        s1 in "[a-z]{5,60}",
        s2 in "[a-z]{5,60}",
        s3 in "[a-z]{5,60}",
        ws in "[ \t\n]{1,3}",
    ) {
        let text = format!("{s1}. {ws}{s2}. {ws}{s3}.");
        let a = atomic_claim_units(&text);
        let b = atomic_claim_units(&text);
        prop_assert_eq!(a, b);
    }

    /// Every atomized candidate passes the typed gate's whitespace-collapsed
    /// containment (`ex_evidence_containment`) — verbatim substrings never
    /// need repair, and never drop hedge markers outside their coverage.
    #[test]
    fn p_atomic_claims_pass_the_typed_gate(
        s1 in "[a-z]{10,60}",
        s2 in "[a-z]{10,60}",
        hedge in "[a-z]{4,10}",
    ) {
        let text = format!("{s1} is {hedge} here. {s2} is simply false.");
        let units = atomic_claim_units(&text);
        for unit in &units {
            let evidence = Evidence::Span {
                text: unit.clone(),
                locator: "heading:H".into(),
            };
            prop_assert_eq!(
                bajan::extract::check_evidence_containment(&evidence, &text),
                Ok(()),
                "verbatim substring passes containment"
            );
        }
    }
}

// --- integration through run_extract ---------------------------------------

/// The default extraction pass now stages atomic claims: one per
/// claim-sized unit, not one per episode (the smoke-test gap closed:
/// 2 episodes → 2 whole-text claims becomes N atomic claims).
#[test]
fn run_extract_stages_atomic_claims_by_default() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(
        &db,
        &episode("ep-1", "The first claim is here. The second claim lives."),
    )
    .expect("persist");
    let report = bajan::extract::run_extract(&db, &mut Default::default()).expect("extract");
    assert_eq!(report.episodes_processed, 1);
    assert_eq!(report.candidates_proposed, 2, "two atomic claims, not one");
    assert_eq!(
        staged_texts(&db),
        vec![
            "The first claim is here.".to_string(),
            "The second claim lives.".to_string(),
        ]
    );
}

/// Hedge markers inside a unit survive (verbatim substring); markers in
/// other sentences are outside the span's coverage — the gate never
/// refuses atomized claims for hedging elsewhere in the episode.
#[test]
fn atomic_claims_never_drop_hedge_markers() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(
        &db,
        &episode("ep-1", "Alpha is maybe true here. Beta is simply false."),
    )
    .expect("persist");
    let report =
        bajan::extract::run_extract(&db, &mut Default::default()).expect("extract succeeds");
    assert!(
        report.gate_rejections.is_empty(),
        "substring evidence never drops in-coverage markers: {:?}",
        report.gate_rejections
    );
    assert_eq!(report.candidates_proposed, 2);
    let texts = staged_texts(&db);
    assert!(texts.contains(&"Alpha is maybe true here.".to_string()));
    assert!(texts.contains(&"Beta is simply false.".to_string()));
}

/// Episodes with a typed absent locator yield atomic claims with the
/// typed absent evidence marker (alignment failed; text still atomized).
#[test]
fn absent_locator_yields_unknown_evidence_atomic_claims() {
    let db = SqliteStore::open(":memory:").expect("store");
    let mut ep = episode("ep-1", "The first claim is here. The second claim lives.");
    ep.locator = Locator::Absent;
    ingest::persist(&db, &ep).expect("persist");
    let report = bajan::extract::run_extract(&db, &mut Default::default()).expect("extract");
    assert_eq!(report.candidates_proposed, 2);
    let claims = db.claims_with_lineage().expect("read back");
    for (_, node, _) in &claims {
        assert!(
            matches!(node.evidence, Evidence::Unknown),
            "absent locator projects Unknown evidence, not an invented span"
        );
    }
}

/// Empty episode text (defensive): zero candidates — legitimately
/// extracted and cached as empty (`ex_typed_gate`).
#[test]
fn empty_episode_text_yields_zero_candidates() {
    let atomizer = SentenceAtomizer::new("test");
    let mut ep = episode("ep-1", "   ");
    ep.text = "   ".into();
    let candidates = atomizer.propose(&ep).expect("propose");
    assert!(candidates.is_empty());
}

/// Cache economics: the atomizer's version keys the cache — a second run
/// at the same version processes nothing (ex_single_call).
#[test]
fn same_version_rerun_hits_the_cache() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(
        &db,
        &episode("ep-1", "The first claim is here. The second claim lives."),
    )
    .expect("persist");
    bajan::extract::run_extract(&db, &mut Default::default()).expect("first");
    let report = bajan::extract::run_extract(&db, &mut Default::default()).expect("second");
    assert_eq!(
        report.episodes_processed, 0,
        "cache hit at the atomizer version"
    );
    assert_eq!(staged_texts(&db).len(), 2);
}

/// The atomizer's version differs from the legacy proposer's plain
/// package version: switching modes re-extracts every episode and
/// tombstones prior-version staged claims (ex_supersession) — the
/// default upgrade must not silently reuse legacy extraction output.
#[test]
fn mode_switch_reextracts_and_supersedes() {
    let db = SqliteStore::open(":memory:").expect("store");
    ingest::persist(
        &db,
        &episode("ep-1", "The first claim is here. The second claim lives."),
    )
    .expect("persist");
    // Legacy proposer at the plain package version (the pre-upgrade default).
    let legacy_report = bajan::extract::run_extract_versioned(
        &db,
        env!("CARGO_PKG_VERSION"),
        &mut Default::default(),
    )
    .expect("legacy extract");
    assert_eq!(legacy_report.candidates_proposed, 1);
    // The default pass now runs the atomizer under its own version key.
    let report = bajan::extract::run_extract(&db, &mut Default::default()).expect("atomic extract");
    assert_eq!(
        report.episodes_processed, 1,
        "distinct version: re-extracts"
    );
    assert_eq!(
        report.superseded, 1,
        "legacy whole-episode claim tombstoned"
    );
    assert_eq!(report.candidates_proposed, 2);
    let live: Vec<String> = db
        .claims_with_lineage()
        .expect("read back")
        .into_iter()
        .filter(|(_, node, _)| node.status == ClaimStatus::Staged)
        .map(|(_, node, _)| node.text)
        .collect();
    assert_eq!(
        live,
        vec![
            "The first claim is here.".to_string(),
            "The second claim lives.".to_string(),
        ]
    );
}
