---
id: entity.review
kind: intent
statement: "WHEN a deterministic entity-resolution pass proposes a merge candidate THE bajan review workflow SHALL hold the merge in a human-owned queue that only an explicit human resolution approves or rejects, preserving both sides' lineage on approval"
---

# review

The human-in-the-loop boundary of entity resolution, consuming the
`gm_relation_typing` extension point (`possible_duplicate_of`) and the
deterministic-ER invariants of `graph.model` (`gm_deterministic_er`,
`gm_merge_keeps`). The grill decision this spec makes explicit: ER is
deterministic-only — normalization plus curated alias lists — and the
merge path is human-approved. The queue is where the machine's
uncertainty becomes the human's agenda: `possible_duplicate_of` edges
are proposals, never silent merges, and the review commands are the only
HITL surface in an otherwise AFK pipeline. Every resolution decision
(approve or reject) writes an audit record — actor, timestamp, both
entity ids, decision — because a merge that loses provenance is the
graph-killing defect (`gm_lineage_survives`): an approved merge
preserves the union of both sides' lineage edges; a rejection removes
only the proposal edge. Re-proposing a rejected pair is allowed only
with new evidence (a new mention or alias match), never by re-running
the same deterministic pass — a loop guard against queue churn.

## Constraints

| id | kind | expr | traces_to |
|----|------|------|-----------|
| er_queue_entry | invariant | a merge candidate enters the review queue only as a `possible_duplicate_of` edge written by the deterministic ER pass (normalization plus curated alias lists per `gm_deterministic_er`); no LLM pass and no review command may create a candidate pair — the queue drains, it never grows by human invention | [[entity.review]] |
| er_human_resolution | invariant | a queued candidate leaves the queue only through an explicit human resolution command carrying actor identity — approve (merge) or reject (drop the proposal edge); there is no automated path from queue entry to entity merge, and an unattended queue never resolves itself | [[entity.review]] |
| er_merge_preserves | invariant | an approved merge preserves the union of both entities' lineage edges (every `derived_from` edge from both sides survives onto the merged entity) and writes one audit record per resolution — actor, timestamp, both entity ids, decision — per `gm_merge_keeps` | [[entity.review]] |
| er_reject_drops_edge | invariant | a rejected candidate loses only its `possible_duplicate_of` edge — both entities, their lineage, and their claims are untouched; the rejection is recorded in the audit trail with actor and timestamp | [[entity.review]] |
| er_repropose_guard | invariant | a rejected pair re-enters the queue only when the deterministic pass finds new evidence for it — a new mention of either entity or a new alias-table entry — and never by re-running the pass on unchanged inputs; the audit record of the prior rejection is not erased | [[entity.review]] |
| er_queue_transparency | invariant | the queue is inspectable: a list command returns every open candidate with both entities' ids, alias/normalization evidence that produced the proposal, claim counts, and queue age, ordered oldest-first — a human can see what they are deciding and what they are leaving undecided | [[entity.review]] |
| er_review-schema | extension_point | the review-queue record schema (fields, types, requiredness, including the decision vocabulary `approved`, `rejected` and the audit-record shape) is published for HITL tooling specs to conform to | [[entity.review]] |

## Model

### States

- `proposed`
- `resolved_approved`
- `resolved_rejected`

### Transitions

| id | from | to | guard |
|----|------|----|-------|
| approve | proposed | resolved_approved | [[entity.review.er_human_resolution]] |
| reject | proposed | resolved_rejected | [[entity.review.er_human_resolution]] |
| repropose | resolved_rejected | proposed | [[entity.review.er_repropose_guard]] |

## Properties

| id | kind | derives_from | generator | predicate |
|----|------|--------------|-----------|-----------|
| p_queue_entry | unit | [[entity.review.er_queue_entry]] | ER passes over mention streams producing candidates, plus attempts to inject candidates via review commands and synthetic LLM output | every queue entry traces to a `possible_duplicate_of` edge from the deterministic pass; injected candidates are refused; the queue contents equal the open proposal edges |
| p_human_resolution | unit | [[entity.review.er_human_resolution]] | resolution attempts through every automated path (pipeline stage, re-ingest, batch job) and through the human command, with and without actor identity | only explicit human commands with actor identity resolve a candidate; automated attempts leave it `proposed`; resolutions without actor identity are refused |
| p_merge_preserves | unit | [[entity.review.er_merge_preserves]] | approved merges over entity pairs with disjoint, overlapping, and multi-source lineage sets | the merged entity's lineage equals the union of both sides' pre-merge edges; exactly one audit record per resolution with actor, timestamp, both ids, and decision |
| p_reject_drops_edge | unit | [[entity.review.er_reject_drops_edge]] | rejections over pairs with claims and lineage on both sides | after rejection the `possible_duplicate_of` edge is gone and everything else — entities, lineage, claims, statuses — is unchanged; the audit trail gains one rejection record |
| p_repropose_guard | unit | [[entity.review.er_repropose_guard]] | re-runs of the deterministic pass over unchanged inputs and inputs with new mentions or alias entries | unchanged-input re-runs never re-queue a rejected pair; new-evidence re-runs re-queue it once with the prior rejection record intact |
| p_queue_transparency | unit | [[entity.review.er_queue_transparency]] | queues with candidates of varied ages, evidence kinds, and claim counts | the list command returns every open candidate with evidence, claim counts, and age, oldest-first; nothing is hidden from the human reviewing it |
| p_review-schema | unit | [[entity.review.er_review-schema]] | review-queue records sampled from the published schema | every queue and audit record validates against the published schema with the closed decision vocabulary |
