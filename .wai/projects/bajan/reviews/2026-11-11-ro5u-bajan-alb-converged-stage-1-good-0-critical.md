---
tags: [pipeline-run:tdd-ro5-2026-11-11-bajan-alb-coverage-evidence, pipeline-step:ro5u-review]
---

# RO5U bajan-alb (diff 415d99f..f4447c3, src/ingest.rs + src/query.rs + src/store/sqlite.rs + src/cli.rs + tests/coverage_declarations.rs)

Converged stage 1, verdict **GOOD**, 0 critical. Review scope: declared-id
coverage declarations (`ic_coverage_declaration`) + zero-hit ingest-coverage
marker (`qt_coverage_status`), specs/ingestion-contract.md +
specs/query-tools.md deltas 415d99f, green 910f60c, tidy f4447c3.

## Findings

- **EDGE-001 MEDIUM deferred with reason → bajan-bcu** — dump/restore does
  not carry `coverage_declarations`/`coverage_declared_ids`: a dump-rebuild
  drops every declaration, so a rebuilt partially-declared store answers
  bare `not-found` on zero-hits (the exact dishonesty this change removes)
  until the next declared import lands. Deferred: alb's tasks.md scopes
  store+read paths only, and `ExtractionDump` is a versioned serde surface
  (schema conformance consumers); the carry-through is a focused follow-up
  with a preservation test, filed as bajan-bcu.
- **CORR-001 LOW no-fix (documented)** — the declaration key is FNV-1a-64
  over the sorted declared set: not collision-resistant. A collision would
  silently merge two distinct declarations (INSERT OR IGNORE). The key's
  role is convergence + audit ("an operator with the stream can recompute
  the digest"), not adversarial identity; a cryptographic digest is a
  drop-in swap if that ever becomes a threat model.
- **EDGE-002 LOW no-fix (honest by design)** — a run of all-malformed
  records declares ids that never persist → the corpus reads
  coverage-partial forever until corrected resubmission (which writes its
  own declaration and converges, pinned by the E2E). This is D5's intended
  semantics: declared-but-never-persisted is genuinely known
  incompleteness.
- **CLAR-001 LOW no-fix (documented, conservative direction)** — beyond the
  D1 bound, per-id presence is unverifiable (count+digest only), so the
  tail always counts as unpersisted: an over-bound fully-persisted corpus
  reports partial (n, bound) indefinitely. Never false-complete; never a
  false `not-found`. D1's trade-off section accepts the fidelity loss for
  pathological imports; a set-equality digest for the tail would be a new
  proposal, not a tweak.
- **OBS-002 LOW no-fix** — a failed declaration write fails the whole run
  as per-record `store_error` outcomes (identical message × N). The
  outcome schema already carries store errors; per-record duplication is
  cosmetic. The strict "every accepted run declares" gate is the point.
- **CORR-002 verified, no-fix needed** — immutability under concurrency:
  there is no UPDATE path on either coverage table; identical concurrent
  runs converge on the digest key (INSERT OR IGNORE), distinct runs write
  distinct rows. p_coverage_declaration pins rerun/shuffle convergence.

## Verification at close

ah check 0 findings; ah check --run-tests 203 passed 0 findings (10 new:
8 unit/scenario incl. the acceptance E2E declare5/persist3 → (5,3) →
not-found, 2 proptests); query lib shape matrix; just test-full green;
clippy -D warnings 0; fmt clean; spk lint 7 files 0; openspec validate
11/11 strict; dont check green; spec-drift-gate green at every commit.
