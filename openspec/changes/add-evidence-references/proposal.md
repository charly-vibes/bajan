# Change: Add Resolvable Evidence References for Source-Backed Claims

## Why

The Bajan/Sunset capability-gaps analysis (artifact
`bajan_sunset_reusable_capability_gaps.md`, 2026-11-04, item B1 — the
artifact's highest-priority Bajan structural improvement; filed in pinky
ledger `ledger/topics/bajan-structure.org`) finds that bajan preserves
episode lineage (`Evidence::Span {text, locator}`, `Lineage` with episode
id + extractor version) but never represents an evidence reference as a
*resolvable* pointer: there is no source-revision digest, no locator
scheme version, no resolution state, and no record of when evidence was
last checked. Consumers that re-resolve evidence (bogart's pending
`add-bajan-edge-projection`, item bogart-5j9) need exactly this.

Two repo facts make this the right moment, and shape the design:

1. **Tombstones made resolution stateful.** Episode delete/revive landed
   (bajan-aod, `episode_tombstones`). An evidence reference must
   therefore distinguish `tombstoned` (policy-deleted, revival possible)
   from `unresolvable` (source gone) — exported evidence packages must
   not misreport provenance across a tombstone window.
2. **Authority tiers exist** (`episodes.authority_tier`, immutable source
   metadata per `ic_verbatim`). A reference must declare its interaction:
   it records the tier as observed at creation and never re-ranks
   (ranking stays `qt_ranking`'s query-time job).

The frozen `ClaimNode` schema (`gm_schema_v2`) is untouched — references
are side records keyed by claim, per the artifact's explicit instruction.

## What Changes

Spec-level (apply stage lands as specodelic constraint edits to
`specs/graph-model.md`; this change records the deltas and design
decisions):

- **graph-model** — three new invariants, no amendments:
  - `gm_evidence_ref`: evidence-backed claims can carry an evidence-
    reference side record (episode id, source revision digest, locator
    scheme version, resolution state, last-checked timestamp); claims
    without one remain fully valid and queryable.
  - `gm_evidence_resolution`: resolution yields exactly one explicit
    state — `resolved`, `unresolvable`, or `tombstoned` — never a
    fabricated anchor; tombstoned reverts to `resolved` on revive;
    unresolvable claims stay inspectable, never deleted.
  - `gm_evidence_ref_tier`: the reference records the supporting
    episode's authority tier as observed at creation; resolution never
    re-ranks or re-weights claims.

## Impact

- **specs/graph-model/spec.md** — three ADDED requirement sets.
- **Code** — evidence-reference side table, resolve operation, revive
  hook. No ClaimNode/claims-table change, no extraction-path change, no
  query-result change (inspection surface ships with apply; consumer
  export formats are out of scope).
- **Compatibility** — purely additive; claims without references behave
  exactly as before.
