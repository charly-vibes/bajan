# bajan/date-roundtrip-and-canonical-acceptance — dates round-trip and only canonical calendar dates parse

Baseline: extends superproject `../../antithesis/scratchbook/property-catalog.md` and `../../antithesis/scratchbook/properties/bajan.md`. Evidence re-verified against bajan `c63dcdc` (line numbers shifted from the catalog's `2ee3045` snapshot; re-cited below).

## Invariant statement

Two halves (the catalog's evaluation split the original single property):

1. **Exhaustive roundtrip (not PBT):** for every day `d` in `1..=9999`, `days_from_iso(iso_from_days(d)) == Some(d)`.
2. **Canonical acceptance (generated):** if `days_from_iso(s) == Some(d)` then `s == iso_from_days(d)` — the parser accepts *only* the canonical `YYYY-MM-DD` calendar form produced by `iso_from_days` (no `2024-02-30`, no `+026-01-01`). The doc contract is "malformed values are never guessed into a date" (`src/query.rs:59-61`).

## Check mode

Pure in-process (`bajan::query`), no I/O. Half 1 is a plain exhaustive loop (3,650,000 iterations — milliseconds); half 2 is the only randomized half and the property-based-test deliverable. No subprocess.

## Seam

Public: `bajan::query::iso_from_days` (`src/query.rs:44`), `bajan::query::days_from_iso` (`src/query.rs:62`). Both free functions, no test seam needed.

## Generator sketch

Half 2: arbitrary `String` restricted to length-10 byte strings whose bytes 4 and 7 are `-` (the pre-filter at `query.rs:64-65`; without it, most cases exit on the first branch and never reach the parsing logic), with digits, leading `+`/`-`, non-ASCII filler in the digit slots, and month/day values 0..=99. Plus canonical-form mutations: valid dates with day-of-month changed to impossible values (30/31 in the wrong month, Feb 29 on non-leap years).

## Predicted red/green

- Half 1 (exhaustive roundtrip): predicted **green** — both directions are Hinnant's civil-days algorithms and the doc comments claim exact inversion.
- Half 2 (canonical acceptance): predicted **RED** today; defect confirmed by reading the code (`days_from_iso`, `src/query.rs:62-79`):
  - **Non-canonical year accepted:** the length/dash pre-filter (`query.rs:64-65`) passes `+026-01-01`, and `"+026".parse::<i64>()` succeeds (`query.rs:67-68`) — Rust integer parsing accepts a leading `+`. `days_from_iso("+026-01-01")` returns `Some`, violating `s == iso_from_days(d)`.
  - **Non-calendar dates accepted:** the validity check is only `month in 1..=12` and `day in 1..=31` (`query.rs:70-71`); there is no month-length or leap-year validation. `days_from_iso("2024-02-30")` returns `Some`, and `2024-02-30 != iso_from_days(that value)`.
  - No multi-byte slicing hazard: the `b[4] == b'-'` / `b[7] == b'-'` checks guarantee the three subslice boundaries are char boundaries, so the generator's non-ASCII cases can only produce `None` or the two defects above — no panic to hunt.

Downstream effect: `data_cutoff` filtering (`src/query.rs:243,380`) feeds `days_from_iso` output into staleness arithmetic, so an accepted non-calendar date shifts cutoff comparisons silently.

Deliverable is the plan only; no `src/` change.

## Existing coverage / migration overlap

No `proptest!` block touches `days_from_iso`/`iso_from_days` acceptance: the only test references are fixture helpers (`tests/config.rs:202`, `tests/query_ranking.rs:26`, `tests/query_ranking.rs:204`) that *call* `iso_from_days` to build cutoffs, never assert parser behavior. Disjoint from the 22 handwritten blocks; the exhaustive half belongs in a plain `#[test]`, the generated half in the new Hegel file.
