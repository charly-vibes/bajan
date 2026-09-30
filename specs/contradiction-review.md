---
id: contradiction.review
kind: intent
statement: "WHEN an LLM proposer suggests a contradicts pair between existing claims THE bajan proposal workflow SHALL hold the pair in a human-owned proposal queue that only an explicit human resolution turns into a contradicts edge or drops, never landing model opinion in the graph unapproved"
---

# proposal

The human-in-the-loop boundary for LLM-proposed contradictions, consuming
the `gm_contradicts_provenance` invariant of `graph.model` and the
transport/config seams of the extractor arc (`llm.rs`). The design
decision this spec makes explicit: the deterministic contradiction-scan
(bajan-3hg) writes `contradicts` edges directly because its edit classes
are closed and sound; an LLM proposer sees no such closure — its output
is unverified opinion. So its proposals are staged, never wired: a
proposal cites two claim keys that already exist, carries the producing
model's identity, and waits. The queue drains only through an explicit
human command with actor identity, and only approval writes the edge —
with `llm:<model_id>` producer provenance, so the graph always records
which contradictions are machine-opinion approved by a human versus
deterministically detected. A rejected pair re-enters only when a new
proposer run produces changed evidence (changed claim texts or
rationale) — the loop guard against queue churn. The deterministic scan
is untouched: the proposal queue adds a second producer path, it does
not alter the closed edit classes or the scan's writes.

## Constraints

| id | kind | expr | traces_to | satisfies |
|----|------|------|-----------|-----------|
| cr_proposal_entry | invariant | an LLM-proposed contradiction enters the queue only as a staged proposal citing two distinct claim keys that both exist, carrying the producer provenance `llm:<model_id>` and the rationale the model gave; no `contradicts` edge is written at proposal time, and a proposal citing a hallucinated claim key is refused honestly, never silently dropped or repaired | [[contradiction.review]] | [[graph.model.gm_relation_typing]] |
| cr_human_resolution | invariant | a proposal leaves the queue only through an explicit human resolution command carrying actor identity — approve (write exactly one `contradicts` edge with `llm:<model_id>` producer provenance, the sanctioned human-gated write path of `gm_contradicts_provenance`) or reject (drop only the proposal); there is no automated path from queue entry to edge write, and a resolution without actor identity is refused | [[contradiction.review]] | [[graph.model.gm_contradicts_provenance]] |
| cr_repropose_guard | invariant | a rejected pair re-enters the queue only when a new proposer run produces changed evidence for it — a changed pair fingerprint (claim texts or rationale) — and never by re-running the proposer on unchanged inputs; the audit record of the prior rejection is not erased | [[contradiction.review]] | |
| cr_queue_transparency | invariant | the proposal queue is inspectable: a list command returns every open proposal with both claim keys, producer, rationale, and queue age, ordered oldest-first — a human can see what they are deciding and what they are leaving undecided | [[contradiction.review]] | |
| cr_scan_untouched | invariant | the deterministic contradiction-scan pass is unchanged by the proposal queue — its closed edit classes, pass identity, and edge writes follow `gm_contradicts_provenance` alone; the queue never feeds edges into, filters, or suppresses the scan | [[contradiction.review]] | |

## Model

### States

- `proposed`
- `resolved_approved`
- `resolved_rejected`

### Transitions

| id | from | to | guard |
|----|------|----|-------|
| approve | proposed | resolved_approved | [[contradiction.review.cr_human_resolution]] |
| reject | proposed | resolved_rejected | [[contradiction.review.cr_human_resolution]] |
| repropose | resolved_rejected | proposed | [[contradiction.review.cr_repropose_guard]] |

## Properties

| id | kind | derives_from | generator | predicate |
|----|------|--------------|-----------|-----------|
| p_proposal_entry | unit | [[contradiction.review.cr_proposal_entry]] | proposer runs over seeded stores with valid pairs, repeated pairs, self-pairs, and hallucinated claim keys | valid pairs stage as open proposals with producer provenance and no `contradicts` edge; duplicates, self-pairs, and hallucinated keys are refused honestly |
| p_human_resolution | unit | [[contradiction.review.cr_human_resolution]] | resolution attempts through automated paths and the human command, with and without actor identity, over approved and rejected proposals | only explicit actor-identified commands resolve a proposal; approval writes exactly one provenanced `contradicts` edge, rejection writes none; exactly one audit record per resolution |
| p_repropose_guard | unit | [[contradiction.review.cr_repropose_guard]] | re-proposals of rejected pairs with unchanged and changed fingerprints | unchanged re-proposals are refused with the prior rejection audit intact; changed fingerprints re-queue the pair once |
| p_queue_transparency | unit | [[contradiction.review.cr_queue_transparency]] | queues with proposals of varied ages, producers, and rationales | the list command returns every open proposal with producer, rationale, and age, oldest-first; nothing is hidden |
| p_scan_untouched | unit | [[contradiction.review.cr_scan_untouched]] | scan passes run before, during, and after proposal-queue activity | scan reports and written edges are identical with an empty queue and with queued/resolved proposals present |
