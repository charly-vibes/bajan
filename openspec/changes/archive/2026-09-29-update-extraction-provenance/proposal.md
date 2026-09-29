# Change: Update Extraction Provenance — Supersession, Human Adoption, Evidence Spans, Run Records

## Why

An investigation of the sibling repo `donp/fpa` change `add-kb-pipeline`
(specced knowledge-base pipeline, Rule-of-5 reviewed against bajan's
specodelic corpus on 2026-09-29) surfaced four gaps in bajan's normative
specs (`specs/extraction-claims.md`, `specs/graph-model.md`):

1. **Re-extraction clobbering is unspecified.** `ex_single_call` keys
   extraction output by `(episode id, extractor version)` and re-extracts on
   version bump, but no constraint says what happens to claims already
   derived from the prior version's output — `active` claims could silently
   lose their basis.
2. **`adopt` has no actor requirement.** fpa explicitly forbids any
   automated path to `accepted`; bajan's `adopt` transition (proposed→active)
   is guarded only by `gm_reified`, leaving adoption actor-unowned.
3. **Claims carry no verbatim evidence span.** Lineage links to whole
   episodes, but nothing binds a claim to the sentence(s) supporting it;
   fpa's whitespace-collapsed containment verification is the proven
   mechanism (exact containment fails on converter-extracted text).
4. **Extraction runs lack provenance records and reasoned failure.**
   `ex_single_call` records only the cache key; there is no run record
   (model, versions, timestamps, finish status) and gate failures carry no
   reason — violations land in `rejected` unmodified, unexplained.

Items investigated but **deferred** (see design.md D5): suspicion-scored
review ordering (depends on items 3–4) and the bootstrap review gate
(optional calibration pattern).

## What Changes

Spec-level (apply stage lands as specodelic constraint edits; this change
records the deltas and the design decisions behind them):

- **extraction.claims** — three new invariants:
  - `ex_supersession`: version-bump re-extraction supersedes only
    unadopted (`proposed`) claims from prior versions; `active` claims are
    preserved and conflicts route through `ex_mutation_proposal`;
    superseded candidates are `rejected` with reason
    `superseded-by-reextraction`, never deleted.
  - `ex_run_record`: every extraction call is recorded (extractor version,
    model id when applicable, timestamps, finish status) and every failure
    (parked episode, gate rejection) carries a machine-readable reason.
  - `ex_evidence_containment`: candidate claims must carry a verbatim
    evidence span verified by whitespace-collapsed containment against the
    episode text; violations are rejected with reason
    `evidence-not-contained`.
- **graph.model** — one new invariant and one modified invariant:
  - `gm_human_adopt`: only an explicit human accept action moves a claim
    from `proposed` to `active`; batch accepts are audit-logged per claim.
  - `gm_schema_v1` → `gm_schema_v2`: claim nodes gain `evidence` (verbatim
    span + episode locator) as an eighth field; hedges in the span are
    preserved (`gm_hedge_anchor` extends to the span).

Not in this change: suspicion-scored review ordering, bootstrap review
gate, embedding-based entity resolution (already deferred), any
implementation code beyond what the new invariants' tests require.

## Impact

- Affected specodelic specs: `specs/extraction-claims.md` (+3 constraints,
  +2 properties, lifecycle unchanged), `specs/graph-model.md`
  (+1 constraint, +1 property, `gm_schema_v1` modified to `gm_schema_v2`).
- Gates that must stay green after apply: `spk lint specs`,
  `ah check` (new constraints need deriving properties + corresponding
  tests), `dont check` (new constraints cite the fpa change docs as
  provenance), `pretender`.
- Affected code (apply stage): claim-node schema/types, extraction cache
  and run-record store, adopt CLI path — each behind the TDD tasks in
  `tasks.md`.