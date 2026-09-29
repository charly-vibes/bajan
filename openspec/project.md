# Project Context

## Purpose

bajan is a CLI tool that helps build knowledge graphs from LLM/agent content,
following the best practices synthesized in `knowledge-graphs-report.md`
(this repo). It is a sibling tool in the charly-vibes suite, structured like
[genesis](../genesis/) (Rust CLI, shared suite conventions).

Best practices the tool encodes (from the report):

- **Provenance as graph structure** — store sources verbatim as episode
  nodes; every derived fact links back to its sources; lineage survives
  entity merges and fact invalidation (Chalef/Graphiti).
- **Schema-first extraction** — the extractor is given a target node/edge
  schema plus ontology instructions; never free-form "extract triples".
- **Deterministic-first pipeline** — structure that already exists (headings,
  links, frontmatter) is loaded idempotently without an LLM; LLM extraction
  is reserved for facts that only exist in prose; cheap dedup (simhash/
  entropy) before any LLM pass.
- **Entity resolution as a first-class step** — embedding-based matching,
  not naive string maps.
- **Triples in boring storage first** — start in SQLite/Postgres/a file;
  earn a specialized graph DB through evals (Christian). No graph DB
  dependency in the MVP.
- **Eval before adoption** — measure retrieval accuracy with and without the
  graph; the tool should make this measurable.

## Tech Stack

- Implementation language: Rust, stable toolchain, rustfmt + clippy clean.
- CLI: clap, JSON envelope output consistent with the charly-vibes suite
  (see genesis `envelope` module).
- Storage: SQLite (via rusqlite) for the triple store in MVP — no graph DB
  dependency.
- Extraction: pluggable; deterministic loaders first, optional LLM extractor
  behind a trait.

## Project Conventions

### Code Style
- rustfmt defaults, clippy with `-D warnings`.
- Errors via `thiserror` internally, suite JSON envelope on output.

### Architecture Patterns
- Pipeline stages as separate modules: ingest → extract → resolve → link →
  query. Each stage idempotent and re-runnable.
- Provenance is structural, not logged: every triple carries edge metadata
  linking to episode nodes; merges keep all parent links; invalidation sets
  an `invalid_at` on the edge, never deletes.

### Testing Strategy
- Unit tests per stage; golden-file tests for deterministic loaders;
  property tests for entity-resolution merge invariants (lineage never
  dropped on merge).

### Git Workflow
- trunk-based; conventional commits; beads for task tracking; **specodelic
  specs are the normative requirements format** (`specs/*.md`, one file =
  one spec, four layers, `specodelic lint specs` must stay clean);
  openspec (`openspec/changes/`) remains available for larger change
  proposals when a spec needs a design doc alongside it.
- Behavioral acceptance oracle: `docs/knowledge-assistant-ears-spec.md`
  (the superseded system's defect classes); domain synthesis:
  `knowledge-graphs-report.md`.

## Domain Context

- The authoritative domain synthesis is `knowledge-graphs-report.md` at repo
  root — read it before designing any pipeline stage.
- Terms: triple, episode, entity resolution, provenance, schema-first
  extraction (glossary in the report).

## Important Constraints

- Follow the report's cost discipline: avoid LLMs wherever traditional
  IR/NLP works.
- No big-bang ontology: schema lives in a versioned, user-editable file;
  reuse existing taxonomies (schema.org, FOAF) where applicable.
- Deletion must be a graph walk (GDPR): a fact is deleted only when no
  remaining episode supports it.

## External Dependencies

- charly-vibes suite tooling managed by `ddl` (dulce-de-leche): bd (beads),
  openspec, ah/espectacular, dont, pretender, testaruda, wai, turu,
  specodelic, fabbro.
