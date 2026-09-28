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
earlier knowledge-assistant spec while carrying over its defect classes as
graph invariants: verbatim evidence, source-faithful dating, no silent
merges, deletion only by lineage walk.

## Constraints

| id | kind | expr | traces_to |
|----|------|------|-----------|
| gm_reified | invariant | every claim is a node with at least one `derived_from` edge to an episode node; a claim synthesized from N sources carries N lineage edges, one per episode | [[graph.model]] |
| gm_lineage-survives | invariant | invalidation sets `invalid_at` and never destroys lineage; a claim is deleted only when no episode supports it; entity merges preserve all lineage edges from both sides | [[graph.model]] |
| gm_deterministic-er | invariant | entity resolution is deterministic (normalization plus curated alias lists); unmatched mentions create new entity nodes; merge candidates enter a review queue via `possible_duplicate_of` edges; no silent or automatic merge | [[graph.model]] |
| gm_schema-v1 | invariant | claim nodes carry exactly: text, valid_at, invalid_at, data_cutoff, status, scope, source_type — source_type inherited from the episode; no confidence field and no vector blob on any node | [[graph.model]] |
| gm_embedded-store | invariant | the graph persists as plain rows in a single embedded SQLite database and is fully rebuildable from the episode stream plus extraction output | [[graph.model]] |
| gm_hedge-anchor | advisory | epistemic strength lives only in verbatim episode text; derived artifacts must not upgrade a hedged source statement to an unhedged claim | [[graph.model]] |
| gm_relation-typing | extension_point | the claim relation vocabulary (`mentions`, `contradicts`, `supports`, `derived_from`, `possible_duplicate_of`) is published for query-tool specs to conform to | [[graph.model]] |

## Model

### States

- `proposed`
- `active`
- `invalid`
- `merged`

### Transitions

| id | from | to | guard |
|----|------|----|-------|
| adopt | proposed | active | [[graph.model.gm_reified]] |
| invalidate | active | invalid | [[graph.model.gm_lineage-survives]] |
| merge | active | merged | [[graph.model.gm_deterministic-er]] |
| repropose | invalid | proposed | [[graph.model.gm_lineage-survives]] |

## Properties

| id | kind | derives_from | generator | predicate |
|----|------|--------------|-----------|-----------|
| p.reified | unit | [[graph.model.gm_reified]] | arbitrary claim extractions over synthetic episode corpora | every claim node in the resulting graph has at least one episode lineage edge; N-source claims have N edges |
| p.lineage-survives | unit | [[graph.model.gm_lineage-survives]] | random sequences of invalidation and merge operations over a seeded graph | after every operation: each surviving claim's episode edge count is greater than or equal to its pre-operation count minus deleted episodes |
| p.deterministic-er | unit | [[graph.model.gm_deterministic-er]] | mention streams mixing known aliases, unmatched names, and near-duplicates | entity node count changes only via explicit new-node creation or reviewed merges; queue contents equal candidate pairs |
| p.schema-v1 | unit | [[graph.model.gm_schema-v1]] | claim records with extra, missing, and mistyped fields | stored claim nodes expose exactly the seven schema fields with types preserved |
| p.embedded-store | unit | [[graph.model.gm_embedded-store]] | seeded graphs of arbitrary size | dump-then-recreate from episode stream plus extraction output reproduces the graph exactly |
| p.hedge-anchor | unit | [[graph.model.gm_hedge-anchor]] | episode texts containing hedged statements (may, signals, estimates) | no derived claim text drops a hedge marker present in its supporting episode |
| p.relation-typing | unit | [[graph.model.gm_relation-typing]] | edge records sampled from the published vocabulary | every edge label is a member of the published relation set |
