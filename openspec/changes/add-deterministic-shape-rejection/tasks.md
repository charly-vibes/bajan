# Tasks — add-deterministic-shape-rejection

Ordered, TDD-first (red → green per item; refactors land as separate tidy
commits per project convention). Each specodelic edit and its deriving
property land in the same commit so `spk lint specs`, `ah check`, and
`dont check` stay green throughout. Do not start until the proposal is
approved.

## 1. Spec deltas (normative specodelic edits)

- [x] 1.1 `specs/extraction-claims.md`: add `ex_shape_rejection` constraint
      + deriving property (fixed-in-code deny set — interrogative-final,
      heading-fragment, navigation/boilerplate; reason `shape-filtered` on
      the run record per `ex_run_record`; prose candidates unchanged;
      fully filtered episode caches extracted-empty per `ex_single_call`)
- [x] 1.2 `spk lint specs`, `ah check`, `dont check` green; contract TOML
      `p-shape-rejection.toml` for the deriving property

## 2. Implementation (red → green)

- [x] 2.1 Shape matcher unit tests: interrogative-final, heading-anchor
      span, navigation/boilerplate, and non-matching prose (red first)
- [x] 2.2 Wire matcher into the deterministic post-pass after
      `ex_section_filter`; rejection reason `shape-filtered` on the run
      record, never the claim node (green)
- [x] 2.3 Fully-filtered-episode caching: extracted-empty cache entry, no
      new call at same extractor version (red → green)

## 3. Eval gate (acceptance per design.md D3)

- [x] 3.1 Re-run `bajan eval claims` on `gold-fi4e@v1`; append results row
      per `ev_results_checked_in`; acceptance: precision ≥ 0.9, recall
      ≥ 0.884 with zero matched-golden drops, hedge ≥ 0.929
- [x] 3.2 Any shape rule that drops a matched golden claim is removed
      before landing (rule set is bought by the gate)

## 4. Tidy

- [x] 4.1 Separate tidy commit: matcher table-driven, clippy + fmt clean,
      `just test` green
