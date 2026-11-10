# Design — add-deterministic-shape-rejection

## Context

evals/results.md (2026-11-04, det-0.1.0 @ gold-fi4e@v1): precision 0.752,
recall 0.884, hedge 0.929; 25/25 spurious candidates are interrogative,
heading, or navigation shapes in prose episodes. Section-kind tags
(`ex_section_filter`) cannot catch them — the episodes are genuine prose;
the candidates themselves are malformed as claims.

## Goals / Non-Goals

- Goal: reject non-claim shapes deterministically, with a machine-readable
  run-record reason, without touching recall or hedge fidelity.
- Non-Goal: no LLM judging shapes (`ex_no_llm_post`), no extractor-prompt
  change (`ex_single_call` cache economics preserved), no re-extraction of
  already-cached episodes, no ClaimNode/schema change.

## Decisions

### D1 — Shape set is conservative and fixed in code

Deny set (v1): (a) interrogative-final text (ends with `?` after
whitespace collapse), (b) heading/section-title fragments — candidates
whose aligned source span sits inside a converter-supplied heading anchor
(`ex_deterministic_structure` markers), (c) navigation/list boilerplate
(lines that are pure list markers, breadcrumbs, or menu fragments).
Rationale: every rule must be explainable as "this text is not an
assertion"; anything subtler belongs to the reflection pass
(`ex_reflection`) or human review, not the gate.

### D2 — Reason placement follows ex_run_record

`shape-filtered` lands on the run record (never the claim node) — same
discipline as `section-filtered`.

### D3 — Acceptance gate is the eval harness, not vibes

Re-run `bajan eval claims` on `gold-fi4e@v1` at the new extractor
post-pass version. Acceptance: precision ≥ 0.9, recall ≥ 0.884 (no
matched-golden drops), hedge preservation ≥ 0.929, results row appended to
evals/results.md per `ev_results_checked_in`. If any shape rule drops a
matched golden claim, that rule is removed from the deny set before
landing — the gate buys the deny set, not the reverse.

### D4 — Versioning and cache ordering

The extraction cache stores post-pass output (pinned by
`ex_section_filter`'s extracted-empty caching), so a shape-deny-set
change is forward-only: cached and already-staged episodes are never
re-filtered. The post-pass version bumps with the shape-set change so
eval rows are comparable per `ev_deterministic_scoring` (same corpus,
new extractor version = new row, never an overwrite).

## Risks / Trade-offs

- Over-rejection (recall loss) → mitigated by D3's golden-claim gate.
- Corpora where questions ARE claims (FAQ-style sources) → the deny set is
  global in v1; if a real workload needs question-claims, that is a future
  scope proposal (possibly tag-gated), not a v1 relaxation.

## Migration

None — no persisted state changes. Previously staged shape-shaped
candidates stay as they are (adopt/reject remains human-only per
`gm_human_adopt`); the gate binds new extractions.

## Contract TOMLs

Property contracts for the new constraints land at apply stage;
`ah check --changes add-deterministic-shape-rejection` reporting no-toml
findings before apply is expected and pre-existing policy (mirrors
add-optional-jev-verification).
