---
tags: [pipeline-run:tdd-ro5-2026-11-11-bajan-w17-shape-rejection, pipeline-step:quality-ledger]
---

# QUALITY LEDGER bajan-w17 — deterministic candidate-shape rejection

Changed: src/extract.rs (ShapeRule enum, Reason::ShapeFiltered,
ExtractionRun.shape_filtered additive field, non_claim_shape matcher +
heading_glued_lead/is_structure_reference helpers, post-pass wiring
after ex_section_filter with extracted-empty caching for fully filtered
episodes); src/eval.rs (apply_shape_post_pass replay + shape_filtered
count on EvalOutcome); src/cli.rs (eval_envelope replays the post-pass
before scoring); tests/extractor_seam.rs (partial + full-filter seam
tests); tests/eval_harness.rs (post-pass replay test); src/extract.rs
unit tests (matcher incl. gate-refined rules + discourse negatives);
specs/extraction-claims.md (ex_shape_rejection constraint +
refuse_shape transition + p_shape_rejection property);
.espectacular/extraction-claims/p-shape-rejection.toml (2 scenario
tests); evals/results.md (det-0.1.0+shape1 row + delta section).

Red→green note: seam full-filter test observed red in the working tree
(heading fixture misalignment — mid-line `#` vs line-leading anchor),
then green after fixture fix; unit tests co-developed with the pure
matcher (red = API-absence compile state, not separately captured).

Verified: just test 317 ingested green; cargo test --lib 77,
--test extractor_seam 12, --test eval_harness 25; clippy --all-targets
0; fmt clean; spk lint 7 files 0 issues; ah check 0 findings; dont
check grounded; openspec mirror deployed via spec-drift-gate at both
commits. BINDING GATE: bajan eval claims on gold-fi4e@v1 →
precision 0.9157 ≥ 0.9, recall 0.8837 (76/86, ZERO matched-golden
drops), hedge 0.9286 ≥ 0.929 — PASS, row appended per
ev_results_checked_in.

Review: RO5U converged stage 1, GOOD, 0 critical; EDGE-001 fixed via
gate-evidenced deny-set extensions (heading-glued lead + structure
references, per-candidate zero-drop validation before the gate run);
OBS-003 deferred with reason (report-level shape counter — polish, not
spec-required); EDGE-002/CORR-001/CORR-002/CLAR-001 LOW no-fix
(documented).

Commits: bfb742e (tasks 1-2: spec-first apply + impl + tests), 09a8414
(task 3: gate + extensions + results row).
