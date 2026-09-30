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
lineage walk. Deletion leaves a tombstone (`ic_deleted_tombstone` in
ingestion.contract): re-ingest never resurrects a deleted episode, and
recreation requires the explicit revive action — the same
no-automated-recovery discipline as superseded claims' explicit
re-stage. Claims are proposed from the episode stream published by the
`ingestion.contract` spec; the proposal mechanism is specced in
`extraction.claims` and out of scope here. Adoption into the graph is
human-only: the actor requirement (`gm_human_adopt`) follows fpa's
kb-store/kb-review pipeline (`openspec/changes/update-extraction-provenance/design.md`
D2), which independently arrived at the same no-automated-accept rule.

## Constraints

| id | kind | expr | traces_to |
|----|------|------|-----------|
| gm_reified | invariant | every claim is a node with at least one `derived_from` edge to an episode node; a claim synthesized from N sources carries N lineage edges, one per episode | [[graph.model]] |
| gm_human_adopt | invariant | a claim moves from `proposed` to `active` only through an explicit human accept action (CLI `bajan adopt`; batch accepts of multiple claims in one session are permitted) — no automated stage, re-ingest, or merge may set `active` — and every accepted claim's transition is individually recorded with actor and timestamp, one audit record per claim | [[graph.model]] |
| gm_lineage_survives | invariant | invalidation sets `invalid_at` and never destroys lineage; a claim is deleted only when no episode supports it any longer; episode deletion — permitted only when no claim supports the episode — records a tombstone for its stable id per `ic_deleted_tombstone` in ingestion.contract, so re-ingest never resurrects a deleted episode and recreation requires the explicit revive action per `ic_deleted_recreate` | [[graph.model]] |
| gm_deterministic_er | invariant | entity resolution is deterministic (normalization plus curated alias lists); unmatched mentions create new entity nodes; merge candidates are proposed as `possible_duplicate_of` edges into a review queue; no silent or automatic merge | [[graph.model]] |
| gm_merge_keeps | invariant | a merge is performed only after explicit human resolution of a review candidate, and it preserves every lineage edge from both sides; rejecting a candidate removes only its `possible_duplicate_of` edge | [[graph.model]] |
| gm_schema_v2 | invariant | claim nodes carry exactly: text, valid_at, invalid_at, data_cutoff, status, scope, source_type, evidence — evidence is a verbatim span of the supporting episode text plus its episode locator, or the typed absent marker (never a value that can collide with a real span) when sentence alignment failed — source_type inherited from the episode; no confidence field and no vector blob on any node | [[graph.model]] |
| gm_embedded_store | invariant | the graph persists as plain rows in a single embedded SQLite database — including deletion tombstones — and is fully rebuildable from the episode stream plus extraction output plus tombstones, so a dump-recreate reproduces deletions and never resurrects a deleted episode | [[graph.model]] |
| gm_hedge_anchor | invariant | epistemic strength lives only in verbatim episode text; derived artifacts must not upgrade a hedged source statement to an unhedged claim; an evidence span must not drop a hedge marker present in the supporting episode text within the span's coverage | [[graph.model]] |
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
| adopt | proposed | active | [[graph.model.gm_human_adopt]] [[graph.model.gm_reified]] |
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
| p_schema_v2 | unit | [[graph.model.gm_schema_v2]] | claim records with extra, missing, and mistyped fields | stored claim nodes expose exactly the eight schema fields with types preserved; evidence is a verbatim span with locator or the typed absent marker |
| p_embedded_store | unit | [[graph.model.gm_embedded_store]] | seeded graphs of arbitrary size, with and without deletion tombstones | dump-then-recreate from episode stream plus extraction output plus tombstones reproduces the graph exactly, deletions included |
| p_hedge_anchor | unit | [[graph.model.gm_hedge_anchor]] | episode texts containing hedged statements (may, signals, estimates) | no derived claim text drops a hedge marker present in its supporting episode; evidence spans preserve hedge markers within their coverage |
| p_human_adopt | unit | [[graph.model.gm_human_adopt]] | adopt attempts driven through every automated path (pipeline stage, re-ingest, merge) and through the human CLI, single and batch | only explicit human accept actions move a claim to `active`; every transition record carries actor and timestamp, one per claim; automated attempts leave the claim `proposed` |
| p_relation_typing | unit | [[graph.model.gm_relation_typing]] | edge records sampled from the published vocabulary | every edge label is a member of the published relation set |
