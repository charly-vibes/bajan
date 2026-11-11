# Change: Add Query Coverage Evidence for Partially Ingested Corpora

*Approved: 2026-11-10, operator session approval. Implementation ticket:
bajan-alb.*

## Why

The Bajan/Sunset capability-gaps analysis (artifact
`bajan_sunset_reusable_capability_gaps.md`, 2026-11-04, item B6; filed in
pinky ledger `ledger/topics/bajan-structure.org`) flags a honesty failure
mode: a read returning zero claims over a partially ingested corpus can be
read as "the underlying world contains no matching facts" when the truth
is "the corpus is incomplete".

The budget half of the problem is already solved: `qt_bounded_traversal`
(specs/query-tools.md) mandates honest stopping with the
`complete` / `budget-exhausted` vocabulary. The missing half is *corpus
coverage*: nothing records how many episodes an import run declared, so
no read can distinguish a complete corpus from a partial one. This change
adds exactly that evidence, and nothing else.

Standalone: no dependency on the other proposed changes
(add-deterministic-shape-rejection, add-evidence-references).

## What Changes

Spec-level (apply stage lands as specodelic constraint edits; this change
records the deltas and design decisions):

- **ingestion-contract** — one new invariant:
  - `ic_coverage_declaration`: every accepted import run persists a
    coverage declaration recording the declared episode stable-id set for
    that run, immutable once the run completes, so later reads can
    distinguish declared-but-unpersisted episodes from never-declared ids.
- **query-tools** — one new invariant:
  - `qt_coverage_status`: reads compute corpus coverage from the
    declarations — `coverage-complete` when every declared episode is
    persisted, `coverage-partial` with declared and persisted counts
    otherwise — and a zero-hit read under `coverage-partial` carries the
    marker and counts, never a bare `not-found`.

No changes to existing constraints: `qt_bounded_traversal` keeps the
budget vocabulary; this adds an orthogonal coverage axis. Partial-coverage
zero-hits are still correct answers about the persisted graph — the
marker prevents a *scope* misreading, it does not change graph semantics.

## Impact

- **specs/ingestion-contract/spec.md** — one ADDED requirement.
- **specs/query-tools/spec.md** — one ADDED requirement.
- **Code** — coverage-declaration row written per accepted import run;
  coverage
  computation + marker on the zero-hit read path. No schema change to
  episodes or claims (new side table), no query-language change.
- **Compatibility** — reads over fully-ingested corpora produce
  byte-identical results; only partial corpora gain a marker.
