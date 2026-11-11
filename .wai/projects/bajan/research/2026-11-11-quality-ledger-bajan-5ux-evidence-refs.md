---
tags: [pipeline-run:tdd-ro5-2026-11-11-bajan-5ux-evidence-refs, pipeline-step:quality-ledger]
---

# QUALITY LEDGER bajan-5ux — resolvable evidence references + check command

Changed: specs/graph-model.md (gm_evidence_ref, gm_evidence_resolution,
gm_evidence_check_record, gm_evidence_ref_tier constraints + four
deriving properties; no transitions — side records, claim state machine
unchanged); .espectacular/graph-model/p-evidence-*.toml (4 contracts, 6
test bindings); src/store/sqlite.rs (evidence_refs + evidence_checks
tables; ResolutionState tri-state with stable snake_case codes;
EvidenceRef/EvidenceCheck/EvidenceCheckOutcome records;
derive_resolution pure fn — tombstoned wins first, then episode
availability, then the ex_evidence_containment whitespace-collapsed
containment re-check via the store predicate; evidence_check lazy ref
creation + append-only record; evidence_ref/evidence_checks read paths);
src/cli.rs (bajan evidence check <claim-key> — sole writer, envelope
with state/created/episode_id/tier/checked_at, ClaimNotFound →
spec-traced evidence_error); tests/evidence_refs.rs (7 tests: ref
creation + claim byte-identical, 7-case derivation table, tombstone
window via dump-rebuild shape, append-only accumulation, read-paths-
write-nothing sweep, tier provenance, full lifecycle through the real
extraction seam); README quickstart 5b.

Red→green notes (all caught by tests, not review): (1) containment
fixture span was not a true substring — derivation honestly Unresolvable,
fixture fixed; (2) delete_episode refuses under a supporting claim
(gm_lineage_survives) — task 3.1's literal sequence unreachable by
design; tombstone legs re-shaped to the dump-rebuild path and the
refusal pinned as an assertion; (3) revive alone leaves the episode row
gone — post-revive RE-INGEST required before a resolved check
(ic_deleted_recreate); (4) proposals_staged counts mutation proposals,
not staged claims — lifecycle asserts claims_with_lineage instead.

Verified: evidence_refs 7 green; cli evidence envelopes 2 green; just
test 326 ingested green; just test-full green; clippy --all-targets 0;
fmt clean; spk lint 0 issues; ah check 0; ah check --run-tests 193
passed 0 findings; dont check grounded; spec-drift-gate green.

Review: RO5U converged stage 1, GOOD, 0 critical; EDGE-001 deferred
with reason (multi-source refs record first lineage edge — D1-pinned
singular schema); INTERPLAY-003 discovered red and pinned; rest LOW
no-fix documented.

Commits: evidence impl (tasks 1-3, one commit), tidy README (task 4.1).
