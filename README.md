# bajan

A spec-driven knowledge-graph pipeline CLI — ingest sources, extract claims,
query the graph. Rust, SQLite-backed triple store, provenance as graph
structure.

Encodes the best practices from
[`knowledge-graphs-report.md`](knowledge-graphs-report.md) as enforced,
lint-checked specifications (`specs/`, `openspec/`).

Part of the charly-vibes suite; emits suite-envelope JSON on every command.

## Status

**v0.1.0 — early release.** The deterministic pipeline is spec-complete and
CI-gated (specodelic lint, contract tests, property tests). The LLM
extraction path is smoke-tested against real providers at book scale but
young — see [Known limitations](#known-limitations) before relying on it.

## Install

```sh
cargo install --path .          # from a checkout
cargo install bajan             # once published
```

Requires Rust 1.85+ (edition 2024).

## Quickstart

Every command writes a JSON suite envelope to stdout; add `--json` after the
subcommand for machine consumption (text mode renders the same envelope in a
short human form — same content, never a second format).

```sh
# 1. Convert a source into a normalized episode stream (JSON array)
converters/target/release/epub2bajan book.epub > episodes.json

# 2. Ingest — episodes persist verbatim, each gets an outcome record
bajan ingest < episodes.json

# 3. Extract claims (default: deterministic `atomic` extractor)
bajan extract

# or the LLM extractor (OpenAI-compatible endpoint):
BAJAN_EXTRACTOR=llm \
BAJAN_EXTRACTOR_MODEL=deepseek/deepseek-v4-flash \
BAJAN_EXTRACTOR_BASE_URL=https://openrouter.ai/api/v1 \
BAJAN_EXTRACTOR_API_KEY_ENV=OPENROUTER_API_KEY \
bajan extract

# 4. Human-in-the-loop adoption of staged claims
bajan adopt

# 5. Entity-review queue: the only human-only resolutions in the pipeline
bajan er list
bajan er approve <claim-key> --actor you
bajan er reject <claim-key> --actor you

# 6. Query — ranked hits with lineage-traceable provenance
bajan query "profit" --scope workspace:dev
bajan query "profit" --json | jq '.data.hits'

# 7. Contradictions — closed rule set, plus an optional LLM proposal pass
bajan contradict
bajan propose            # LLM-assisted: stages proposals, never writes edges
bajan proposal list
bajan approve <id> --actor you
```

Concepts:

- **Episode** — a normalized span of source text with a locator and
  authority metadata; persisted verbatim, never rewritten.
- **Claim** — a claim node derived from episodes, staged until adopted;
  lineage edges make provenance part of the graph structure.
- **Envelope** — every command answers `{ok, envelope_version, envelope_kind,
  data}`; exit status mirrors `ok`.

## Design in one line

Deterministic-first: no LLM where traditional IR/NLP works; NLP/IR crates are
exact-pinned so a rebuild reproduces the same store; LLM passes only propose —
humans approve. Specifications under `specs/` are linted by
[specodelic](https://crates.io/crates/specodelic) and gate every commit.

## Known limitations

- **Parked episodes are silent.** When the LLM extractor hits transient
  provider overload it parks 10–24% of episodes per pass; reruns recover
  some, but there is no per-episode failure telemetry yet (bajan-gso).
- **The eval harness is not built.** Extraction precision/recall numbers are
  not yet published (bajan-2k4, deferred); adopt with judgment.
- **Cost/usage reporting is thin.** Per-call usage cost is not logged from
  provider responses yet (bajan-3w4); burn-in measured ~$0.02 for a
  50-episode book with a $0.50 cap.
- Burn-in findings and failure profile:
  [`docs/burnin-deepseek-v4-flash-profit-first.org`](docs/burnin-deepseek-v4-flash-profit-first.org).

## Development

```sh
just ci        # full gate: fmt, clippy, tests, spec lint, hooks parity
just --list    # all recipes
```

Issue tracking and burn-in history live in `.beads/`; the spec corpus is
under `specs/` with change proposals under `openspec/`.
