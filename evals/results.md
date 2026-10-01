# Eval results

Per `ev_results_checked_in` (specs/eval-claims.md): checked-in decision
artifact. One row per (corpus, extractor) run. Precision/recall and
hedge-preservation rates are pending the eval harness (specs/eval-claims.md
is specced, not implemented) — raw counts and observed noise rates are the
measured columns until then. **No fabricated numbers.**

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