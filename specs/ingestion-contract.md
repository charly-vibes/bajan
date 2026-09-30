---
id: ingestion.contract
kind: intent
statement: "WHEN a converter submits a normalized episode stream THE bajan ingest pipeline SHALL persist each episode verbatim with its immutable source metadata"
---

# contract

bajan is format-agnostic by design: it never parses source formats. PDF
page extraction, EPUB chapter splitting, markdown frontmatter — all of it
happens in external converters upstream of bajan. The tool's public input
interface is the normalized episode stream defined here: stable source id,
verbatim text, structural locators (required when derivable, otherwise an
explicit absent marker that cannot collide with a real locator value), and
source metadata. This supersedes the page-oriented architecture of the
earlier knowledge-assistant spec
(`docs/knowledge-assistant-ears-spec.md`); its defect classes (evidence
capture, cutoff fidelity, no fabrication) remain the acceptance bar. This
constraint set publishes the stream schema as a contract: converter specs
will point at it with `satisfies`.

The resubmission policy is decided here (bajan-6sw): an unchanged
re-submission of a persisted episode is a no-op; a mutated re-submission is
rejected with a conflict reason — deterministically updating would orphan
claims whose lineage text no longer matches the episode, violating the
hedge-anchor discipline; a rejected id may be re-submitted with a corrected
record through the normal acceptance path; intra-stream duplicate ids are
resolved first-wins with per-record rejection; a zero-episode stream is a
valid no-op; and the produced graph is independent of episode order.
Extraction state stays orthogonal to ingestion (bajan-42y): an episode
parked by extraction after repeated call failure is persisted, so its
unchanged re-submission is already_persisted; the park and its retry
accounting live in extraction run records (`ex_park_requeue` in
extraction.claims), never on ingest outcome records. Deletion is
tombstoned (bajan-vye): deleting an episode — permitted only when no
claim supports it, per `gm_lineage_survives` in graph.model — records a
deletion tombstone for its stable id, so re-ingesting the stream never
resurrects it; recreation requires an explicit operator revive, after
which the id goes through the normal acceptance path again.

## Constraints

| id | kind | expr | traces_to |
|----|------|------|-----------|
| ic_verbatim | invariant | an episode persists its verbatim text plus a locator (heading anchor, text fragment, page/slide) or an explicit absent marker when none is derivable — absent is represented so it can never collide with a real locator value — plus immutable source metadata (stable id, source type, data cutoff, authority tier, optional workspace tags), and any converter-supplied structure markers (section boundaries, links) | [[ingestion.contract]] |
| ic_malformed | invariant | an episode whose verbatim text is absent, empty, or whitespace-only, or that lacks a stable id or required source metadata, is rejected and never persisted | [[ingestion.contract]] |
| ic_id_canon | invariant | a stable id is a nonempty, non-whitespace-only string in NFC normalization; id equality is decided on the normalized form — two records whose ids differ only by Unicode normalization form denote the same episode | [[ingestion.contract]] |
| ic_unique_id | invariant | the store holds at most one persisted episode per stable id, independent of submission order, interleaving, or concurrency; a submission matching an already-persisted id never creates a second episode | [[ingestion.contract]] |
| ic_batch_resume | invariant | episode persistence is transactional per episode (each episode all-or-nothing) and batch atomicity is not guaranteed; re-submitting a stream after a partial persistence converges to the graph a single uninterrupted submission would have produced | [[ingestion.contract]] |
| ic_idempotent | invariant | re-ingesting an already-ingested episode stream yields a graph identical to the one it produced before — no duplicate episodes, no mutated provenance | [[ingestion.contract]] |
| ic_no_format_parsing | invariant | the ingest pipeline consumes only the normalized episode stream; it performs no format-specific parsing and derives no structure from file syntax | [[ingestion.contract]] |
| ic_mutated_resubmit | invariant | a re-submission of a stable id already persisted in the store whose verbatim text, locator, or source metadata differs from the persisted episode is rejected with a machine-readable conflict reason; the persisted episode is never updated in place and never duplicated — updating would orphan claims whose lineage text no longer matches, violating the hedge-anchor discipline | [[ingestion.contract]] |
| ic_corrected_resubmit | invariant | a stable id in state rejected may be re-submitted with a corrected record; the corrected record passes through the same acceptance path as a fresh episode and persists exactly once | [[ingestion.contract]] |
| ic_state_orthogonal | invariant | an ingest outcome record reflects only ingestion outcomes with their own machine-readable reasons (`malformed`, `conflict`, `duplicate`); extraction state never appears on it — an episode parked in extraction's `rejected` after repeated call failure is persisted, so its unchanged re-submission is `already_persisted`, and the parked episode's retry accounting lives exclusively in extraction run records | [[ingestion.contract]] |
| ic_deleted_tombstone | invariant | deleting a persisted episode — permitted only when no claim supports it, per `gm_lineage_survives` in graph.model — removes the episode and records a deletion tombstone for its stable id; any later submission of a tombstoned id, unchanged or mutated, is rejected with a machine-readable deleted-id reason; the tombstone survives every re-submission, so the graph after ingest → delete → re-ingest equals the plain-ingest graph exactly minus the deleted episode — re-ingest never resurrects a deleted episode | [[ingestion.contract]] |
| ic_deleted_recreate | invariant | a tombstoned stable id becomes re-ingestable only through an explicit operator revive action that clears the tombstone; revive alone changes nothing until a new submission, which then persists through the normal acceptance path exactly once — no automated path (re-ingest, batch replay, dump-recreate rebuild) clears a tombstone | [[ingestion.contract]] |
| ic_batch_duplicate | invariant | within one submitted stream, the first occurrence of a stable id persists and every later occurrence of that id in the same stream is rejected with a machine-readable duplicate reason; rejection is per record and never extends to other episodes in the batch | [[ingestion.contract]] |
| ic_empty_stream | invariant | a stream with zero episodes is a valid no-op: ingest succeeds, emits zero outcome records, and leaves the graph unchanged — it is never an error | [[ingestion.contract]] |
| ic_order_insensitive | invariant | ingesting the same episode stream in any episode order yields an identical graph, both on first ingest and when re-ingesting an already-ingested stream | [[ingestion.contract]] |
| ic_date_fidelity | advisory | a persisted episode's data cutoff is the source's cutoff when discoverable, an explicit absent marker otherwise — never the ingest timestamp | [[ingestion.contract]] |
| ic_stream-schema | extension_point | the episode-stream record schema (field names, types, requiredness, including the absent-marker representation for locator and cutoff fields) is published for converters to conform to | [[ingestion.contract]] |
| ic_outcome-schema | extension_point | the per-episode ingest outcome record schema (`persisted`, `already_persisted`, `rejected` with machine-readable reason) is published for converter specs to conform to | [[ingestion.contract]] |

## Model

### States

- `received`
- `persisted`
- `rejected`

### Transitions

| id | from | to | guard |
|----|------|----|-------|
| accept | received | persisted | [[ingestion.contract.ic_verbatim]] |
| reject | received | rejected | [[ingestion.contract.ic_malformed]] |
| reingest | persisted | persisted | [[ingestion.contract.ic_idempotent]] |
| reject_conflict | received | rejected | [[ingestion.contract.ic_mutated_resubmit]] |
| reject_duplicate | received | rejected | [[ingestion.contract.ic_batch_duplicate]] |
| resubmit_corrected | rejected | persisted | [[ingestion.contract.ic_verbatim]] |

## Properties

| id | kind | derives_from | generator | predicate |
|----|------|--------------|-----------|-----------|
| p_verbatim | unit | [[ingestion.contract.ic_verbatim]] | arbitrary episode records with random locator forms | persisted episode round-trips: stored text, locator, and metadata equal the submitted record |
| p_idempotent | unit | [[ingestion.contract.ic_idempotent]] | the same episode stream submitted twice in sequence, a mutated variant reusing an existing id with altered text, plus a shuffled re-submission of the same stream | re-ingest yields a graph identical to the one before it, in original and shuffled order alike; a mutated re-submission is rejected with a conflict reason — it never duplicates or updates an episode |
| p_mutated_resubmit | unit | [[ingestion.contract.ic_mutated_resubmit]] | a persisted stream, then re-submissions of existing ids with altered text, altered locator, and altered metadata | every mutated re-submission is rejected with a conflict reason; the graph before and after the rejection is identical; no episode is duplicated |
| p_no_format_parsing | unit | [[ingestion.contract.ic_no_format_parsing]] | episode streams with arbitrary trailing format artifacts (frontmatter, page markers) | ingest output depends only on stream fields, never on unparsed bytes |
| p_date_fidelity | unit | [[ingestion.contract.ic_date_fidelity]] | episodes with missing, malformed, and valid cutoff dates | stored cutoff equals source cutoff when present, explicit absent marker otherwise; never ingest time |
| p_malformed | unit | [[ingestion.contract.ic_malformed]] | episode records missing verbatim text (including empty and whitespace-only text), a stable id, or required metadata | no malformed record reaches `persisted`; every malformed record is `rejected` |
| p_id_canon | unit | [[ingestion.contract.ic_id_canon]] | id pairs differing only by NFC vs NFD form, plus empty and whitespace-only ids | normalization-variant ids resolve to one episode idempotently; empty or whitespace-only ids are rejected |
| p_unique_id | unit | [[ingestion.contract.ic_unique_id]] | overlapping streams submitted sequentially and interleaved (concurrently) | exactly one persisted episode per stable id; the interleaved final graph equals the sequential one |
| p_batch_resume | unit | [[ingestion.contract.ic_batch_resume]] | streams interrupted after each k of N episodes, then fully re-submitted | the final graph equals the graph of a single uninterrupted submission; no episode is half-persisted |
| p_outcome_schema | unit | [[ingestion.contract.ic_outcome-schema]] | streams producing every outcome class (new, duplicate, malformed) | every ingest run emits exactly one outcome record per submitted episode and the published outcome schema validates each record |
| p_stream_schema | unit | [[ingestion.contract.ic_stream-schema]] | record instances sampled from the published schema | schema document validates every emitted record |
| p_corrected_resubmit | unit | [[ingestion.contract.ic_corrected_resubmit]] | streams with rejected (malformed) records followed by corrected variants of the same ids | the corrected re-submission persists through the normal acceptance path exactly once |
| p_state_orthogonal | unit | [[ingestion.contract.ic_state_orthogonal]] | episodes parked by extraction, re-submitted unchanged and with mutations | the unchanged re-submission emits `already_persisted`; no ingest outcome record carries extraction state or extraction reasons; a mutated re-submission is rejected with a conflict reason as usual |
| p_deleted_tombstone | unit | [[ingestion.contract.ic_deleted_tombstone]] | a stream ingested, an episode with no supporting claims deleted, then the same stream re-submitted unchanged and shuffled | exactly the deleted id is rejected with the deleted-id reason on every re-submission; all other episodes re-ingest idempotently; the graph equals the plain-ingest graph minus the deleted episode — no resurrection in any submission order |
| p_deleted_recreate | unit | [[ingestion.contract.ic_deleted_recreate]] | tombstoned ids with and without an explicit revive, followed by unchanged and corrected submissions | without revive every submission is rejected with the deleted-id reason and the graph is unchanged; revive alone changes nothing; the first post-revive submission persists through the normal path exactly once |
| p_batch_duplicate | unit | [[ingestion.contract.ic_batch_duplicate]] | streams containing repeated ids within one batch, with differing payloads after the first occurrence | exactly the first occurrence persists; each later occurrence is rejected with a duplicate reason; all other episodes in the batch persist |
| p_empty_stream | unit | [[ingestion.contract.ic_empty_stream]] | zero-episode streams | ingest succeeds with zero outcome records and an unchanged graph |
| p_order_insensitive | unit | [[ingestion.contract.ic_order_insensitive]] | a stream and random shuffles of it, ingested fresh and as re-ingest over an already-ingested store | every episode order yields an identical graph |
