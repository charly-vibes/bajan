# FAQ

Answers to the questions that come up most often, sourced from the specs and
code. Spec paths are relative to the repository root.

## Which extractor should I choose — `atomic` or `llm`?

The default is the deterministic `atomic` extractor (`BAJAN_EXTRACTOR` unset
resolves to `atomic`; `src/extract.rs`), which segments episode text into
atomic claim-sized units without any model call. Use it first: it is free,
fast, and reproducible, and the design principle is "no LLM where traditional
IR/NLP works" (`specs/extraction-claims.md`). Reach for
`BAJAN_EXTRACTOR=llm` when the deterministic pass misses paraphrased or
implicit claims and you can pay per call — it requires an OpenAI-compatible
endpoint, an API key env var, and `BAJAN_MAX_SPEND_USD`. Either way, a
deterministic shape post-pass (`ex_shape_rejection`) runs after extraction
and the validation gate, and humans adopt claims — the extractor only
proposes (`specs/extraction-claims.md`).

## Why did extraction stop before finishing all episodes? (spend cap)

The LLM extractor requires `BAJAN_MAX_SPEND_USD` and refuses to run without
it. When a call would push the known spend past the cap, the pass stops and
the remaining episodes stay pending — the extract envelope reports it and
names `BAJAN_MAX_SPEND_USD` in the remediation (`src/extract.rs`,
bajan-8cc; per-call costs are logged to stderr, bajan-3w4). This is a
deliberate safety property, not a bug: the cap bounds spend even when a
provider misbehaves. Two caveats from `## Known limitations` in the README:
providers that do not report per-call cost in `usage.cost` cannot be
accounted (one unaccountable call fails the next call closed, never
silently unbounded), so bound spend at the provider as well. To resume,
raise the cap and rerun `bajan extract` — extracted episodes are cached, so
no call is repeated.

## Why are some episodes parked, and how do I recover them?

When the LLM extractor exhausts its retry budget on transient provider
overload (timeout / http-N / transport / schema failures), it parks the
episode in `rejected` — visible in the extract report with its failure
class and attempt count (bajan-gso). Parking is one run's honest stopping,
never a permanent state: a parked episode holds no extraction-cache entry
at the failed extractor version, so a later `bajan extract` pass re-attempts
it with a fresh retry budget (`ex_park_requeue`,
`specs/extraction-claims.md`). Retries back off 2s·2^(n−1) with jitter,
capped at 60s (bajan-266); burn-in reruns recovered parked episodes with
the cache intact.

## Why does a zero-hit query say `coverage-partial` instead of `not-found`?

A zero-hit read over a partially covered corpus reports
`coverage-partial` with exact declared and persisted counts instead of
asserting absence (`qt_coverage_status`, `specs/query-tools.md`). Ingest
persisting declares corpus coverage (`ic_coverage_declaration`,
`specs/ingestion-contract.md`); a coverage-complete or declaration-free
store answers a bare `not-found`, and a budget-truncated scan never claims
coverage. So `coverage-partial` means "the un-ingested part of your
declared corpus might contain hits" — ingest more of the source, or scope
the query, before concluding the graph lacks the answer. Reads with hits
carry no marker. Declarations ride dump-recreate, so a rebuilt store
reports coverage identically.

## How do evidence checks work, and can they change history?

`bajan evidence check <claim_key>` derives exactly one explicit state for a
claim's provenance: `resolved`, `unresolvable`, or `tombstoned`
(`gm_evidence_resolution`, `specs/graph-model.md`). The first check creates
the claim's evidence reference — supporting episode id, locator scheme,
observed authority tier — as a side record; the frozen claim node schema is
untouched and a claim without one stays fully valid and queryable
(`gm_evidence_ref`). Every check appends one immutable record: history is
never rewritten. A policy-deleted episode checks `tombstoned`, a
post-revive re-ingest checks `resolved`, and prior check records keep the
states they recorded. The check command is the only writer of either
evidence table.
