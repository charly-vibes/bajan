//! Purpose: vertical-slice property tests (bajan-6hz) — the thin path
//! ingest→extract→SQLite→first query, governed by the specodelic specs.
//! Responsibilities: red-first property tests for `gm_embedded_store`
//! (dump-then-recreate), `ic_malformed`/`ic_unique_id`/`ic_idempotent`
//! (ingest outcomes), and `qt_bounded_traversal`/`qt_lineage_traceable`/
//! `qt_readonly` (the first read query).
//! Rationale: each test derives from a specodelic property (p_embedded_store,
//! p_malformed, p_idempotent, p_bounded_traversal, p_lineage_traceable,
//! p_readonly); written red before the implementation.

use bajan::ingest::{self, EpisodeRecord, IngestOutcome, Locator, RejectReason, SourceMeta};
use bajan::query::{self, BudgetStatus};
use bajan::store::sqlite::SqliteStore;
use bajan::store::{ClaimNode, ClaimStatus, Evidence, Lineage, StoreError};
use proptest::prelude::*;

/// Episode text used by the search tests: hedge-free, and a prefix of it
/// serves as claim text so the containment gate passes without hedging.
const EPISODE_TEXT: &str = "alpha beta gamma delta epsilon zeta.";

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

/// A staged candidate claim over `EPISODE_TEXT`: text is a whitespace-
/// collapsed-contained span of the episode (passes the typed gate), and
/// carries no hedge marker (nothing to drop).
fn staged_claim(episode_id: &str) -> (ClaimNode, Lineage) {
    (
        ClaimNode {
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
        Lineage {
            episode_id: episode_id.into(),
            extractor_version: "0.1.0".into(),
        },
    )
}

// ---- gm_embedded_store (p_embedded_store): dump-then-recreate from the
// episode stream plus extraction output reproduces the graph exactly. ----

proptest! {
    #[test]
    fn dump_then_recreate_reproduces_the_graph(
        ids in proptest::collection::vec("[a-z0-9-]{3,12}", 1..5),
        // bajan-15i generator extension: alongside the original long
        // ASCII shape, episodes now include SHORT texts (EDGE-002:
        // simhash is unreliable below a length threshold — exact-match
        // territory) and NON-ENGLISH texts (EDGE-001: UAX #29 is the
        // language-neutral default; Spanish/German diacritics exercise
        // it). None of the branches can produce whitespace-only text,
        // so `ic_malformed` never rejects a generated episode.
        texts in proptest::collection::vec(
            prop_oneof![
                // Long ASCII (original shape).
                "[a-z0-9 .,!?-]{10,60}",
                // Short episode, incl. accented characters.
                "[a-záéíóúñüß]{1,5}",
                // Non-English (Spanish/German diacritics), long.
                "[a-záéíóúñA-ZÁÉÍÓÚÑÜÄäÖöß .,!?-]{10,60}",
            ],
            1..5,
        ),
    ) {
        let db = SqliteStore::open_in_memory().expect("open");
        let n = ids.len().min(texts.len());
        for i in 0..n {
            let record = EpisodeRecord {
                id: ids[i].clone(),
                text: texts[i].clone(),
                ..episode("seed", "")
            };
            ingest::persist(&db, &record).expect("persist");
            let (mut node, lineage) = staged_claim(&ids[i]);
            node.text = texts[i].clone();
            node.evidence = Evidence::Span {
                text: texts[i].clone(),
                locator: "heading:Notes".into(),
            };
            db.insert_claim(&node, &[lineage], &texts[i]).expect("insert");
        }

        // Dump: the episode stream plus extraction output.
        let stream = ingest::dump_stream(&db).expect("stream read");
        let extraction = db.dump_extraction_output().expect("dump read");

        // Recreate a fresh store from the dumps alone.
        let fresh = SqliteStore::open_in_memory().expect("open");
        for record in &stream {
            ingest::persist(&fresh, record).expect("re-persist");
        }
        fresh
            .restore_extraction_output(&extraction)
            .expect("restore succeeds");

        prop_assert_eq!(fresh.episodes().expect("episodes read").len(), n);
        prop_assert_eq!(
            fresh.dump_extraction_output().expect("fresh dump read"),
            db.dump_extraction_output().expect("db dump read")
        );
        for (i, text) in texts.iter().take(n).enumerate() {
            prop_assert_eq!(fresh.claim_text(i).expect("claim text read"), Some(text.clone()));
        }
    }
}

// ---- ic_malformed: malformed records are rejected, never persisted ----

proptest! {
    #[test]
    fn malformed_records_are_rejected_never_persisted(
        whitespace in " \t\n{1,5}",
    ) {
        let db = SqliteStore::open_in_memory().expect("open");

        // Whitespace-only verbatim text → rejected (ic_malformed).
        let no_text = EpisodeRecord {
            text: whitespace,
            ..episode("ep-x", "")
        };
        let outcome = ingest::persist(&db, &no_text);
        let rejected_text = matches!(outcome, Ok(IngestOutcome::Rejected { .. }));
        prop_assert!(rejected_text);

        // Empty or whitespace-only id is malformed too (ic_id_canon).
        let no_id = EpisodeRecord {
            id: "  ".into(),
            ..episode("", "real text")
        };
        let outcome = ingest::persist(&db, &no_id);
        let rejected_id = matches!(outcome, Ok(IngestOutcome::Rejected { .. }));
        prop_assert!(rejected_id);
    }
}

// ---- ic_unique_id / ic_idempotent: re-ingest converges, never duplicates ----

#[test]
fn reingest_is_idempotent_and_never_duplicates() {
    let db = SqliteStore::open_in_memory().expect("open");
    let good = episode("ep-ok", "Cache hits are cheap.");
    assert!(matches!(
        ingest::persist(&db, &good),
        Ok(IngestOutcome::Persisted)
    ));
    // ic_mutated_resubmit: same id, mutated text → rejected with a
    // machine-readable conflict reason — never updated in place, never
    // duplicated (updating would orphan claims whose lineage text no
    // longer matches).
    let mutated = episode("ep-ok", "mutated text");
    assert!(matches!(
        ingest::persist(&db, &mutated),
        Ok(IngestOutcome::Rejected {
            reason: RejectReason::Conflict,
        })
    ));
    assert_eq!(db.episodes().expect("episodes read").len(), 1);
    assert_eq!(
        db.episodes().expect("episodes read")[0].text,
        "Cache hits are cheap."
    );
    // ic_idempotent: an *unchanged* re-submission is still already_persisted.
    assert!(matches!(
        ingest::persist(&db, &good),
        Ok(IngestOutcome::AlreadyPersisted)
    ));
    assert_eq!(db.episodes().expect("episodes read").len(), 1);
}

// ---- ic_mutated_resubmit (p_mutated_resubmit): every mutation class —
// altered text, altered locator, altered metadata — is rejected with the
// conflict reason; the graph before and after is identical. ----

#[test]
fn mutated_resubmissions_are_rejected_with_conflict() {
    let db = SqliteStore::open_in_memory().expect("open");
    let original = episode("ep-ok", "Cache hits are cheap.");
    assert!(matches!(
        ingest::persist(&db, &original),
        Ok(IngestOutcome::Persisted)
    ));

    // Altered text.
    let altered_text = episode("ep-ok", "mutated text");
    assert!(matches!(
        ingest::persist(&db, &altered_text),
        Ok(IngestOutcome::Rejected {
            reason: RejectReason::Conflict,
        })
    ));
    // Altered locator.
    let mut altered_locator = episode("ep-ok", "Cache hits are cheap.");
    altered_locator.locator = Locator::Span("heading:Other".into());
    assert!(matches!(
        ingest::persist(&db, &altered_locator),
        Ok(IngestOutcome::Rejected {
            reason: RejectReason::Conflict,
        })
    ));
    // Altered source metadata.
    let mut altered_meta = episode("ep-ok", "Cache hits are cheap.");
    altered_meta.source.authority_tier = 3;
    assert!(matches!(
        ingest::persist(&db, &altered_meta),
        Ok(IngestOutcome::Rejected {
            reason: RejectReason::Conflict,
        })
    ));

    // The graph is unchanged and never duplicated through all rejections.
    assert_eq!(db.episodes().expect("episodes read").len(), 1);
    assert_eq!(db.episodes().expect("episodes read")[0], original);
}

// ---- ic_batch_duplicate (p_batch_duplicate): within one submitted
// stream, the first occurrence of a stable id persists and every later
// occurrence is rejected with a duplicate reason — per record, never
// extending to other episodes in the batch. ----

#[test]
fn intra_batch_duplicate_is_first_wins() {
    let db = SqliteStore::open_in_memory().expect("open");
    let first = episode("ep-dup", "first payload");
    let second = episode("ep-dup", "second payload differs");
    let other = episode("ep-other", "An unaffected episode.");
    let results = ingest::persist_stream(&db, &[first, second, other]);
    assert!(matches!(results[0], Ok(IngestOutcome::Persisted),));
    assert!(matches!(
        results[1],
        Ok(IngestOutcome::Rejected {
            reason: RejectReason::Duplicate,
        }),
    ));
    assert!(matches!(results[2], Ok(IngestOutcome::Persisted),));
    // Exactly the first payload persisted; the second never overwrote it.
    assert_eq!(db.episodes().expect("episodes read").len(), 2);
    let persisted = db
        .get_episode("ep-dup")
        .expect("episode read")
        .expect("ep-dup persisted");
    assert_eq!(persisted.text, "first payload");
}

// ---- ic_corrected_resubmit (p_corrected_resubmit): a rejected id may be
// re-submitted corrected; it persists through the normal acceptance path
// exactly once. ----

#[test]
fn corrected_resubmission_persists_through_normal_path() {
    let db = SqliteStore::open_in_memory().expect("open");
    let malformed = episode("ep-fix", "   ");
    assert!(matches!(
        ingest::persist(&db, &malformed),
        Ok(IngestOutcome::Rejected {
            reason: RejectReason::MissingText,
        })
    ));
    assert_eq!(
        db.episodes().expect("episodes read").len(),
        0,
        "rejected never persisted"
    );

    let corrected = episode("ep-fix", "Cache hits are cheap.");
    assert!(matches!(
        ingest::persist(&db, &corrected),
        Ok(IngestOutcome::Persisted)
    ));
    assert_eq!(
        db.episodes().expect("episodes read").len(),
        1,
        "persists exactly once"
    );
}

// ---- ic_order_insensitive (p_order_insensitive): any episode order of
// the same stream yields an identical graph, fresh and as re-ingest. ----

/// Fisher-Yates over a xorshift64* PRNG — dependency-free deterministic
/// shuffle seeded by the proptest-generated `seed`.
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

/// The graph content without insertion-order claim keys: (node, lineage)
/// pairs sorted — key numbering is an implementation detail of row order.
fn claims_multiset(db: &SqliteStore) -> Vec<(bajan::store::ClaimNode, Vec<bajan::store::Lineage>)> {
    let mut claims: Vec<_> = db
        .claims_with_lineage()
        .expect("claims read")
        .into_iter()
        .map(|(_, node, lineage)| (node, lineage))
        .collect();
    // ClaimNode/Lineage derive Eq but not Ord; the deterministic JSON form
    // is a total order (struct field order is fixed by the schema).
    claims.sort_by_key(|pair| serde_json::to_string(pair).expect("claim serializes"));
    claims
}

proptest! {
    #[test]
    fn order_insensitive_ingest_yields_identical_graph(
        n in 1usize..6,
        seed in proptest::num::u64::ANY,
    ) {
        // Distinct ids: intra-batch first-wins would make "identical graph"
        // ill-defined across orders when payloads differ.
        let records: Vec<EpisodeRecord> = (0..n)
            .map(|i| episode(&format!("ep-{i:03}"), &format!("Episode number {i} recorded verbatim.")))
            .collect();
        let mut shuffled = records.clone();
        shuffle_with_seed(&mut shuffled, seed);

        let a = SqliteStore::open_in_memory().expect("open a");
        let b = SqliteStore::open_in_memory().expect("open b");
        for record in &records {
            ingest::persist(&a, record).expect("persist a");
        }
        for record in &shuffled {
            ingest::persist(&b, record).expect("persist b");
        }
        // Re-ingest the shuffled stream over the already-ingested store.
        for record in &shuffled {
            ingest::persist(&a, record).expect("re-ingest a");
        }

        let mut runs_a = bajan::extract::ExtractionRunStore::default();
        let mut runs_b = bajan::extract::ExtractionRunStore::default();
        bajan::extract::run_extract_versioned(&a, "0.1.0", &mut runs_a).expect("extract a");
        bajan::extract::run_extract_versioned(&b, "0.1.0", &mut runs_b).expect("extract b");

        prop_assert_eq!(a.episodes().expect("episodes a"), b.episodes().expect("episodes b"));
        prop_assert_eq!(claims_multiset(&a), claims_multiset(&b));
        let mut extracted_a = a.dump_extraction_output().expect("dump a").extracted;
        let mut extracted_b = b.dump_extraction_output().expect("dump b").extracted;
        extracted_a.sort();
        extracted_b.sort();
        prop_assert_eq!(extracted_a, extracted_b);
    }
}

// ---- extract: the thin deterministic proposer feeds the typed gate ----

#[test]
fn end_to_end_ingest_extract_then_first_query() {
    let db = SqliteStore::open_in_memory().expect("open");
    ingest::persist(&db, &episode("ep-a", EPISODE_TEXT)).expect("persist");

    let mut runs = bajan::extract::ExtractionRunStore::default();
    let report = bajan::extract::run_extract_versioned(&db, "0.1.0", &mut runs).expect("extract");
    assert_eq!(report.episodes_processed, 1);
    assert_eq!(report.candidates_proposed, 1, "one deterministic candidate");
    assert!(
        report.gate_rejections.is_empty(),
        "no refusals on a hedge-free episode"
    );

    // bajan-r1h (ex_run_record wired): one run row per extraction call.
    let rows: Vec<_> = runs.rows().collect();
    assert_eq!(rows.len(), 1, "exactly one run row per call");
    assert_eq!(rows[0].episode_id, "ep-a");
    assert_eq!(rows[0].finish, bajan::extract::Finish::Succeeded);

    // The proposed claim is staged and searchable (lineage-traceable).
    let result = query::search(&db, "alpha", 100).expect("search");
    assert!(matches!(result.status, BudgetStatus::Complete));
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].status, ClaimStatus::Staged);
    assert_eq!(result.hits[0].episodes, vec!["ep-a".to_string()]);

    // Same-version re-extract is a cache hit: no new work, no new claims.
    let mut again_runs = bajan::extract::ExtractionRunStore::default();
    let again =
        bajan::extract::run_extract_versioned(&db, "0.1.0", &mut again_runs).expect("re-extract");
    assert_eq!(again.episodes_processed, 0, "cache hit: no new work");
    assert_eq!(
        again_runs.rows().count(),
        0,
        "cache hit: no new calls, no new rows"
    );
    assert_eq!(query::search(&db, "alpha", 100).unwrap().hits.len(), 1);
}

// ---- bajan-r1h: infrastructure failures are never gate rejections ----

// A store-level failure (read-only database) must propagate as an error
// instead of being swallowed into the report's gate rejections: a SQL
// failure is not a typed-gate refusal, and the report must not lie about
// the outcome class (`ex_run_record`: failure outcomes carry honest,
// machine-readable reasons). The episode also stays pending — retryable
// at the same version — because a failed call must not cache.
#[test]
fn infrastructure_failure_propagates_and_episode_stays_retryable() {
    let dir = std::env::temp_dir().join(format!("bajan-r1h-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let db_path = dir.join("ro.db");

    {
        let db = SqliteStore::open(db_path.to_str().expect("utf8 path")).expect("open");
        ingest::persist(&db, &episode("ep-ro", EPISODE_TEXT)).expect("persist");
    }
    // Freeze the database file so extraction-time writes fail at the SQL
    // layer (infrastructure), not at the typed gate.
    let mut perms = std::fs::metadata(&db_path).expect("stat").permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o444);
    std::fs::set_permissions(&db_path, perms).expect("chmod");

    let db = SqliteStore::open(db_path.to_str().expect("utf8 path")).expect("reopen");
    assert_eq!(
        db.pending_episodes("0.1.0").expect("pending read").len(),
        1,
        "episode pending before the run"
    );

    let mut runs = bajan::extract::ExtractionRunStore::default();
    let outcome = bajan::extract::run_extract_versioned(&db, "0.1.0", &mut runs);
    assert!(
        outcome.is_err(),
        "infrastructure failures propagate; they are never reported as gate rejections"
    );
    assert_eq!(
        db.pending_episodes("0.1.0").expect("pending read").len(),
        1,
        "a failed call never caches the episode: retryable at the same version"
    );

    // Restore permissions so the temp file is removable.
    let mut perms = std::fs::metadata(&db_path).expect("stat").permissions();
    perms.set_mode(0o644);
    std::fs::set_permissions(&db_path, perms).expect("chmod back");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- bajan-2sj: corrupt-store reads surface as StoreError, never panics ----

// A corrupted tags column (raw SQL UPDATE to a non-JSON value) must make
// the read path return `StoreError::Sqlite` carrying specs/graph-model.md
// instead of aborting the process with a panic — every invocation emits
// the suite envelope (bajan-aan/bajan-ts6 single-output-format contract),
// and a panic on a read path bypasses it. Corrupts with raw SQL because
// the store API itself never writes non-JSON tags.
#[test]
fn corrupt_store_reads_error_instead_of_panic() {
    let dir = std::env::temp_dir().join(format!("bajan-2sj-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let db_path = dir.join("corrupt.db");
    {
        let db = SqliteStore::open(db_path.to_str().expect("utf8 path")).expect("open");
        ingest::persist(&db, &episode("ep-c", EPISODE_TEXT)).expect("persist");
    }
    let conn = rusqlite::Connection::open(&db_path).expect("reopen raw");
    conn.execute("UPDATE episodes SET tags = 'not json'", [])
        .expect("corrupt tags column");

    let db = SqliteStore::open(db_path.to_str().expect("utf8 path")).expect("reopen");
    let err = db
        .episodes()
        .expect_err("corrupt tags read errors instead of panicking");
    match err {
        StoreError::Sqlite { message, spec } => {
            assert_eq!(spec, "specs/graph-model.md");
            assert!(!message.is_empty(), "the decode failure is in the message");
        }
        other => panic!("expected StoreError::Sqlite, got {other:?}"),
    }

    // Downstream read paths propagate the same store error: extraction
    // (pending_episodes) and any caller over episodes()/claims read the
    // corrupt rows.
    let mut runs = bajan::extract::ExtractionRunStore::default();
    assert!(
        bajan::extract::run_extract_versioned(&db, "0.1.0", &mut runs).is_err(),
        "extraction over a corrupt store errors, never panics"
    );

    // Restore permissions so the temp file is removable.
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- qt_lineage_traceable + qt_bounded_traversal + qt_readonly ----

proptest! {
    #[test]
    fn search_is_lineage_traceable_and_bounded(
        lineaged in proptest::collection::vec(proptest::bool::ANY, 1..8),
    ) {
        let db = SqliteStore::open_in_memory().expect("open");
        ingest::persist(&db, &episode("ep-a", EPISODE_TEXT)).expect("persist");
        for &live in lineaged.iter() {
            let (node, mut lineage) = staged_claim("ep-a");
            if !live {
                // Lineage-broken: its only edge points at a deleted episode.
                lineage.episode_id = "ep-deleted".into();
            }
            db.insert_claim(&node, &[lineage], EPISODE_TEXT).expect("insert");
        }
        let total = lineaged.len();
        let live_count = lineaged.iter().filter(|&&l| l).count();

        // qt_lineage_traceable: lineage-broken claims are invisible; every
        // returned claim resolves to at least one persisted episode.
        let full = query::search(&db, "alpha", total).expect("search");
        prop_assert_eq!(full.hits.len(), live_count);
        for hit in &full.hits {
            prop_assert!(!hit.episodes.is_empty());
        }

        // qt_bounded_traversal: a truncated run reports the typed marker
        // deterministically, never a silent partial answer.
        if total > 1 {
            let bounded = query::search(&db, "alpha", 1).expect("search");
            prop_assert!(matches!(bounded.status, BudgetStatus::BudgetExhausted));
        }

        // qt_readonly: no state transition, audit, or edge written.
        let before = db.dump_extraction_output().expect("dump read");
        let _ = query::search(&db, "alpha", total);
        prop_assert_eq!(db.dump_extraction_output().expect("dump read"), before);
    }
}

#[test]
fn not_found_is_honest_when_budget_completes_without_match() {
    let db = SqliteStore::open_in_memory().expect("open");
    ingest::persist(&db, &episode("ep-a", "plain text only.")).expect("persist");
    let result = query::search(&db, "absent", 100).expect("search");
    assert!(matches!(result.status, BudgetStatus::Complete));
    assert!(
        result.hits.is_empty(),
        "not-found is a complete, empty answer"
    );
}
