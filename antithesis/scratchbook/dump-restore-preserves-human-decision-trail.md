# bajan/dump-restore-preserves-human-decision-trail — rebuild preserves the audit trail

Baseline: extends superproject `../../antithesis/scratchbook/property-catalog.md` and `../../antithesis/scratchbook/properties/bajan.md`. Evidence re-verified against bajan `c63dcdc`.

## Invariant statement

For any store state, `dump_extraction_output()` followed by `restore_extraction_output()` into a fresh `SqliteStore` yields a store with, up to ordering:

1. Equal `audit()` — every adoption audit row survives; every claim that was active-with-one-audit-row still has exactly one (the discipline stated at `src/store/mod.rs:37-42`).
2. Equal `restage` history — restaged claims are restaged in the rebuilt store.
3. Equal `evidence_ref` / `evidence_checks` state — resolution state and check history survive.

The rebuild contract is stated as "rebuild reproduces the graph, never re-litigates it" (`sqlite.rs`, restore comments); the human decision trail is part of what a rebuild must reproduce for the audit discipline to survive.

## Check mode

In-memory SQLite (`SqliteStore::open_in_memory()`), in-process; two stores per case (source + rebuilt). No crash injection, no subprocess — the dump is the crash boundary being modeled.

## Seam

Public API: `dump_extraction_output` (`src/store/sqlite.rs:1386`), `restore_extraction_output` (`sqlite.rs:1530`), `adopt` (`sqlite.rs:2795`), `audit` (`sqlite.rs:2834`), `restage` (`sqlite.rs:2034`), `evidence_check` (`sqlite.rs:1005`), `evidence_ref` (`sqlite.rs:1107`), `evidence_checks` (`sqlite.rs:1128`). `ExtractionDump` fields are public and `Deserialize`, so a harness can construct dumps directly.

## Generator sketch

Generate a store history: 1..=10 claims (mixed staged/active/rejected), 0..=6 `adopt` calls (the audit writer at `sqlite.rs:2824`), 0..=4 `restage` calls (`sqlite.rs:2080`), 0..=3 `evidence_check` calls (writers at `sqlite.rs:1066,1087`), then dump → restore and compare all four views. Control case: claims/edges/invalidations/supersessions equality (known good) to isolate the missing-table failure from general restore breakage.

## Predicted red/green

Predicted **RED** today; defect confirmed by reading the code:

- `ExtractionDump` (`src/store/sqlite.rs:375-427`) carries: `claims`, `extracted`, `edges`, `invalidations`, `supersessions`, `reviews`, `review_audits`, `er_aliases`, `contradiction_proposals`, `contradiction_audits`, `episode_tombstones`, `coverage_declarations`, `coverage_declared_ids`. It has **no** fields for the `audits`, `restages`, `evidence_refs`, or `evidence_checks` tables — although all four tables are created (`sqlite.rs:74,97,153,159`) and written on the live path (`INSERT INTO audits :2824`; `INSERT INTO restages :2080`; `INSERT INTO evidence_refs :1066`; `INSERT INTO evidence_checks :1087`).
- `restore_extraction_output` (`sqlite.rs:1530-1643`) restores exactly the carried tables and none of the four.
- Therefore clauses 1–3 fail whenever the source store holds any audit/restage/evidence row: the dump is lossy by construction, not by corruption.

The asymmetry is visible in the codebase's own history: `review_audits` and `contradiction_audits` were added to the dump with "without these a rebuild would lose the … lifecycle states" rationales, while claim `audits`/`restages`/evidence tables never got the same treatment. Open question from the baseline stands: is the audit trail inside the "graph exactly" rebuild contract? If the owner scopes the property to "the graph only," this goes green by narrowing, not by fix — that decision is the owner's.

Deliverable is the plan only; no `src/` change.

## Existing coverage / migration overlap

No `proptest!` block covers dump/restore trail equality. Existing restore tests check specific carried tables and would not notice the loss: `tests/contradiction_proposals.rs:430`, `tests/contradiction_scan.rs:295`, `tests/entity_review.rs:488,1030`, `tests/episode_tombstones.rs:269`, `tests/query_contradiction.rs:246`, `tests/resolve.rs:305`, `tests/write_path_discipline.rs:193`, `tests/coverage_declarations.rs:443`, `tests/vertical_slice.rs:125`. Disjoint from the 22 handwritten blocks.
