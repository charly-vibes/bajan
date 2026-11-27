//! Property: `bajan/insert-claim-key-addresses-the-written-row` (bajan-llg).
//!
//! Purpose: pin the invariant that `SqliteStore::insert_claim` returns the
//! key of the row it just wrote, on both dense and sparse key spaces.
//! Responsibilities: one sparse red fixture (keys {5, 7}, ignored, bug
//! bajan-trd), one generated-gap red variant (ignored, bug bajan-trd), and
//! a live dense-control `#[hegel::test]` that must stay green.
//! Rationale: key allocation is `MAX(claim_key)+1` but the returned key is
//! `claim_count()-1` (src/store/sqlite.rs:1192,1208) — these agree only on
//! the dense prefix 0..n-1; on sparse keys the function returns a key that
//! names a different row (or no row) and attaches lineage to it. Sparse
//! keys are a legitimate persisted state because `restore_extraction_output`
//! carries `claim_key` verbatim (src/store/sqlite.rs:1547).

use bajan::store::sqlite::{DumpedClaim, ExtractionDump, SqliteStore};
use bajan::store::{ClaimNode, ClaimStatus, Evidence, Lineage};
use hegel::generators as gs;

// ---- fixture helpers (shape follows tests/write_path_discipline.rs) ----

fn node_with(text: &str) -> ClaimNode {
    ClaimNode {
        text: text.into(),
        valid_at: None,
        invalid_at: None,
        data_cutoff: None,
        status: ClaimStatus::Staged,
        scope: "workspace:dev".into(),
        source_type: "episode".into(),
        evidence: Evidence::Span {
            text: text.into(),
            locator: "heading:Notes".into(),
        },
    }
}

fn lineage_with(episode_id: &str) -> Lineage {
    Lineage {
        episode_id: episode_id.into(),
        extractor_version: "0.1.0".into(),
    }
}

/// A dump whose claims carry exactly the given non-dense keys; sparse keys
/// are legitimate: restore writes `claim_key` verbatim.
fn sparse_dump(keys: &[usize]) -> ExtractionDump {
    let claims = keys
        .iter()
        .map(|&k| DumpedClaim {
            claim_key: k,
            node: node_with(&format!("sparse claim {k}")),
            lineage: vec![lineage_with(&format!("ep-sparse-{k}"))],
        })
        .collect();
    ExtractionDump {
        claims,
        extracted: vec![],
        edges: vec![],
        invalidations: vec![],
        supersessions: vec![],
        reviews: vec![],
        review_audits: vec![],
        er_aliases: vec![],
        contradiction_proposals: vec![],
        contradiction_audits: vec![],
        episode_tombstones: vec![],
        coverage_declarations: vec![],
        coverage_declared_ids: vec![],
    }
}

fn restored(keys: &[usize]) -> SqliteStore {
    let db = SqliteStore::open_in_memory().expect("open");
    db.restore_extraction_output(&sparse_dump(keys))
        .expect("restore");
    db
}

/// The three clauses of `insert-claim-key-addresses-the-written-row`:
/// (a) exactly one claims row has the returned key and it holds the node
/// just written; (b) `lineage_of(k)` is exactly the passed lineage; (c) no
/// persisted claim has zero lineage rows.
fn assert_key_addresses_row(
    db: &SqliteStore,
    returned_key: usize,
    node: &ClaimNode,
    lineage: &[Lineage],
) {
    let all = db.claims_with_lineage().expect("read claims");

    // (a) the returned key names exactly one row, and that row is the
    // one just written (same text/evidence).
    let matching: Vec<&(usize, ClaimNode, Vec<Lineage>)> = all
        .iter()
        .filter(|(key, _, _)| *key == returned_key)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "clause (a): returned key {returned_key} must name exactly one claims row"
    );
    assert_eq!(
        matching[0].1.text, node.text,
        "clause (a): the row at {returned_key} holds the text just written"
    );
    assert_eq!(
        matching[0].1.evidence, node.evidence,
        "clause (a): the row at {returned_key} holds the evidence just written"
    );

    // (b) every lineage edge produced by this insert references k.
    let mut seen = db.lineage_of(returned_key).expect("read lineage");
    seen.sort_by(|l, r| {
        (&l.episode_id, &l.extractor_version).cmp(&(&r.episode_id, &r.extractor_version))
    });
    let mut expected: Vec<Lineage> = lineage.to_vec();
    expected.sort_by(|l, r| {
        (&l.episode_id, &l.extractor_version).cmp(&(&r.episode_id, &r.extractor_version))
    });
    assert_eq!(
        seen, expected,
        "clause (b): lineage_of({returned_key}) must be exactly the passed episodes"
    );

    // (c) no persisted claim ends with zero lineage rows.
    let empty: Vec<usize> = all
        .iter()
        .filter(|(_, _, lin)| lin.is_empty())
        .map(|(key, _, _)| *key)
        .collect();
    assert!(
        empty.is_empty(),
        "clause (c): persisted claims with zero lineage rows: {empty:?}"
    );
}

// ---- sparse red fixture ----

// gm_embedded_store says sparse keys are a real persisted state, so the
// returned key must name the written row there too. Today it returns
// claim_count()-1 (= 2) while the row is written at MAX+1 (= 8): a red
// defect, not a fix — see bajan-trd.
#[test]
#[ignore = "bug bajan-trd: insert_claim returns claim_count()-1 on sparse keys"]
fn sparse_keys_returned_key_names_the_written_row() {
    let db = restored(&[5, 7]);
    let node = node_with("fresh claim on sparse keys");
    let lineage = vec![lineage_with("ep-fresh-a")];
    let k = db
        .insert_claim(&node, &lineage, "alpha beta gamma delta epsilon zeta.")
        .expect("insert");
    assert_key_addresses_row(&db, k, &node, &lineage);
}

// ---- generated-gap red variant (deterministic gap shapes) ----

// Same invariant over a spread of gap shapes — including a gap that
// collides with claim_count()-1, where lineage would attach to another
// claim's row. Each shape gets 1..=3 inserts, checked after each.
#[test]
#[ignore = "bug bajan-trd: insert_claim returns claim_count()-1 on sparse keys"]
fn generated_gap_shapes_returned_key_names_the_written_row() {
    let shapes: [&[usize]; 3] = [&[0, 2], &[10, 20, 30], &[3, 5, 7, 9]];
    for shape in shapes {
        let db = restored(shape);
        for i in 1..=3 {
            let node = node_with(&format!("fresh claim {i} on {shape:?}"));
            let lineage = vec![lineage_with(&format!("ep-fresh-{shape:?}-{i}"))];
            let k = db
                .insert_claim(&node, &lineage, "alpha beta gamma delta epsilon zeta.")
                .expect("insert");
            assert_key_addresses_row(&db, k, &node, &lineage);
        }
    }
}

// ---- dense control (live, NOT ignored) ----

// insert_claim_key_identity_dense_control: on a fresh dense store every
// insert's returned key is the written row's key and lineage lands on it.
// Predicted green: on the dense prefix 0..n-1, MAX+1 == claim_count()-1,
// which is why the existing suite never caught the sparse defect.
#[hegel::test]
fn dense_control_returned_key_names_the_written_row(tc: hegel::TestCase) {
    let texts: Vec<String> = tc.draw(gs::vecs(gs::text().max_size(64)).min_size(1).max_size(20));
    let db = SqliteStore::open_in_memory().expect("open");
    for (i, text) in texts.iter().enumerate() {
        let node = node_with(text);
        let lineage = vec![lineage_with(&format!("ep-dense-{i}"))];
        let k = db
            .insert_claim(&node, &lineage, "alpha beta gamma delta epsilon zeta.")
            .expect("insert");
        tc.note(&format!("insert {i}: returned key {k}"));
        assert_key_addresses_row(&db, k, &node, &lineage);
    }
}
