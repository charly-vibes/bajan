# Tasks — add-query-coverage-evidence

Ordered, TDD-first (red → green per item; refactors land as separate tidy
commits). Each specodelic edit and its deriving property land in the same
commit so `spk lint specs`, `ah check`, and `dont check` stay green. Do
not start until the proposal is approved.

## 1. Spec deltas (normative specodelic edits)

- [x] 1.1 `specs/ingestion-contract.md`: add `ic_coverage_declaration`
      constraint + deriving property (declared stable-id set per accepted
      batch import, bounded per design D1, immutable on run completion)
- [x] 1.2 `specs/query-tools.md`: add `qt_coverage_status` constraint +
      deriving property (coverage-complete / coverage-partial with
      declared+persisted counts; zero-hit under partial carries the
      marker, never bare `not-found`; budget-exhausted outranks coverage)
- [x] 1.3 `spk lint specs`, `ah check`, `dont check` green; contract
      TOMLs for both deriving properties

## 2. Implementation (red → green)

- [x] 2.1 Coverage-declaration side table + write path on accepted batch
      imports; immutability after run completion (red first)
- [x] 2.2 Bound behavior per D1: id-set form ≤ bound, count+digest form
      beyond (red → green)
- [x] 2.3 Coverage computation + zero-hit marker on the read path, reason
      precedence budget-exhausted > coverage-partial > not-found
      (red → green)
- [x] 2.4 No-declaration stores report `coverage-complete` (migration D
      note: honest default, no backfill)

## 3. Verification

- [x] 3.1 `just test` green; partial-corpus end-to-end test: declare 5,
      persist 3, zero-hit query → `coverage-partial` (5,3); persist last
      2 → same query → `not-found`
- [ ] 3.2 clippy + fmt clean; `just test-full` green

## 4. Tidy

- [x] 4.1 Separate tidy commit: coverage computation extracted to a pure
      helper, table-driven tests
