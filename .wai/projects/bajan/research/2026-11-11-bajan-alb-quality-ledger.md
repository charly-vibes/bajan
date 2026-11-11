# Quality ledger — bajan-alb (add-query-coverage-evidence)

Pipeline: TDD red (compile-red observed on tests/coverage_declarations.rs
against missing API: DECLARED_ID_BOUND, coverage_declarations,
coverage_declared_ids, SearchResult.ingest_coverage) → spec-first apply
415d99f (specs + mirrors + contract TOMLs + red file, per red-tests-share-
spec-commit convention; plain ah check/spk lint/dont green at commit) →
green 910f60c (store tables + declare_coverage/ingest_coverage +
persist_stream declaration + SearchResult marker + CLI text line) → tidy
f4447c3 (pure coverage_marker helper + table-driven shape matrix).

Decisions worth keeping:
- Declaration key = FNV-1a-64 over sorted normalized id set (dependency-
  free, order-independent) → reruns/shuffles converge without new rows;
  full declared count lives on the run row, verbatim ids bounded at
  DECLARED_ID_BOUND = 10_000 (D1), tail counts as unpersisted
  (conservative: never false-complete).
- Declaration written at run acceptance BEFORE outcomes in persist_stream;
  singles = 1-element stream; empty stream declares nothing; failed
  declaration fails the run (store_error per record).
- Marker = presence of SearchResult.ingest_coverage, zero-hit +
  budget-complete only (D4 precedence), separate field from the bajan-913
  extraction-coverage note (two different coverage axes — do not merge).
- Property-test lesson (repeat of prior gotchas): match-pattern/
  budget-shape properties must guard the exhaust shape — budget 1 with a
  single claim COMPLETES (walk 0 < 1); budget 0 truncates on the first
  claim and is the honest exhaust-with-one-claim shape.

Gates at close: ah check 0; ah check --run-tests 203 passed 0 findings;
just test-full green; clippy -D 0; fmt clean; spk lint 0; openspec
validate 11/11 strict; dont check green.

RO5U: GOOD, 0 critical; EDGE-001 (dump/restore carry-through) deferred
with reason → bajan-bcu. Review artifact:
reviews/2026-11-11-ro5u-bajan-alb-converged-stage-1-good-0-critical.md
