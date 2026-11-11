---
tags: [pipeline-run:tdd-ro5-2026-11-11-bajan-w17-shape-rejection, pipeline-step:ro5u-review]
---

# RO5U bajan-w17 (diff 68685e1..09a8414, src/extract.rs + src/eval.rs + src/cli.rs + tests)

Converged stage 1, verdict **GOOD**, 0 critical.

## Findings

- **CORR-001 LOW no-fix (documented)** — `shape_filtered` uses
  `skip_serializing_if = "Vec::is_empty"` + `serde(default)` under
  `deny_unknown_fields`: empty-vec rows serialize without the field; old
  rows deserialize to empty via default. Both reader generations safe;
  round-trip pinned by the run-record proptests (77 lib green).
- **EDGE-001 MEDIUM fixed-in-sim, verified** — initial deny set caught
  only 8/25 spurious (gate FAILED 0.817 < 0.9 on first gate run). Fixed
  by two gate-evidenced extensions: heading-glued lead (ALL-CAPS run or
  two-word title-case phrase opening into a sentence at a clean
  boundary) and document-structure references ("part 2", "following
  chapter"). Both validated per-candidate against the matched set
  BEFORE the gate run: zero matched-golden drops; final gate
  0.916/0.884/0.929 PASS.
- **EDGE-002 LOW no-fix (v1 scope)** — hypothetical all-caps shouting
  prose ("DO NOT BUY THIS NOW Limited offer") would misfire as a
  heading-glued lead. Deny set is fixed-in-code v1, corpus-arbitrated,
  forward-only (cache stores post-pass output); a real-workload miss is
  a future scope proposal, not a relaxation (design.md anti-goal).
- **CLAR-001 LOW no-fix** — title-run stoplist divergence between the
  python pre-port simulation ("am" absent) and the rust port ("am"
  present): no outcome difference on gold-fi4e@v1 (rust 18 filtered =
  sim 8+10). Documented so future sim↔rust parity checks know the
  stoplists are not byte-identical.
- **OBS-003 MEDIUM deferred with reason** — `ExtractReport` carries no
  aggregate `shape_filtered` counter; partial-filtering telemetry lives
  only on run rows. Spec requires run-record placement only
  (`ex_run_record`/D2); a report counter is additive polish, deferred to
  a follow-up ticket if operators ask for it in `bajan extract` output.
- **CORR-002 LOW no-fix** — eval post-pass replay keys heading alignment
  off corpus episode text; unknown-episode candidates get empty text (no
  alignment) but still judged on their own shape. Conservative
  direction; wrong-corpus guard (`eval_error`) unchanged and upstream.

## Gates at review time

just test 317 ingested green; cargo test --lib 77 + extractor_seam 12 +
eval_harness 25 green; clippy --all-targets 0; fmt clean; real gate:
precision 0.9157 ≥ 0.9, recall 0.8837 (76/86, zero matched-golden
drops), hedge 0.9286 ≥ 0.929 on gold-fi4e@v1 (det-0.1.0+shape1).

## Risks

- Deny-set rules are English-corpus-shaped (title-case, all-caps,
  "part N"); non-English corpora will under-filter — conservative
  direction, honest.
- Discourse glue ("But now is the time.") intentionally survives; DoD
  for any future extension stays D3 (zero matched-golden drops).
