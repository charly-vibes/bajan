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

```sh
md2bajan < doc.md    | bajan --db graph.db ingest
html2bajan < page.html | bajan --db graph.db ingest
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

## Formats not covered (yet) — and their caveats

Per the bajan-15i converter table:

- **EPUB** (`epub` crate): chapter splitting maps naturally to the
  heading-locator contract, but EPUB is a ZIP container — extraction adds
  a failure mode (container corruption, DRM) the flat-text converters
  don't have. Not implemented; a future `epub2bajan` would reuse
  `html_episodes` per chapter document.
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
