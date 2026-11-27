# bajan/two-store-implementations-agree — the Vec model and SQLite store accept and reject identically

Baseline: extends `antithesis/scratchbook/property-catalog.md` (superproject `../../antithesis/scratchbook/`) and `../../antithesis/scratchbook/properties/bajan.md`. Evidence re-verified against bajan `c63dcdc` (catalog snapshot was `2ee3045`; line numbers below are current).

## Invariant statement

Over any sequence of the shared four operations — `insert`/`insert_claim`, `supersede_prior_versions`, `stage_invalidation_proposal`, `adopt` — applied with identical arguments to `ClaimStore` and `SqliteStore`:

1. Ok/Err verdicts agree for every operation.
2. Per-claim status agrees (`ClaimStore::nodes()` vs `SqliteStore::claims_with_lineage()`).
3. `supersessions()` and `invalidations()` return equal records (same order, same fields).
4. `audit()` returns equal audit records; after any `adopt`, the adopted claim has exactly one audit row in both stores.

Error-precedence (which `Err` wins when several guards fail) and multi-lineage handling are the suspected divergence points, per the baseline evidence file.

## Check mode

In-process. `ClaimStore` is a pure Vec model (`src/store/mod.rs:45-231`); `SqliteStore::open_in_memory()` per case (`src/store/sqlite.rs:683`). No subprocess, no filesystem, no crash injection. Maps directly to a Hegel `#[hegel::state_machine]` with shrink by operation prefix; in proptest terms, a `proptest-state-machine`-style driver.

## Seam

Public crate API: `bajan::store::ClaimStore` (`mod.rs:60,118,151,189`) and `bajan::store::SqliteStore` (`sqlite.rs:1192…` — `insert_claim:1161`, `stage_invalidation_proposal:1855`, `supersede_prior_versions:1943`, `adopt:2795`, projections `invalidations:1916`, `supersessions:1980`, `audit:2834`, `claims_with_lineage:1262`). No test seam needed. Note `adopt_batch` exists only on the model (`mod.rs:220`) — the alphabet is the shared four ops only.

## Generator sketch

`Action` enum: `Insert { text, status, lineage_len 0..=3, evidence kind }`, `Supersede { claim_index, reason, at }`, `StageInvalidation { claim_index, episode_id }`, `Adopt { claim_index, actor, at }`; sequences of 0..=40 steps. Indices drawn from currently-live keys, plus deliberate out-of-range indices to compare refusal behavior. Texts include empty, newline, quote-bearing strings. Claim inputs to `supersede` must be drawn from prior successful inserts to reach the interesting state; the machine must track applied keys per store (keys can diverge — see candidate 2).

## Predicted red/green

Predicted **green** on the narrowed four-op alphabet — no confirmed defect today; the only code claim of equivalence is prose (`sqlite.rs:1932-1934`, `tests/resolve.rs:361-366`), and no existing test applies one op sequence to both stores. Predicted **red** if the alphabet is widened beyond the shared ops (model-only `adopt_batch`, multi-lineage inserts — the model keeps exactly one lineage per node). Divergence outside the shared alphabet is invisible by construction; the artifact must state the oracle's scope (eval report §6, "Differential properties narrowed to shared ops").

## Existing coverage / migration overlap

None of the 22 `proptest!` blocks in `tests/` (15 files) runs a differential; this candidate is new coverage. This is the only stateful-machine candidate in the crate and the only migration candidate from the handwritten suite (`Cargo.toml:35` pins `proptest = "1"`, no state-machine crate). Eval-report open question 3 ("port one bajan differential machine?") points here.
