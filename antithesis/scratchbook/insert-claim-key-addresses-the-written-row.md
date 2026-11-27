# bajan/insert-claim-key-addresses-the-written-row — the returned key names the row that was written

Baseline: extends superproject `../../antithesis/scratchbook/property-catalog.md` and `../../antithesis/scratchbook/properties/bajan.md`. Evidence re-verified against bajan `c63dcdc`.

## Invariant statement

After `SqliteStore::insert_claim(node, lineage, episode)` returns `k`:

1. Exactly one row in `claims` has `claim_key == k`, and it is the row just written (same text/evidence).
2. Every lineage edge produced by this insert references `k` — i.e. `lineage_of(k)` contains the passed episodes.
3. No persisted claim ends with zero lineage rows.

The catalog's open question ("can a legitimate store hold non-contiguous claim keys?") is answered by the code itself: `restore_extraction_output` inserts `DumpedClaim.claim_key` **verbatim** (`src/store/sqlite.rs:1547`), so sparse keys are a legitimate persisted state, not a corrupt one.

## Check mode

In-memory SQLite (`SqliteStore::open_in_memory()`, `src/store/sqlite.rs:683`), in-process. Seed via `restore_extraction_output` with an `ExtractionDump` whose claims carry non-dense keys (e.g. 5 and 7); the restore fixture pattern is `tests/write_path_discipline.rs:172-193`. No subprocess.

## Seam

Public API only: `SqliteStore::restore_extraction_output` (`sqlite.rs:1530`), `SqliteStore::insert_claim` (`sqlite.rs:1161`), `SqliteStore::lineage_of` (`sqlite.rs:1250`), `SqliteStore::claims_with_lineage` (`sqlite.rs:1262`). `ExtractionDump` fields are public (`sqlite.rs:375-427`).

## Generator sketch

Two parts: (a) a fixed sparse fixture — dump with keys `{5, 7}`, then one `insert_claim`; (b) generated dumps with random distinct key sets (including dense `0..n-1` as the control case), random gap sizes, then 1..=5 generated inserts, checking the three clauses after each insert.

## Predicted red/green

Predicted **RED** today; defect confirmed by reading the code:

- Key allocation is `MAX(claim_key) + 1`: `VALUES ((SELECT COALESCE(MAX(claim_key), -1) + 1 FROM claims), …)` — `src/store/sqlite.rs:1192`.
- The returned key is `claim_count() - 1`: `let claim_key = self.claim_count()? - 1;` — `src/store/sqlite.rs:1208-1209`.
- That returned value is what lineage is attached to: `self.insert_lineage(claim_key, lineage)?` — `sqlite.rs:1209-1210`, inserting into `lineage (claim_key, …)` at `sqlite.rs:1220-1228`.

These agree only while keys are the dense prefix `0..n-1`. On the sparse fixture (keys `{5, 7}`): the new row gets `claim_key = 8`, but the function returns `3 - 1 = 2` and writes lineage for key `2` — a key that names no row (or, if a generated dump also contains key 2, a different claim's row). Clauses 1, 2, and 3 all fail. Dense-key control cases stay green, which is why the existing suite never caught it.

Deliverable is the plan only; no fix proposed here, no `src/` change.

## Existing coverage / migration overlap

No `proptest!` block covers key identity: `tests/evidence_refs.rs` uses the returned key but only on dense stores where `MAX+1 == COUNT-1`, so the defect is invisible there. The 22 existing blocks in `tests/` are otherwise disjoint from this candidate.
