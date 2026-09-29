# Design — update-extraction-provenance

Upstream source: `donp/fpa` change `add-kb-pipeline`
(`openspec/changes/add-kb-pipeline/{design.md,specs/*/spec.md}`), reviewed
into bajan scope by a Rule-of-5 pass on 2026-09-29. This file records only
decisions that shape the spec deltas and their eventual specodelic edits.

## Context

Bajan's normative requirements are the specodelic files under `specs/`
(four layers: constraints, model, properties, extension points); openspec
changes exist to carry design reasoning and deltas for larger spec
revisions. fpa's KB pipeline independently arrived at the same
architecture as bajan (schema-first extraction, many-to-many provenance,
SQLite, human-only acceptance) — the deltas here are the four places where
fpa is *stricter* or *more specified* than bajan.

## Goals / Non-Goals

- Goals: close all four gaps at the spec level; keep every new constraint
  derivable (a property cites it) so `spk lint` and `ah check` stay clean;
  preserve frozen-schema discipline by making the schema change explicit
  (v1 → v2), not silent.
- Non-Goals: no retrieval, no suspicion scoring, no bootstrap gate, no
  change to `ex_single_call`'s one-call-per-episode economics, no entity
  resolution changes.

## Decisions

### D1 — Supersession semantics map fpa's candidate/reviewed split onto bajan's state machine

fpa (kb-extract, last requirement): re-extraction replaces only `candidate`
facts; `accepted`/`rejected` are preserved. Bajan's equivalent split is
claim status: `proposed` (= `staged` in extraction.claims' dual view) is
unadopted and replaceable; `active` and `rejected` are not. So:

- Re-extraction at a new extractor version supersedes the prior version's
  `proposed` claims for that episode: status `rejected`, reason
  `superseded-by-reextraction`, lineage and evidence intact (tombstone, not
  delete — consistent with `gm_lineage_survives`).
- `active` claims are never touched by re-extraction. If the new
  extraction output conflicts with an `active` claim, the existing
  `ex_mutation_proposal` path already handles it (invalidation proposal
  with causing-episode lineage) — no new mechanism.
- Superseded candidates may be re-adopted only by re-staging (existing
  lifecycle), never silently resurrected.

Alternative rejected: version-keyed *parallel* claim sets (keep v1 and v2
claims side by side). Rejected because it multiplies lineage edges per
episode and makes `gm_reified`'s N-edge accounting ambiguous.

### D2 — Adoption actor requirement, not just a guard rewording

`gm_human_adopt` makes the *actor* part of the invariant: an explicit
human accept action (CLI `bajan adopt`, batch permitted) is the only
transition proposed→active; each accepted claim is individually
audit-logged with actor and timestamp (fpa kb-review: "batch accept is a
human action"). Actor means the local operator identity available to the
suite envelope/config — single-user CLI, no auth model; exact source is
resolved at apply. `gm_reified` remains as a co-guard — the transition
guard becomes the conjunction, pending a pre-apply check that specodelic
guards accept conjunction citations (`spk explain`); fallback is
`gm_human_adopt` as the sole transition guard, since `gm_reified` already
structurally forbids unlinked claims. This mirrors fpa's kb-store "no automated path
sets `accepted`" and closes the ownership hole found in the review
(CORR-002).

### D3 — Evidence spans require an explicit schema revision (gm_schema_v2)

`gm_schema_v1` freezes claim nodes to seven fields; adding `evidence`
(verbatim span + episode locator) is a deliberate v2 revision, not a
pickup. Consequences:

- `ex_typed_gate` extends: a candidate persists only if its evidence span
  survives whitespace-collapsed containment against the episode text
  (exact containment is unimplementable over converter-extracted text —
  fpa's hard-won lesson, design.md D6). Violations are rejected with
  reason `evidence-not-contained`, never repaired.
- `gm_hedge_anchor` extends to the span: the evidence span must not drop a
  hedge marker present in the supporting episode text.
- Downgrade path: adopters without evidence-capable extractors can emit
  span unknown (same typed-absent convention as the ingestion contract's
  locator fallback — an explicit absent marker, never a string colliding
  with a real value) — extraction remains possible for episodes where sentence
  alignment fails, but `unknown` spans are flagged by the deterministic
  flag pass (the reflection mechanism, extended). The pre-v2 migration
  backfill does not schedule re-extraction of affected episodes; flagged
  `unknown` spans are actionable later, not urgent.
- Unknown-span flagging rides the existing deterministic reflection pass
  (`ex_reflection`) rather than introducing a second audit mechanism.
- The whole-episode lineage edge (`derived_from`) is unchanged; evidence
  is an additional narrowing, not a replacement — N-source accounting in
  `gm_reified` is unaffected.

### D4 — Run records live beside the cache, reasons live beside rejections

`ex_run_record` adds a run row per extraction call: episode id, extractor
version, model id (when the extractor uses an LLM), started/finished
timestamps, finish status. This parallels fpa's `run` table (design.md D4)
but stays storage-agnostic at spec level (rows in the embedded SQLite
store per `gm_embedded_store`). Failure reasons (parked episode after
retries, gate rejection, quarantine-style containment failure) are recorded
on the run or the rejection record — **not** on the claim node, keeping
`gm_schema_v2`'s field list closed. `ex_single_call` is amended, not
replaced: cache-key economics unchanged.

### D5 — Deferred items and why

- **Suspicion-scored review ordering** (fpa kb-review): two of its four
  signals presuppose evidence spans (D3) and quarantine-rate records (D4).
  Deferred to a later change, likely landed as an eval-harness spec
  satisfying the published `ex_output_schema` / `gm_relation_typing`
  extension points.
- **Bootstrap review gate** (state-based first-source review gate):
  calibration pattern, valuable once bajan has real corpora flowing
  through review; no spec hole in bajan today blocks it.

### Grounding note (dont / ah gates)

Specodelic structured links (`traces_to`) cite corpus-resolvable invariant
definitions only (`linter.total_refs`): new constraints trace to
`[[extraction.claims]]` / `[[graph.model]]` and to the invariants they
amend. Upstream fpa provenance is cited in prose (repo-relative paths), as
the existing specs already cite Chalef and `knowledge-graphs-report.md` in
prose. `dont check` grounding references this change's docs; every new
invariant gets a deriving property in the same apply-stage edit so
`spk lint` (`coverage`, `no_orphan_property`) and `ah check`
(spec-test correspondence) pass in the same commit as the constraint edits
and their tests.

## Risks / Trade-offs

- `gm_schema_v2` is a breaking change for any stored graph: a migration
  backfills `evidence = unknown` for existing claim nodes (apply-stage
  task, loud in output, never silent).
- Containment verification adds a per-candidate string search — negligible
  at bajan's scale, and it runs in the deterministic post-pass
  (`ex_no_llm_post` unchanged).
- Human-only adoption makes scripted bulk adoption impossible by design;
  batch accept is the escape hatch, still per-claim audited.

## Migration / Open Questions

1. Should `evidence` be required for all new extractions from day one, or
   can `unknown` persist indefinitely? (Design lean: required at gate;
   `unknown` only for alignment-failure episodes, flagged by audit.)
2. Does the adopt audit log belong in the graph store or the suite-standard
   JSON envelope / audit module? (Lean: same store, `audit_log` analogue
   of fpa's, decided at apply.)