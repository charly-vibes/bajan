# Design — add-query-coverage-evidence

## Context

`qt_bounded_traversal` already guarantees honest *budget* reporting
(`complete` / `budget-exhausted`). Artifact B6 asks for the *coverage*
half: zero-result reads must expose corpus completeness. Today nothing
records what an import run declared, so "no matches" cannot be qualified.

## Goals / Non-Goals

- Goal: a persisted per-run declared-id set + a coverage status on
  zero-hit reads. Minimal, honest, no query-language change.
- Non-Goal: no completeness guarantee for un-declared (API/append) ingest
  paths — coverage is only defined where a declaration exists; no
  progress tracking UI; no automatic re-ingest.

## Decisions

### D1 — Declarations attach to batch imports, stored as a bounded id set

A coverage declaration stores the declared stable-id set verbatim up to a
fixed bound (10,000 ids) and count-plus-digest beyond it; resolution of
individual presence stays an `ic_unique_id` lookup against persisted
episodes. Rationale: honest fractions need the actual set; the bound
keeps pathological imports from bloating the store, and the digest keeps
the declaration auditable at any size.

### D2 — Immutability on run completion

A declaration is written when the batch import is accepted and is
immutable once the run completes — re-running converges per
`ic_batch_resume`, and a new run writes its own declaration (matching
extraction's run-row discipline: history is never rewritten).

### D3 — Coverage is a zero-hit marker, not a field on every hit

Only reads that return zero hits carry `coverage-partial` + counts. Hit
results are unchanged — byte-identical output on complete corpora, and no
schema churn on the query result record (`qt_query-schema` publishes the
budget vocabulary; coverage vocabulary is published by the new
requirement's own text, keeping that extension point untouched).

### D4 — `not-found` vs `coverage-partial` are mutually exclusive labels

`not-found` remains the honest answer for a completed budget over a
coverage-complete corpus. A zero-hit under partial coverage reports
`coverage-partial` (with `declared` and `persisted` counts) instead —
the marker says "the question may be answerable later", not "it does not
exist". Budget exhaustion still reports `budget-exhausted` first: budget
trumps coverage in the reason precedence, because a truncated scan says
nothing definitive about either world or corpus.

## Risks / Trade-offs

- Imports without declarations (programmatic single-episode ingest) have
  no coverage evidence — accepted: single submits declare themselves
  trivially (one id), so the gap is only for out-of-band writers, which
  the CLI does not expose.
- Store growth from declared-id sets — bounded by D1; digest form beyond
  the bound keeps it O(1) per over-bound run.

## Migration

New side table; no backfill — existing stores simply report
`coverage-complete` for zero-hit reads until the next declared import
lands (honest default: absence of declarations means no known incompleteness).

## Contract TOMLs

Contracts land at apply stage; pre-apply `ah check --changes
add-query-coverage-evidence` no-toml findings are expected (mirrors
add-optional-jev-verification policy).
