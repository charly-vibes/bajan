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

## Properties

| id | kind | derives_from | generator | predicate |
|----|------|--------------|-----------|-----------|
| p_verbatim | unit | [[ingestion.contract.ic_verbatim]] | arbitrary episode records with random locator forms | persisted episode round-trips: stored text, locator, and metadata equal the submitted record |
| p_idempotent | unit | [[ingestion.contract.ic_idempotent]] | the same episode stream submitted twice in sequence, plus a mutated variant reusing an existing id with altered text | re-ingest yields a graph identical to the one before it; a mutated re-submission either deterministically updates or is rejected — it never duplicates an episode |
| p_no_format_parsing | unit | [[ingestion.contract.ic_no_format_parsing]] | episode streams with arbitrary trailing format artifacts (frontmatter, page markers) | ingest output depends only on stream fields, never on unparsed bytes |
| p_date_fidelity | unit | [[ingestion.contract.ic_date_fidelity]] | episodes with missing, malformed, and valid cutoff dates | stored cutoff equals source cutoff when present, explicit absent marker otherwise; never ingest time |
| p_malformed | unit | [[ingestion.contract.ic_malformed]] | episode records missing verbatim text (including empty and whitespace-only text), a stable id, or required metadata | no malformed record reaches `persisted`; every malformed record is `rejected` |
| p_id_canon | unit | [[ingestion.contract.ic_id_canon]] | id pairs differing only by NFC vs NFD form, plus empty and whitespace-only ids | normalization-variant ids resolve to one episode idempotently; empty or whitespace-only ids are rejected |
| p_unique_id | unit | [[ingestion.contract.ic_unique_id]] | overlapping streams submitted sequentially and interleaved (concurrently) | exactly one persisted episode per stable id; the interleaved final graph equals the sequential one |
| p_batch_resume | unit | [[ingestion.contract.ic_batch_resume]] | streams interrupted after each k of N episodes, then fully re-submitted | the final graph equals the graph of a single uninterrupted submission; no episode is half-persisted |
| p_outcome_schema | unit | [[ingestion.contract.ic_outcome-schema]] | streams producing every outcome class (new, duplicate, malformed) | every ingest run emits exactly one outcome record per submitted episode and the published outcome schema validates each record |
| p_stream_schema | unit | [[ingestion.contract.ic_stream-schema]] | record instances sampled from the published schema | schema document validates every emitted record |
