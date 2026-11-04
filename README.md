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
cargo install --path .          # from a checkout — the bajan CLI only
cargo install bajan             # once published
```

Requires Rust 1.85+ (edition 2024). The source converters live in the
separate `converters/` crate and are built separately:

```sh
cargo build --release --manifest-path converters/Cargo.toml
# binaries land in converters/target/release/{epub,html,md,pdf}2bajan
```

## Data & state

Your whole graph is one SQLite file — **`bajan.db` in the current directory**
by default. Point every command at the same file (or a different one) with
the global `--db` flag: `bajan --db corpus.db extract`. Claims are referenced
by numeric **claim keys**, which appear in extract/query output — text mode
shows them as `#N` in the hit list.

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
# BAJAN_MAX_SPEND_USD is REQUIRED — without it the extractor refuses to
# run; when a call would push known spend past the cap, the pass stops
# (later episodes stay pending; rerun with a raised cap to resume).
# BAJAN_EXTRACTOR_TIMEOUT_SECS raises the per-call HTTP timeout from the
# 120s default — reasoning models under load exceed it (must be a
# positive integer; misconfiguration refuses at selection).
BAJAN_EXTRACTOR=llm \
BAJAN_EXTRACTOR_MODEL=deepseek/deepseek-v4-flash \
BAJAN_EXTRACTOR_BASE_URL=https://openrouter.ai/api/v1 \
BAJAN_EXTRACTOR_API_KEY_ENV=OPENROUTER_API_KEY \
BAJAN_MAX_SPEND_USD=0.50 \
BAJAN_EXTRACTOR_TIMEOUT_SECS=600 \
bajan extract

# 4. Human-in-the-loop adoption of staged claims
#    (claim keys come from the extract output / `bajan query` hit list)
bajan adopt 1 2 3

# 5. Entity-review queue — the only human-only resolutions in the pipeline
bajan review list
bajan review approve <from> <to> --actor you   # merge a duplicate pair
bajan review reject <from> <to> --actor you    # drop the merge candidate

# 6. Query — ranked hits with lineage-traceable provenance
bajan query "profit" --scope workspace:dev
bajan query "profit" --json | jq '.data.hits'

# 7. Contradictions — closed rule set over specific claims, plus an
#    optional LLM proposal pass
bajan contradict 1 2
bajan scan                                     # deterministic contradicts pass
bajan propose            # LLM-assisted: stages proposals, never writes edges
bajan proposal list
bajan proposal approve <from> <to> --actor you  # writes the contradicts edge

# 8. Score an extraction run against a golden corpus (see
#    "Evaluating extraction quality" below; the adjudicated corpus
#    lives in evals/fixtures/gold-fi4e-v1.json)
bajan eval claims --corpus evals/fixtures/gold-fi4e-v1.json --run run.json
```

Concepts:

- **Episode** — a normalized span of source text with a locator and
  authority metadata; persisted verbatim, never rewritten.
- **Claim** — a claim node derived from episodes, staged until adopted;
  lineage edges make provenance part of the graph structure.
- **Envelope** — every command answers `{ok, envelope_version, envelope_kind,
  data}`; exit status mirrors `ok`.

## Evaluating extraction quality

`bajan eval claims` scores an extraction run against a **golden corpus** —
a versioned fixture of episodes whose true claim sets a human adjudicated
(`specs/eval-claims.md`). It publishes precision, recall, and hedge
preservation with raw counts, one per-case judgment per candidate. Scoring
is deterministic: no embeddings, no LLM judgment, no store access, no wall
clock — identical inputs reproduce byte-identical results, and an eval run
can never touch a real graph.

The golden corpus is a JSON file; every episode carries the claims the
text **should** yield. Constraints: the corpus `version` is required,
episode ids and claim texts/spans must be non-empty and unique, `source_count`
must be ≥ 1, and `hedges` may only use the anchored vocabulary `may, might,
could, possibly, appears, seems, signals, estimates, reportedly, tentative,
uncertain` (anything else is rejected):

```json
{
  "id": "gold-fi4e", "version": "1",
  "episodes": [{
    "id": "ep-1", "text": "Profits may vary by run.",
    "golden_claims": [{
      "text": "Profits may vary by run.", "source_count": 1,
      "evidence_span": "Profits may vary by run.", "hedges": ["may"]
    }]
  }]
}
```

The run manifest is the extraction output to score — candidates with their
evidence spans, stamped with the run and extractor identities. Export a real
manifest from a store with `bajan eval manifest` (bajan-t6z): it carries the
store's live candidates (staged and active claims) for the extractor version
you name, skipping unknown-evidence candidates and reporting the skip count.
With one extraction run in the store the version is picked automatically;
with several, `--extractor-version` selects one (the export refuses to mix
runs into one manifest). Text mode prints the bare manifest JSON so the
round trip is two commands, and `--out <file>` writes it directly:

```console
$ bajan eval manifest --db book.db --out run.json
$ bajan eval claims --corpus gold.json --run run.json
```

(The extract envelope itself still carries counts, not claims — the store is
the run record.) Candidate `episode_id`s must match corpus episode ids: the
published record counts candidates naming unknown episodes in
`unknown_episode_ids`, and scoring hard-refuses (`ok:false`, non-zero exit)
when **every** candidate references an unknown episode — a run scored
against the wrong corpus fails loudly instead of yielding plausible-looking
low numbers:

```json
{
  "run_id": "pf2-2026-10-01", "extractor_version": "llm-deepseek/deepseek-v4-flash-0.1.0",
  "claims": [{"episode_id": "ep-1", "text": "Profits vary by run.",
              "evidence_span": "Profits may vary by run."}]
}
```

Scoring semantics, in order:

1. **Content match** — candidate text equals a golden claim's text after
   whitespace-collapse + lowercase with hedge-marker words ignored on both
   sides (a candidate that adds or drops a hedge still locates its target).
2. **Span check** — the candidate's `evidence_span` must locate inside the
   episode text (whitespace-collapsed containment, same predicate as
   extraction's evidence gate). Fails → `content-miss`, never a match.
3. **Hedge fidelity** — every hedge marker the golden claim lists must
   appear in the candidate text. Missing → `hedge-dropped`: a precision
   failure distinct from a content miss, and the golden claim stays open
   for a later clean candidate.
4. Otherwise → `matched`. Unmatched candidates score `spurious`.

The example pair above demonstrates the strictest rule: the candidate
dropped the hedge its golden claim lists, so it scores `hedge-dropped` —
precision 0, recall 0, hedge preservation 0, never a plain match.

Denominators are always reported in `counts`; a metric with a zero
denominator (no candidates, no golden claims, no hedge-bearing golden
claims) is `null` **and** named in `unknown_metrics` — never invented.
Run with `--json` for the machine envelope (`data.counts`,
`data.unknown_metrics`); text mode renders the same content in human form.
`data.results_row` is the ready-to-append row for the eval-claims table in
`evals/results.md` — the date cell is a placeholder you fill at append
time (scoring itself never reads the wall clock, so reruns reproduce the
row byte-for-byte). Gold labels are human sign-off by design: no
model-generated gold labels, and gold fixtures land in a reviewed commit
before any metrics run.

## Design in one line

Deterministic-first: no LLM where traditional IR/NLP works; NLP/IR crates are
exact-pinned so a rebuild reproduces the same store; LLM passes only propose —
humans approve. Specifications under `specs/` are linted by
[specodelic](https://crates.io/crates/specodelic) and gate every commit.

## Known limitations

- **Parked episodes are silent.** When the LLM extractor hits transient
  provider overload it parks 10–24% of episodes per pass; reruns recover
  some, but there is no per-episode failure telemetry yet (bajan-gso).
- **Spend cap needs `usage.cost`.** The LLM extractor requires
  `BAJAN_MAX_SPEND_USD` and stops the pass when known spend reaches the
  cap (bajan-8cc) — but providers that do not report per-call cost in
  `usage.cost` (e.g. plain OpenAI) cannot be accounted: one
  unaccountable call fails the next call closed (never silently
  unbounded). Bound spend at the provider as well.
- **Extraction precision/recall numbers are not published yet.** The eval
  harness is built (`bajan eval claims`, bajan-2k4) but no golden corpus
  has been adjudicated yet — gold labels are human sign-off; until a
  corpus lands and a run is scored, adopt with judgment.
- **Per-call costs are logged, not reported.** The extractor logs one
  stderr line per successful LLM call carrying the response's exact
  `usage.cost` and the running spend against the cap (bajan-3w4);
  aggregate spend surfaces on the extract report only when a cap stops
  the pass (bajan-8cc).
- Burn-in findings and failure profile:
  [`docs/burnin-deepseek-v4-flash-profit-first.org`](docs/burnin-deepseek-v4-flash-profit-first.org).

## Development

```sh
just ci        # full gate: fmt, clippy, tests, spec lint, hooks parity
just --list    # all recipes
```

Issue tracking and burn-in history live in `.beads/`; the spec corpus is
under `specs/` with change proposals under `openspec/`.
