# Eval results

Per `ev_results_checked_in` (specs/eval-claims.md): checked-in decision
artifact. One row per (corpus, extractor) run. The eval harness is
implemented (bajan-2k4: `bajan eval claims`). Gold labels are human
sign-off and land in a reviewed commit before any metrics run; the first
adjudicated corpus is `gold-fi4e-v1` (2026-11-04). Raw counts and
observed noise rates are the measured columns.
**No fabricated numbers.**

The pre-harness table below records burn-in observations. Scored runs
append to the eval-claims table (columns match `data.results_row` from
`bajan eval claims`; fill the date cell):

| date | corpus | extractor | precision | recall | hedge preservation | raw counts |
|------|--------|-----------|-----------|--------|--------------------|------------|
| 2026-11-04 | gold-fi4e@v1 | det-0.1.0 | 0.752 | 0.884 | 0.929 | emitted=101 matched=76 golden=86 content-miss=0 spurious=25 hedge-dropped=0 |
| 2026-11-11 | gold-fi4e@v1 | det-0.1.0+shape1 | 0.916 | 0.884 | 0.929 | emitted=83 matched=76 golden=86 content-miss=0 spurious=7 hedge-dropped=0 |

## Shape-rejection delta (bajan-w17, det-0.1.0+shape1)

- Same recorded run manifest (`evals/fixtures/gold-fi4e-v1-run.json`,
  extractor det-0.1.0) replayed through the deterministic shape post-pass
  (`ex_shape_rejection`): 18 of 101 candidates shape-filtered
  (8 interrogative/heading/nav + 10 heading-glued leads and
  document-structure references the gate demanded), matched=76 and
  hedge-dropped=0 unchanged — zero matched-golden drops, so the deny set
  is bought by the gate (D3); no rule needed removal.
- 7 spurious survive as discourse glue ("But now is the time.") —
  conservatively out of the deny set per D1; subtler judgment belongs to
  the reflection pass (`ex_reflection`) or human review, not this gate.
- Rule extensions validated against matched candidates before landing:
  heading-glued lead (ALL-CAPS title run or two-word title-case phrase
  opening into a sentence at a clean boundary) and structure references
  ("part 2", "parts 2, 3, and 4", "following chapter") — zero matched
  drops, verified per-candidate before the gate run.

First scored run (bajan-2k4 MUST gate): the deterministic extractor's 25
spurious candidates are exactly the questions/headings/navigation lines
adjudication crossed out — on real prose, sentence segmentation alone is
the precision bottleneck (all 76 kept proposals matched; the 10 golden
unmatched are authored/recovered claims the extractor missed, e.g.
heading-contaminated definitions). Hedge preservation 13/14: the one
miss is an unmatched golden claim, not a dropped marker.

| date | corpus | extractor | episodes | extracted | claims staged | junk claims (praise/testimonial/copyright/CTA) | notes |
|------|--------|-----------|----------|-----------|---------------|------------------------------------------------|-------|
| 2026-10-01 | Profit First (2017 EPUB, 50 episodes, v1 stream, untagged) | llm-deepseek/deepseek-v4-flash-0.1.0 (pre-filter) | 50 | 40 (2 passes) | 344 | 41 (11.9%) | burn-in run, docs/burnin-deepseek-v4-flash-profit-first.org; 10 parked stochastically |
| 2026-10-01 | Profit First (2017 EPUB, 50 episodes, v2 stream, section-kind tags) | llm-deepseek/deepseek-v4-flash-0.1.0 (post-filter) | 50 | 42 (1 pass) | 246 | 4 (1.6%) | bajan-9uy fix measured; all 4 residual are ProfitCON CTA mentions inside genuine chapter prose — content-level, not section-level, out of reach of section tags (curation surface's job) |

## Section-filter delta (bajan-9uy → bajan-0yg/ie0/pbd/162)

- Converter tags: exactly 4 episodes tagged (`section-kind:praise` ×2,
  `section-kind:copyright`, `section-kind:acknowledgments`).
- The 4 tagged episodes extracted-and-cached empty (ex_section_filter);
  38 untagged episodes extracted normally.
- Junk claims: **41 → 4 (−90%)**; claim volume 344 → 246 across fewer
  knowledge episodes (the removed volume was noise, per the sample).
- Unknown-evidence spans: 74 (v1) / 68 (v2) — ~20% either way; deepseek-v4-flash
  omitting verbatim spans is provider behavior, honestly flagged by
  reflection (`ex_evidence_containment` typed-absent path), unchanged by
  the filter.
- Stochastic parking persists at book scale (8/50 pending in one pass;
  v1 recovered 2 on rerun) — tracked in bajan-266, not a filter effect.

## Storage-stack decision status

Insufficient: precision/recall do not exist yet (eval harness unimplemented).
SQLite carries the load fine so far (50-episode corpus, sub-second reads);
the decision waits for the harness and a multi-book corpus.