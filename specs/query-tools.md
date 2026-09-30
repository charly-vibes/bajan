---
id: query.tools
kind: intent
statement: "WHEN a read command executes against the claim graph THE bajan query surface SHALL return only claims whose full lineage is traceable to their supporting episodes, ordered by authority-then-recency within the queried scope and under a bounded traversal budget"
---

# queries

The read side of bajan: the search/read/grep/outline surface over the
claim graph, graph-native rather than document-native. It consumes the
`gm_relation_typing` extension point published by `graph.model` (edge
vocabulary: `mentions`, `contradicts`, `supports`, `derived_from`,
`possible_duplicate_of`) and the tag-inheritance projection of
`ex_tag_inheritance` from `extraction.claims` — tags are resolved by
walking lineage at query time, never stored on the claim. The retrieval
discipline is carried over from the EARS oracle
(`docs/knowledge-assistant-ears-spec.md`): bounded budget with an honest
stopping rule (WR-USE.1), per-hit recency surfacing with stale warnings
(WR-TIME.3), authority-then-recency ranking with staleness demotion
(WR-RANK.1/.2), and scope discipline (WR-SCOPE.1) — re-expressed for a
provenance graph: ranking uses the episode's source metadata
(authority tier, data cutoff) reachable through lineage, and scope
filtering uses the resolved tag projection. Read commands never mutate:
every query is a lineage walk over stored rows, and a contradiction
query is a traversal of `contradicts` edges plus invalidation proposals,
not a semantic judgment — the graph says what conflicts, the human says
what's true.

## Constraints

| id | kind | expr | traces_to | satisfies |
|----|------|------|-----------|-----------|
| qt_lineage_traceable | invariant | every claim returned by a read command resolves, at query time, through its `derived_from` edges to at least one persisted supporting episode; a claim whose lineage walk reaches no episode is invisible to the query surface (deletion by lineage walk per `gm_lineage_survives`) | [[query.tools]] | |
| qt_bounded_traversal | invariant | every read command executes under a bounded traversal budget — a maximum walked node count and depth — and reports a typed exhausted-budget marker when results are truncated; the stopping rule is honest: `not-found` is reported when the budget completed without a match, `budget-exhausted` when it stopped early — never a silent partial answer | [[query.tools]] | |
| qt_ranking | invariant | multi-hit read results order by the supporting episodes' metadata reachable through lineage — authority tier first, then data cutoff recency; when a comparably-relevant higher-authority source is stale (beyond the staleness window) it demotes below fresh lower-authority hits; every hit surfaces its data cutoff, and the best-scoring hit carries a staleness warning when its cutoff is stale | [[query.tools]] | |
| qt_scope_filtering | invariant | a read command scoped to a workspace tag set returns only claims whose resolved tag projection (the union over its lineage edges per `ex_tag_inheritance`) intersects the requested scope; a claim outside the requested scope is either excluded or labelled with its own scope in the result — never silently blended | [[query.tools]] | |
| qt_readonly | invariant | read commands never mutate the graph — no state transition, no audit record, no derived-from edge is written by any query; a read against a claim set with active and invalid claims returns the status of every hit verbatim, never silently dropping `invalid` claims from results | [[query.tools]] | |
| qt_contradiction_query | invariant | a contradiction query returns the claim pairs joined by `contradicts` edges plus the invalidation proposals staged against the queried claims (per `ex_mutation_proposal`), each hit carrying both sides' lineage and status; the query reports graph structure only and never adjudicates which side is true | [[query.tools]] | [[graph.model.gm_relation_typing]] |
| qt_query-schema | extension_point | the query result record schema (fields, types, requiredness, including the budget-status vocabulary `complete`, `budget-exhausted` and the staleness marker form) is published for reporting and agent-tool specs to conform to | [[query.tools]] |

## Model

### States

- `queried`
- `traversed`
- `budget_exhausted`
- `answered`

### Transitions

| id | from | to | guard |
|----|------|----|-------|
| walk | queried | traversed | [[query.tools.qt_lineage_traceable]] |
| stop | traversed | budget_exhausted | [[query.tools.qt_bounded_traversal]] |
| answer | traversed | answered | [[query.tools.qt_ranking]] |
| answer_truncated | budget_exhausted | answered | [[query.tools.qt_bounded_traversal]] |

## Properties

| id | kind | derives_from | generator | predicate |
|----|------|--------------|-----------|-----------|
| p_lineage_traceable | unit | [[query.tools.qt_lineage_traceable]] | seeded graphs mixing fully-lineaged claims, lineage-broken claims, and multi-source claims | every returned claim's walk reaches a persisted episode; lineage-broken claims never appear in any result; multi-source claims return all their sources |
| p_bounded_traversal | unit | [[query.tools.qt_bounded_traversal]] | queries over graphs sized to complete and to overflow the budget at varied depths | runs that finish within budget report `complete` and full results; runs that hit the bound stop at the same node count deterministically and report `budget-exhausted` with the truncation marker; a no-match completed run reports `not-found` |
| p_ranking | unit | [[query.tools.qt_ranking]] | hit sets mixing authority tiers, cutoffs spanning the staleness window, and equal-rank ties | results order authority-first then recency; a stale high-authority hit demotes below a fresh lower-authority hit; every hit carries its cutoff and the top hit carries the stale warning when applicable |
| p_scope_filtering | unit | [[query.tools.qt_scope_filtering]] | claims with overlapping, disjoint, and unioned tag projections against varied requested scopes | results contain exactly the claims whose projected tags intersect the scope; out-of-scope claims appear only labelled, never unlabeled |
| p_readonly | unit | [[query.tools.qt_readonly]] | read commands executed against stores pre-seeded with claims in every status plus staged invalidation proposals | the store is byte-identical before and after any sequence of read commands; results include `invalid` hits with their status rather than omitting them |
| p_contradiction_query | unit | [[query.tools.qt_contradiction_query]] | graphs with contradict pairs, staged invalidation proposals, and unconnected claims | returned pairs are exactly the `contradicts` edges over the queried claims plus their staged proposals; each hit carries both lineages and statuses; no pair is dropped or adjudicated |
| p_query-schema | unit | [[query.tools.qt_query-schema]] | result records sampled from the published schema | every result record validates against the published schema with the closed budget-status vocabulary |
