# Change: Add Resolvable Evidence References for Source-Backed Claims

*Approved: 2026-11-10, operator session approval. Implementation ticket:
bajan-5ux.*

## Why

The Bajan/Sunset capability-gaps analysis (artifact
`bajan_sunset_reusable_capability_gaps.md`, 2026-11-04, item B1 — the
artifact's highest-priority Bajan structural improvement; filed in pinky
ledger `ledger/topics/bajan-structure.org`) finds that bajan preserves
episode lineage (`Evidence::Span {text, locator}`, `Lineage` with episode
id + extractor version) but never represents an evidence reference as a
*resolvable* pointer: there is no locator scheme version, no resolution
state, and no record of when evidence was last checked. Consumers that
re-resolve evidence (bogart's pending `add-bajan-edge-projection`, item
bogart-5j9) need exactly this.

Two repo facts make this the right moment, and shape the design:

1. **Tombstones made resolution stateful.** Episode delete/revive landed
   (bajan-aod, `episode_tombstones`). An evidence check must therefore
   distinguish `tombstoned` (policy-deleted, revival possible) from
   `unresolvable` (source gone) — exported evidence packages must not
   misreport provenance across a tombstone window.
2. **Authority tiers exist** (`episodes.authority_tier`, immutable source
   metadata per `ic_verbatim`). A reference must declare its interaction:
   it carries the tier as observed provenance and never re-ranks
   (ranking stays `qt_ranking`'s query-time job).

A Rule-of-5 review (2026-11-10) pinned the data model: resolution is a
*writing command* (`bajan evidence check`) persisting append-only check
records — never a read that writes, keeping `qt_readonly` intact.

Independent of add-deterministic-shape-rejection and
add-query-coverage-evidence; it is the bajan-side prerequisite for
bogart's bogart-5j9 evidence re-resolution.

## What Changes

Spec-level (apply stage lands as specodelic constraint edits to
`specs/graph-model.md`; this change records the deltas and design
decisions):

- **graph-model** — four new invariants, no amendments:
  - `gm_evidence_ref`: evidence references are side records keyed by
    claim (episode id, locator scheme version, observed tier), created
    only by the explicit `bajan evidence check` command; claims without
    one remain fully valid and queryable; ClaimNode stays frozen
    (`gm_schema_v2`).
  - `gm_evidence_resolution`: each check derives exactly one explicit
    state — `resolved`, `unresolvable`, `tombstoned` — never a fabricated
    anchor; tombstoned flips to resolved only in post-revive checks;
    unresolvable claims stay inspectable, never deleted or hidden.
  - `gm_evidence_check_record`: every evidence check persists an
    immutable check record (claim key, derived state, checked-at
    timestamp); read commands never write — `qt_readonly` stays intact.
  - `gm_evidence_ref_tier`: the observed tier is provenance only;
    resolution never re-ranks or re-weights claims.

## Impact

- **specs/graph-model/spec.md** — four ADDED requirement sets.
- **Code** — `evidence_refs` + append-only `evidence_checks` tables, the
  `bajan evidence check` command, revive interplay. No ClaimNode or
  claims-table change, no extraction-path change, no query-result change
  (consumer export formats are out of scope).
- **Compatibility** — purely additive; claims without references behave
  exactly as before.
