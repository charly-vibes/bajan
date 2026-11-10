# Change: Add Deterministic Candidate-Shape Rejection to the Extraction Post-Pass

## Why

The first scored eval run (evals/results.md, 2026-11-04, `bajan eval claims`
per bajan-2k4) measured the deterministic extractor at precision 0.752 /
recall 0.884 / hedge preservation 0.929 on `gold-fi4e@v1` — and named the
cause: all 25 spurious candidates are "questions/headings/navigation lines
adjudication crossed out … sentence segmentation alone is the precision
bottleneck". These non-claims slip past `ex_section_filter` because they are
content-shape problems (a question or a bare heading sitting in genuine
prose episodes), not section-tag problems.

A deterministic post-pass rejection of non-claim shapes attacks exactly
these 25 candidates: zero API cost, `ex_no_llm_post`-compliant, and it
improves the base that `add-optional-jev-verification` (record-only
calibration) would later measure. Grounding: pinky ledger
`ledger/topics/bajan-structure.org` (Ro5-corrected evaluation order,
2026-11-10) and the Bajan/Sunset capability-gaps artifact B3 follow-up.

## What Changes

Spec-level (apply stage lands as specodelic constraint edits to
`specs/extraction-claims.md`; this change records the deltas and design
decisions):

- **extraction-claims** — one new invariant, no amendments to existing
  constraints:
  - `ex_shape_rejection`: the deterministic post-pass additionally rejects
    candidate claims whose text matches a fixed-in-code non-claim shape set
    (interrogative-final form, heading/section-title fragments, navigation
    and list boilerplate) with machine-readable reason `shape-filtered` on
    the run record (never on a claim node, per `ex_run_record`); the shape
    set is fixed in code, never an LLM judgment (`ex_no_llm_post` holds);
    non-matching prose candidates stage unchanged, and a fully
    shape-filtered episode still caches as a legitimately extracted empty
    result per `ex_single_call` cache economics.

This mirrors the `ex_section_filter` pattern (tag-based deny set, reason on
run record) at the content-shape level; the two deny sets are orthogonal.

## Impact

- **specs/extraction-claims/spec.md** — one ADDED requirement set (two
  one-behavior requirements), no modifications to existing requirements.
- **Code** — shape matcher + post-pass wiring in the deterministic
  extraction path; no schema change, no extractor-prompt change, no cache
  invalidation (post-pass runs after the cached call output; a shape-deny
  change never triggers re-extraction).
- **evals** — acceptance requires a fresh `bajan eval claims` row on
  `gold-fi4e@v1` (see design.md gates).
