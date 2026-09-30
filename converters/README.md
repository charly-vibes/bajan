# bajan-converters

Standalone upstream converters (bajan-quq): turn source documents into the
bajan **episode-stream** JSON schema so they can be piped straight into
`bajan ingest`.

These tools are **never bajan dependencies** — bajan's `Cargo.toml` carries
no format parsers (`ic_no_format_parsing`): the ingest pipeline consumes
only the normalized episode stream and derives no structure from file
syntax. All format parsing happens here, upstream. This crate is
deliberately not a workspace member of bajan and keeps its own lockfile.

## Tools

| Bin | Input | Output |
|-----|-------|--------|
| `md2bajan` | CommonMark on stdin | episode-stream JSON on stdout |
| `html2bajan` | HTML (html5ever) on stdin | episode-stream JSON on stdout |
| `epub2bajan` | EPUB file (binary — file argument) | episode-stream JSON on stdout |

```sh
md2bajan < doc.md    | bajan --db graph.db ingest
html2bajan < page.html | bajan --db graph.db ingest
epub2bajan book.epub   | bajan --db graph.db ingest
```

## Mapping rules

- Each heading (`#`–`######` / `h1`–`h6`) closes the current episode and
  opens a new one with locator `{"kind": "span", "value": "heading:<text>"}`.
- Text before the first heading (or a document with no headings at all)
  becomes an episode with the typed absent locator `{"kind": "absent"}`
  (ic_verbatim: absent is a distinct marker, never a colliding string).
- Paragraph boundaries are preserved as `\n\n` joins; runs of whitespace
  collapse.
- Deterministic ids: a slug of the heading text (`untitled` when absent),
  repeated anchors disambiguated as `slug-2`, `slug-3`, … — same input
  always yields the same ids, which is what bajan's resubmission policy
  (ic_idempotent / ic_mutated_resubmit) keys on.
- Excluded as code, not prose: fenced code blocks (markdown),
  `<script>`, `<style>`, `<pre>` (HTML).
- Source metadata defaults: `source_type` `markdown`/`html`,
  `data_cutoff` **absent** (ic_date_fidelity: never invent a date),
  `authority_tier` 3 (lowest), no tags. Override downstream if the source
  has real authority/cutoff facts.

Schema conformance is enforced structurally: the emitted records are
bajan's own `EpisodeRecord` type via a path dependency, and the test suite
round-trips converter output through the real `bajan::ingest::persist_stream`
(persist → re-ingest `already_persisted`).

## EPUB mapping rules (`epub2bajan`)

- Each **spine chapter document** becomes ONE episode (chapter-level
  granularity — page numbers do not exist in reflowable EPUB, so there
  are no page locators).
- Chapter text reuses the HTML rules above per spine document (head,
  script/style/pre excluded; block containers as paragraph boundaries).
- Chapter anchor: the toc label wins (toc.ncx via the `epub` crate; the
  EPUB3 nav document is parsed separately — the crate leaves it out),
  else the chapter's first heading text. Locator is
  `{"kind": "span", "value": "heading:<chapter title>"}`.
- Chapters with no toc entry and no heading get the position-based
  fallback id slug `chapter-NNN` (NNN = spine position) and keep the
  typed absent locator — never an invented heading anchor.
- Episode ids: `slug(book title)-slug(chapter title)`, deterministic and
  stream-unique with the same `-N` disambiguation discipline.
- Navigation documents (`properties="nav"`, ncx) listed on a spine are
  skipped — they are metadata, not prose.
- Metadata: `source_type` `epub`; `data_cutoff` from `dc:date` ONLY when
  it is a clean civil date `YYYY-MM-DD` (anything else — "May 2023",
  year-only, ISO datetimes — stays absent, ic_date_fidelity); tier 3; no
  tags.

**EPUB container caveats** (the extra failure mode the flat-text
converters don't have — an EPUB is a ZIP): corrupt containers, valid
zips that are not EPUBs, and DRM-encumbered files fail honestly with a
nonzero exit and a diagnostic — never a partial or garbage stream.
Obfuscated/encrypted fonts are ignored; text extraction is unaffected.

## Formats not covered (yet) — and their caveats

Per the bajan-15i converter table:

- **PDF** (`pdf-extract` or `poppler pdftotext`): the **weakest
  deterministic link**. PDF text extraction is lossy by design — reading
  order is a heuristic, columns/tables scramble, and the same file can
  extract differently across extractor versions. That directly threatens
  the verbatim-persistence contract (ic_verbatim: the episode text is the
  source text) and idempotent re-ingest (the same PDF re-converted under a
  different extractor version yields different text → `conflict` on
  resubmission). The locator-absent-marker contract absorbs the missing
  page anchors, but not the text instability. Not implemented; if it ever
  is, the caveat must be stated on every episode it emits.
