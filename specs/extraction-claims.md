---
id: extraction.claims
kind: intent
statement: "WHEN an episode is persisted THE bajan extractor SHALL propose candidate claims typed against the published claim schema with lineage edges to their source episode"
---

# claims

The write path between a persisted episode (see `ingestion.contract`) and a
proposed claim (see `graph.model`). One extraction call per episode —
single-shot, as validated in the corpus (Chalef/Zep 206: "we've managed to
get a single-shot extraction working… really cheaply as a consequence") —
produces candidate claims against the frozen claim-node schema
(`gm_schema_v2` in graph.model — text, dates, cutoff, status, scope,
source_type, evidence). Everything
after that call is deterministic: normalization, dedup (simhash/entropy),
entity matching, and conflict detection never invoke an LLM (Chalef: "far
cheaper, far faster, far more deterministic"). The extractor proposes;
it never merges entities, applies trust policies, or mutates the graph.
Output is typed at the door (Coyle 209: validation before the loop
continues) and rejected rather than repaired. Extraction output is cached
per (episode id, extractor version) so re-ingest stays idempotent and the
graph remains rebuildable (`gm_embedded-store` semantics, in
graph.model); a version bump re-extracts as new work, never reusing a
cache entry from another version. An episode with zero valid candidates is
legitimately extracted and cached as empty — absence of claims is data,
not failure. A parked episode — one that exhausted its retry budget on
repeated call failure (bajan-42y) — holds no cache entry either, so a
later extraction pass re-attempts it with a fresh retry budget: the park
is one run's honest stopping (the same honesty as budget-exhaustion in
query.tools), never a permanent state, and re-ingest neither grants nor
blocks the retry — eligibility follows cache absence.

Pipeline seam: the three specs' lifecycles hand off as follows — an
episode leaves `ingestion.contract` as `persisted` and enters this spec's
lifecycle as `pending`; a claim leaves this spec's lifecycle as `staged`,
which is the same state graph.model names `proposed` (from there,
`adopt`/`discard`/`invalidate` apply). `staged` and `proposed` are one
state viewed from two files. How hedging survives extraction is governed
by `gm_hedge_anchor` in graph.model; reflection flags violations rather
than introducing claims (see `ex_reflection`). Supersession on version
bumps, per-extraction-call run records, and evidence-span containment
follow fpa's kb-extract/kb-store pipeline
(`openspec/changes/update-extraction-provenance/design.md` D1, D3, D4);
span-absent episodes persist a typed absent marker — same convention as
the ingestion contract's locator/cutoff absent marker — never a string
that could collide with a real span.

## Constraints

| id | kind | expr | traces_to |
|----|------|------|-----------|
| ex_single_call | invariant | each persisted episode id receives exactly one successful LLM extraction call producing candidate claims and entities; failed calls retry with backoff and park the episode in `rejected` after repeated failure; re-ingest of a successfully extracted episode reuses the (episode id, extractor version)-keyed cached output and starts no new call | [[extraction.claims]] |
| ex_park_requeue | invariant | an episode parked in `rejected` after repeated call failure holds no extraction-cache entry at the failed extractor version, so a later extraction pass re-attempts it with a fresh retry budget — the park is one run's honest stopping, never a permanent state; every re-attempt writes its own run rows and prior parked run rows persist with their reasons; re-ingest is orthogonal — an unchanged re-submission of a parked episode is an ingest-side no-op (`already_persisted`) that neither grants nor blocks the fresh budget, because eligibility follows cache absence, never re-ingest | [[extraction.claims]] |
| ex_typed_gate | invariant | a candidate claim persists only if it validates against the published claim schema, carries at least one episode lineage edge, and carries an evidence span that passes `ex_evidence_containment`; invalid candidates are rejected, never repaired; an episode with zero valid candidates is legitimately extracted and cached as empty | [[extraction.claims]] |
| ex_supersession | invariant | re-extracting an episode at a new extractor version supersedes only that episode's `staged` claims derived from prior extractor versions — they become `rejected` with reason `superseded-by-reextraction`, lineage and evidence intact (a tombstone, not a delete) — while `active` and `rejected` claims from any version are never mutated; new output that conflicts with an `active` claim stages an invalidation proposal per `ex_mutation_proposal`; a superseded claim re-enters `staged` only through an explicit re-stage action, never an automated path | [[extraction.claims]] |
| ex_run_record | invariant | every extraction call writes one run row — episode id, extractor version, model id when the extractor uses an LLM, started and finished timestamps, finish status — parallel to, and without changing, the `(episode id, extractor version)` cache economics; every failure outcome (an episode parked in `rejected` after repeated call failure, every candidate rejection at the validation gate) carries a machine-readable reason stored on the run or rejection record, never on a claim node | [[extraction.claims]] |
| ex_evidence_containment | invariant | every candidate claim carries an evidence span — a verbatim span of the supporting episode text plus its episode locator, or the typed absent marker when sentence alignment failed — validated by containment after whitespace collapse (all whitespace runs become a single space) of both span and episode text; a span failing containment is rejected with reason `evidence-not-contained`, never repaired; a typed-absent span persists but is flagged by the deterministic pass (`ex_reflection` extended), not treated as a violation | [[extraction.claims]] |
| ex_no_llm_post | invariant | normalization, dedup, entity matching, and conflict detection after the extraction call are deterministic; normalization rules appear both as prompt instructions and in a deterministic post-pass, because prompts are not bulletproof | [[extraction.claims]] |
| ex_mutation_proposal | invariant | a candidate claim that conflicts with an existing active claim stages an invalidation proposal carrying its causing-episode lineage; the existing claim is not mutated by the extractor | [[extraction.claims]] |
| ex_reflection | advisory | the single extraction call includes a self-check that verifies candidate claims against the episode text and enriches change lineage, rendered as flags by a deterministic pass; it flags unsupported candidates and never introduces new ones, and it is not a second LLM call | [[extraction.claims]] |
| ex_tag_inheritance | invariant | workspace tags and source metadata on an episode are inherited by every claim derived from it as a projection — the union over its lineage edges, resolved by walking `derived_from` at query time — and are never stored on the claim node | [[extraction.claims]] |
| ex_deterministic_structure | invariant | structure the converter already supplied (heading anchors, section boundaries, links) is consumed deterministically and never re-extracted by the LLM | [[extraction.claims]] |
| ex_section_filter | invariant | the deterministic post-pass rejects every candidate of an episode whose tags include a non-knowledge section-kind tag — `section-kind:praise`, `section-kind:copyright`, `section-kind:acknowledgments` — with machine-readable reason `section-filtered` on the run record (never on a claim node, per `ex_run_record`); the deny set is fixed in code, never an LLM judgment (`ex_no_llm_post` holds); untagged episodes stage unchanged, and a fully filtered episode still caches as extracted-empty per the `ex_single_call` cache economics | [[extraction.claims]] |
| ex_output_schema | extension_point | the candidate-claim record schema (field names, types, requiredness) is published for eval-harness specs to conform to | [[extraction.claims]] |

## Model

### States

- `pending`
- `extracted`
- `validated`
- `staged`
- `rejected`

### Transitions

| id | from | to | guard |
|----|------|----|-------|
| run | pending | extracted | [[extraction.claims.ex_single_call]] |
| gate | extracted | validated | [[extraction.claims.ex_typed_gate]] |
| abort | pending | rejected | [[extraction.claims.ex_single_call]] |
| requeue | rejected | pending | [[extraction.claims.ex_park_requeue]] |
| refuse | extracted | rejected | [[extraction.claims.ex_typed_gate]] |
| supersede | staged | rejected | [[extraction.claims.ex_supersession]] |
| stage | validated | staged | [[extraction.claims.ex_no_llm_post]] |
| refuse_section | extracted | rejected | [[extraction.claims.ex_section_filter]] |

## Properties

| id | kind | derives_from | generator | predicate |
|----|------|--------------|-----------|-----------|
| p_single_call | unit | [[extraction.claims.ex_single_call]] | episode corpora ingested, re-ingested unchanged, and re-ingested with a bumped extractor version | exactly one successful call per (episode id, extractor version); re-ingest at the same version produces no new call and reuses cached output; a version bump re-extracts |
| p_park_requeue | unit | [[extraction.claims.ex_park_requeue]] | episodes parked after retry-budget exhaustion, re-submitted unchanged, then re-run through extraction passes that fail again and succeed | the re-attempt pass starts a fresh retry budget and writes new run rows while prior parked rows persist with their reasons; success caches the output and ends the requeue cycle; renewed exhaustion parks again with a recorded reason; the unchanged re-submission itself emits `already_persisted` and changes neither graph nor run state |
| p_section_filter | unit | [[extraction.claims.ex_section_filter]] | candidate batches from episodes tagged with each deny-set section kind and from untagged episodes | tagged episodes stage zero claims and their run rows carry the `section-filtered` reason; untagged episodes stage unchanged; a fully filtered episode is cached as extracted so the pass never re-calls it |
| p_typed_gate | unit | [[extraction.claims.ex_typed_gate]] | candidate records mixing schema-valid, schema-violating, lineage-less, evidence-less, and zero-candidate episodes | only schema-valid lineage-bearing candidates with a passing evidence span reach `validated`; every violation lands in `rejected` unmodified; a zero-candidate episode caches as legitimately empty |
| p_supersession | unit | [[extraction.claims.ex_supersession]] | episode corpora re-extracted at bumped extractor versions with prior output in `staged`, `active`, and `rejected` states | only prior-version `staged` claims become `rejected` with reason `superseded-by-reextraction`, lineage and evidence remaining queryable; `active` and `rejected` claims from any version never change status; active conflicts stage invalidation proposals; superseded claims re-enter `staged` only via explicit re-stage |
| p_run_record | unit | [[extraction.claims.ex_run_record]] | extraction calls with success, repeated-failure, and gate-rejection outcomes | every call writes exactly one run row with episode id, extractor version, model id when present, timestamps, and finish status; every parked episode and gate rejection carries a machine-readable reason; claim nodes expose no reason field |
| p_evidence_containment | unit | [[extraction.claims.ex_evidence_containment]] | candidate spans that are contained, non-contained after whitespace collapse, and typed-absent | contained spans persist verbatim; non-contained spans reject with reason `evidence-not-contained` and are never repaired; typed-absent spans persist flagged by the reflection pass and are never reported as violations |
| p_no_llm_post | unit | [[extraction.claims.ex_no_llm_post]] | candidate sets containing near-duplicates (simhash distance) and unnormalized fields | identical post-pass inputs produce identical outputs and no LLM call occurs after the extraction step |
| p_mutation_proposal | unit | [[extraction.claims.ex_mutation_proposal]] | candidate claims conflicting with seeded active claims | every conflict stages an invalidation proposal citing its causing episode; seeded claims are unchanged until the proposal is applied |
| p_reflection | unit | [[extraction.claims.ex_reflection]] | extracted candidates containing unsupported statements and dropped hedges | reflection flags each unsupported candidate; the post-reflection candidate set introduces no new claims |
| p_tag_inheritance | unit | [[extraction.claims.ex_tag_inheritance]] | episodes with varying tag sets, derived claim sets, and multi-source claims | every derived claim resolves to exactly the union of its supporting episodes' tag sets via lineage walk; no tag is stored on the claim node |
| p_deterministic_structure | unit | [[extraction.claims.ex_deterministic_structure]] | episodes with converter-supplied structure markers | structural relations derive from markers alone, independent of extraction output |
| p_output_schema | unit | [[extraction.claims.ex_output_schema]] | candidate-claim records sampled from the published schema | the published schema validates every emitted candidate record |
