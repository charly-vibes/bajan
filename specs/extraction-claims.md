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
produces candidate claims against the frozen `gm_schema-v1`. Everything
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
not failure.

Pipeline seam: the three specs' lifecycles hand off as follows — an
episode leaves `ingestion.contract` as `persisted` and enters this spec's
lifecycle as `pending`; a claim leaves this spec's lifecycle as `staged`,
which is the same state graph.model names `proposed` (from there,
`adopt`/`discard`/`invalidate` apply). `staged` and `proposed` are one
state viewed from two files. How hedging survives extraction is governed
by `gm_hedge_anchor` in graph.model; reflection flags violations rather
than introducing claims (see `ex_reflection`).

## Constraints

| id | kind | expr | traces_to |
|----|------|------|-----------|
| ex_single_call | invariant | each persisted episode id receives exactly one successful LLM extraction call producing candidate claims and entities; failed calls retry with backoff and park the episode in `rejected` after repeated failure; re-ingest of a successfully extracted episode reuses the (episode id, extractor version)-keyed cached output and starts no new call | [[extraction.claims]] |
| ex_typed_gate | invariant | a candidate claim persists only if it validates against the published claim schema and carries at least one episode lineage edge; invalid candidates are rejected, never repaired; an episode with zero valid candidates is legitimately extracted and cached as empty | [[extraction.claims]] |
| ex_no_llm_post | invariant | normalization, dedup, entity matching, and conflict detection after the extraction call are deterministic; normalization rules appear both as prompt instructions and in a deterministic post-pass, because prompts are not bulletproof | [[extraction.claims]] |
| ex_mutation_proposal | invariant | a candidate claim that conflicts with an existing active claim stages an invalidation proposal carrying its causing-episode lineage; the existing claim is not mutated by the extractor | [[extraction.claims]] |
| ex_reflection | advisory | the single extraction call includes a self-check that verifies candidate claims against the episode text and enriches change lineage, rendered as flags by a deterministic pass; it flags unsupported candidates and never introduces new ones, and it is not a second LLM call | [[extraction.claims]] |
| ex_tag_inheritance | invariant | workspace tags and source metadata on an episode are inherited by every claim derived from it as a projection — the union over its lineage edges, resolved by walking `derived_from` at query time — and are never stored on the claim node | [[extraction.claims]] |
| ex_deterministic_structure | invariant | structure the converter already supplied (heading anchors, section boundaries, links) is consumed deterministically and never re-extracted by the LLM | [[extraction.claims]] |
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
| refuse | extracted | rejected | [[extraction.claims.ex_typed_gate]] |
| stage | validated | staged | [[extraction.claims.ex_no_llm_post]] |

## Properties

| id | kind | derives_from | generator | predicate |
|----|------|--------------|-----------|-----------|
| p_single_call | unit | [[extraction.claims.ex_single_call]] | episode corpora ingested, re-ingested unchanged, and re-ingested with a bumped extractor version | exactly one successful call per (episode id, extractor version); re-ingest at the same version produces no new call and reuses cached output; a version bump re-extracts |
| p_typed_gate | unit | [[extraction.claims.ex_typed_gate]] | candidate records mixing schema-valid, schema-violating, lineage-less, and zero-candidate episodes | only schema-valid lineage-bearing candidates reach `staged`; every violation lands in `rejected` unmodified; a zero-candidate episode caches as legitimately empty |
| p_no_llm_post | unit | [[extraction.claims.ex_no_llm_post]] | candidate sets containing near-duplicates (simhash distance) and unnormalized fields | identical post-pass inputs produce identical outputs and no LLM call occurs after the extraction step |
| p_mutation_proposal | unit | [[extraction.claims.ex_mutation_proposal]] | candidate claims conflicting with seeded active claims | every conflict stages an invalidation proposal citing its causing episode; seeded claims are unchanged until the proposal is applied |
| p_reflection | unit | [[extraction.claims.ex_reflection]] | extracted candidates containing unsupported statements and dropped hedges | reflection flags each unsupported candidate; the post-reflection candidate set introduces no new claims |
| p_tag_inheritance | unit | [[extraction.claims.ex_tag_inheritance]] | episodes with varying tag sets, derived claim sets, and multi-source claims | every derived claim resolves to exactly the union of its supporting episodes' tag sets via lineage walk; no tag is stored on the claim node |
| p_deterministic_structure | unit | [[extraction.claims.ex_deterministic_structure]] | episodes with converter-supplied structure markers | structural relations derive from markers alone, independent of extraction output |
| p_output_schema | unit | [[extraction.claims.ex_output_schema]] | candidate-claim records sampled from the published schema | the published schema validates every emitted candidate record |
