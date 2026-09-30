# Tasks — add-optional-jev-verification

Ordered, TDD-first (red → green per item; refactors land as separate tidy
commits per project convention). Each specodelic edit and its deriving
property land in the same commit so `spk lint specs`, `ah check`, and
`dont check` stay green throughout. Do not start until the proposal is
approved.

## 1. Spec deltas (normative specodelic edits)

- [ ] 1.1 `specs/extraction-claims.md`: add `ex_jev_optional` constraint
      + deriving property (opt-in, disabled by default, observably
      identical disabled path, verification only tightens the gate,
      record-only calibration mode)
- [ ] 1.2 `specs/extraction-claims.md`: add `ex_jev_verify` constraint
      + deriving property (one batched request per episode; semantic-fields-
      only question set — never re-checks hedge preservation or evidence
      containment, which the deterministic gate verifies exactly;
      threshold validated strictly in (0, 1); zero-candidate episodes
      skip the request; reason `jev-verification-failed`; never repaired)
- [ ] 1.3 `specs/extraction-claims.md`: add `ex_jev_infra` constraint
      + deriving property (transport failures = infrastructure class like
      repeated call failure; uninterpretable answers park on the same
      infrastructure path; never widen the gate)
- [ ] 1.4 `specs/extraction-claims.md`: add `ex_jev_version` constraint
      + deriving property (verifier version = model id + question-set
      version, cached per episode separately from the extraction cache:
      verifier bump re-verifies against cached extraction output with no
      new extraction call; signals on run/rejection records only;
      claim-node field set stays closed v2)
- [ ] 1.5 `specs/extraction-claims.md`: amend `ex_no_llm_post` (scoped to
      its named phases; opt-in verification carved out per
      `ex_jev_verify`) and `ex_run_record` (verifier fields join the
      extraction run row when enabled; not a separate run row) + deriving
      properties updated; extend the `refuse` transition guard to the
      conjunction `[[ex_typed_gate]] [[ex_jev_verify]]` when enabled
- [ ] 1.6 Run gates: `spk lint specs`, `ah check`, `dont check` all green
      after 1.1–1.5

## 2. Configuration surface (red → green)

- [ ] 2.1 Failing test: config parses the verification opt-in (enable
      flag, record-only mode, threshold, model id, question-set version)
      with verification absent = disabled; no default-on anywhere;
      threshold outside (0, 1) fails configuration with a validation
      error before any extraction runs
- [ ] 2.2 Implement config surface in `src/cli.rs`; wire through to the
      extraction seam; green

## 3. Disabled-path regression guard (red → green)

- [ ] 3.1 Failing property test: with verification disabled, run records
      and outcomes for a corpus are indistinguishable from the
      pre-feature pipeline (no TypeSafe client constructed, no
      verification reasons possible)
- [ ] 3.2 Implement the guard path (client never constructed when
      disabled); green

## 4. Verifier module behind a trait (red → green)

- [ ] 4.1 Failing tests: verifier trait contract — given candidates +
      episode state, returns per-field typed results; batched into ONE
      request per episode regardless of candidate count; question set
      contains no hedge-preservation or containment questions (those stay
      deterministic); zero-candidate episodes issue no request
- [ ] 4.2 Implement trait + TypeSafe client adapter (HTTP per
      docs.typesafe.ai/api.md; model id pinned, never "latest"); green
- [ ] 4.3 Failing tests: threshold gate — P(wrong) above configured
      threshold rejects the candidate with reason
      `jev-verification-failed`, recorded per `ex_run_record`; below
      threshold passes through the unchanged deterministic gates;
      record-only mode records results on the run row and rejects
      nothing
- [ ] 4.4 Implement the gate hook between candidate proposal and the
      deterministic gate; green

## 5. Failure classification (red → green)

- [ ] 5.1 Failing tests: Jev transport failure after repeated attempts →
      infrastructure reason + attempt count, cached output untouched, no
      `jev-verification-failed`; uninterpretable answers → parked on the
      same infrastructure path, gate neither widened nor silently passed
- [ ] 5.2 Implement classification reusing the existing
      infrastructure-failure machinery; green

## 6. Version pinning and cache identity (red → green)

- [ ] 6.1 Failing tests: verifier version (model id + question-set
      version) cached per episode separately from `(episode id,
      extractor version)`; verifier bump re-verifies against cached
      extraction output with NO new extraction call; extractor bump
      re-extracts and re-verifies; verification signals resolvable from
      run/rejection records and absent from claim nodes
- [ ] 6.2 Implement; green

## 7. Tidy (separate refactor commits)

- [ ] 7.1 Extract shared batching/request-shape helpers if the verifier
      adapter grew duplication; no behavior change
- [ ] 7.2 Full gates: `cargo test`, `cargo clippy -- -D warnings`,
      `cargo fmt --check`, `spk lint specs`, `ah check`, `dont check`
- [ ] 7.3 Document the config surface (enable flag, record-only mode,
      threshold semantics and bounds, pinned model id, question-set
      version) in README/docs; include the calibration procedure
      (record-only run → review P(wrong) distribution → set threshold)

## Dependencies

- 1.x blocks everything (normative constraints first)
- 2.x blocks 3.x–6.x (config feeds the seam)
- 4.x and 5.x parallelizable after 2.x; 6.x after 4.x
- 7.x last, never mixed with feature commits