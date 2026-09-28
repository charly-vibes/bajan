---
id: graph.model
kind: intent
statement: "THE bajan knowledge graph SHALL represent every material claim as a reified claim node linked by lineage edges to verbatim episode nodes and entity nodes"
---

# model

The knowledge model follows the industry practices synthesized in
`knowledge-graphs-report.md`: schema-first beats schema-less; provenance
lives in graph structure, not logs; entity resolution is the cost center.
Claims are reified as nodes — never typed triples between entities — so
multi-source lineage, contradiction edges, and invalidation are
first-class. This supersedes the markdown-page knowledge base of the
earlier knowledge-assistant spec (`docs/knowledge-assistant-ears-spec.md`)
while carrying over its defect classes as graph invariants: verbatim
evidence, source-faithful dating, no silent merges, deletion only by
lineage walk. Claims are proposed from the episode stream published by the
`ingestion.contract` spec; how extraction proposes claims is out of scope
here and specced separately.

## Constraints

| id | kind | expr | traces_to |
|----|------|------|-----------|
| gm_reified | invariant | every claim is a node with at least one `derived_from` edge to an episode node; a claim synthesized from N sources carries N lineage edges, one per episode | [[graph.model]] |
| gm_lineage_survives | invariant | invalidation sets `invalid_at` and never destroys lineage; a claim is deleted only when no episode supports it any longer | [[graph.model]] |
| gm_deterministic_er | invariant | entity resolution is deterministic (normalization plus curated alias lists); unmatched mentions create new entity nodes; merge candidates are proposed as `possible_duplicate_of` edges into a review queue; no silent or automatic merge | [[graph.model]] |
| gm_merge_keeps | invariant | a merge is performed only after explicit human resolution of a review candidate, and it preserves every lineage edge from both sides; rejecting a candidate removes only its `possible_duplicate_of` edge | [[graph.model]] |
| gm_schema_v1 | invariant | claim nodes carry exactly: text, valid_at, invalid_at, data_cutoff, status, scope, source_type — source_type inherited from the episode; no confidence field and no vector blob on any node | [[graph.model]] |
| gm_embedded_store | invariant | the graph persists as plain rows in a single embedded SQLite database and is fully rebuildable from the episode stream plus extraction output | [[graph.model]] |
| gm_hedge_anchor | invariant | epistemic strength lives only in verbatim episode text; derived artifacts must not upgrade a hedged source statement to an unhedged claim | [[graph.model]] |
| gm_relation_typing | extension_point | the claim relation vocabulary (`mentions`, `contradicts`, `supports`, `derived_from`, `possible_duplicate_of`) is published for query-tool specs to conform to | [[graph.model]] |

## Model

### States

- `proposed`
- `active`
- `invalid`
- `rejected`

### Transitions

| id | from | to | guard |
|----|------|----|-------|
| adopt | proposed | active | [[graph.model.gm_reified]] |
| discard | proposed | rejected | [[graph.model.gm_reified]] |
| invalidate | active | invalid | [[graph.model.gm_lineage_survives]] |
| repropose | invalid | proposed | [[graph.model.gm_lineage_survives]] |

## Properties

| id | kind | derives_from | generator | predicate |
|----|------|--------------|-----------|-----------|
| p_reified | unit | [[graph.model.gm_reified]] | claims extracted from synthetic corpora with known source counts N in 1..4 | every claim node has exactly N lineage edges matching its known source count; a zero-source claim is never persisted |
| p_lineage_survives | unit | [[graph.model.gm_lineage_survives]] | random sequences of invalidation and episode-deletion operations over a seeded graph | after every operation: each surviving claim's episode edge count equals its supporting-episode count; a claim disappears only when its last supporting episode is deleted |
| p_deterministic_er | unit | [[graph.model.gm_deterministic_er]] | mention streams mixing known aliases, unmatched names, and near-duplicates | entity node count changes only via explicit new-node creation or reviewed merges; queue contents equal candidate pairs |
| p_merge_keeps | unit | [[graph.model.gm_merge_keeps]] | human-reviewed merge and rejection operations over seeded entity pairs | approved merges preserve the union of both sides' lineage edges; rejected candidates lose only their `possible_duplicate_of` edge |
| p_schema_v1 | unit | [[graph.model.gm_schema_v1]] | claim records with extra, missing, and mistyped fields | stored claim nodes expose exactly the seven schema fields with types preserved |
| p_embedded_store | unit | [[graph.model.gm_embedded_store]] | seeded graphs of arbitrary size | dump-then-recreate from episode stream plus extraction output reproduces the graph exactly |
| p_hedge_anchor | unit | [[graph.model.gm_hedge_anchor]] | episode texts containing hedged statements (may, signals, estimates) | no derived claim text drops a hedge marker present in its supporting episode |
| p_relation_typing | unit | [[graph.model.gm_relation_typing]] | edge records sampled from the published vocabulary | every edge label is a member of the published relation set |
