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
verbatim text, structural locators (required when derivable, otherwise the
literal `unknown`), and source metadata. This supersedes the page-oriented
architecture of the earlier knowledge-assistant spec
(`docs/knowledge-assistant-ears-spec.md`); its defect classes (evidence
capture, cutoff fidelity, no fabrication) remain the acceptance bar. This
constraint set publishes the stream schema as a contract: converter specs
will point at it with `satisfies`.

## Constraints

| id | kind | expr | traces_to |
|----|------|------|-----------|
| ic_verbatim | invariant | an episode persists its verbatim text plus a locator (heading anchor, text fragment, page/slide, or the literal `unknown` when none is derivable) and immutable source metadata: stable id, source type, data cutoff, authority tier | [[ingestion.contract]] |
| ic_malformed | invariant | an episode missing verbatim text, a stable id, or required source metadata is rejected and never persisted | [[ingestion.contract]] |
| ic_idempotent | invariant | re-ingesting an already-ingested episode stream yields a graph identical to the one it produced before — no duplicate episodes, no mutated provenance | [[ingestion.contract]] |
| ic_no_format_parsing | invariant | the ingest pipeline consumes only the normalized episode stream; it performs no format-specific parsing and derives no structure from file syntax | [[ingestion.contract]] |
| ic_date_fidelity | advisory | a persisted episode's data cutoff is the source's cutoff when discoverable, `unknown` otherwise — never the ingest timestamp | [[ingestion.contract]] |
| ic_stream-schema | extension_point | the episode-stream record schema (field names, types, requiredness) is published for converters to conform to | [[ingestion.contract]] |

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
| p_date_fidelity | unit | [[ingestion.contract.ic_date_fidelity]] | episodes with missing, malformed, and valid cutoff dates | stored cutoff equals source cutoff when present, `unknown` otherwise; never ingest time |
| p_malformed | unit | [[ingestion.contract.ic_malformed]] | episode records missing verbatim text, a stable id, or required metadata | no malformed record reaches `persisted`; every malformed record is `rejected` |
| p_stream_schema | unit | [[ingestion.contract.ic_stream-schema]] | record instances sampled from the published schema | schema document validates every emitted record |
