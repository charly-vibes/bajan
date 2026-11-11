# Changelog

All notable changes to bajan. The format is loose — this project versions by
milestone, not by cadence. Every entry links the spec or ticket that governs it.

## v0.3.0 — 2026-11-11

Three RO5U-reviewed features: extraction precision, honest zero-hit reads,
resolvable provenance — plus a strict openspec gate and two contract
executors for clauses no test had pinned.

### Deterministic shape rejection (bajan-w17, `ex_shape_rejection`)

- A fixed-in-code post-pass rejects every candidate matching a non-claim
  shape before the validation gate: interrogative-final text after
  whitespace collapse, heading/section-title fragments aligned inside
  converter-supplied heading anchors, navigation and list boilerplate.
- Filtered candidates never reach `staged`; run rows carry the
  machine-readable `shape-filtered` reason with the matched rule — never
  a claim node (`ex_run_record`).
- An episode whose every candidate is shape-rejected caches as
  legitimately extracted-empty — a rerun starts no new LLM call
  (`ex_single_call`); the cache stores post-pass output, so a changed
  deny set binds only extractions at a bumped version (forward-only).
- The eval harness replays the shape post-pass before scoring; the
  heading-glued + structure-ref deny-set extensions score PASS
  0.916/0.884/0.929 with zero matched drops on the burn-in corpus.

### Evidence references + check command (bajan-5ux, `gm_evidence_ref` / `gm_evidence_resolution` / `gm_evidence_check_record` / `gm_evidence_ref_tier`)

- `bajan evidence check <claim_key>` derives exactly one explicit state
  per run — `resolved`, `unresolvable` (episode absent or span fails the
  containment re-check), or `tombstoned` (policy-deleted) — never a
  fabricated anchor; an unresolvable claim stays inspectable, never
  hidden.
- The first check creates the claim's evidence reference — supporting
  episode id, locator scheme version, and the episode's authority tier
  as observed (provenance only, never a ranking input). The frozen
  ClaimNode schema is untouched, and reference-less claims behave
  exactly as before the feature.
- Every check appends one immutable check record — append-only, never
  rewritten; the check command is the only writer of either evidence
  table (read commands write nothing, `qt_readonly` intact).
- A check inside the tombstone window records `tombstoned`; a
  post-revive check records `resolved`; prior records keep their states.

### Query coverage evidence (bajan-alb + bajan-bcu, `ic_coverage_declaration` / `qt_coverage_status`)

- Import persisting declares corpus coverage; a partial run leaves an
  honest declaration and a rerun converges without rewriting history.
- Zero-hit reads over a partially covered corpus report `coverage-partial`
  with exact declared/persisted counts instead of asserting absence;
  coverage-complete and declaration-free stores answer bare `not-found`;
  budget exhaustion outranks coverage; reads with hits carry no marker.
- Coverage declarations ride dump/recreate raw (`DumpedCoverageDeclaration`
  with `serde(default)` — old dumps stay loadable; restore INSERT OR
  IGNORE converges), so a rebuilt store reports coverage identically —
  declared-but-unpersisted ids included, since the extraction dump is
  their only carrier.

### Strict openspec gate (bajan-0ts)

- The corpus passes `openspec validate --all --strict --no-interactive`
  under openspec 1.x (8/8); CI's openspec gate now runs strict.

### Contract executors (bajan-o4t)

- Two corpus clauses had no executing test: shape deny-set forward-only
  semantics and reference-less claim behavior — both pinned now
  (`deny_set_change_never_refilters_cached_or_staged_episodes`,
  `reference_less_claim_behaves_as_before`) and folded into the
  `p-shape-rejection` / `p-evidence-ref` contracts.

## v0.2.0 — 2026-11-04

The specs-are-the-code release: the corpus spec mirror is drift-gated, every
deployed property has an executing contract test, and the pipeline gains its
first operator-maintenance surface.

### Specs unification (bajan-16o)

- Corpus specs deploy into `openspec/specs/<name>/spec.md` via
  `tools/deploy_specs.py`; `tools/spec_drift_gate.sh` (lefthook pre-commit)
  blocks any commit whose staged mirror is stale.
- 68 scenario-scoped property contracts in `.espectacular/<spec>/p-*.toml`,
  all executing: `ah check --run-tests` 185 passed.
- Contract test filters use real cargo test paths — binary-name prefixes
  never match cargo's substring filter.

### Episode tombstone + revive (bajan-aod, `ic_deleted_tombstone` / `ic_deleted_recreate`)

- `bajan delete <episode_id>` retires a claim-free episode; refused while any
  claim supports it (`gm_lineage_survives`).
- `bajan revive <episode_id>` is the only path that clears a tombstone.
- A tombstoned id rejects every submission with the machine-readable
  `deleted_id` reason — re-ingest never resurrects; mutation cannot bypass.
- Tombstones ride the dump/recreate rebuild pair raw; storage schema gains
  the `episode_tombstones` table (old databases open fine — additive).

### LLM extractor hardening

- `BAJAN_MAX_SPEND_USD` spend cap — required for the LLM extractor,
  enforced pre-call, fail-closed on unaccountable cost (bajan-8cc).
- Per-episode failure telemetry: parked episodes named with `FailureClass`
  (timeout / http-N / transport / schema) and attempt counts (bajan-gso).
- Jittered retry backoff 2s·2^(n−1), 60s cap (bajan-266).
- Per-call `usage.cost` stderr log with running spend (bajan-3w4).
- `BAJAN_EXTRACTOR_TIMEOUT_SECS` configurable transport timeout (bajan-noj).

### Eval harness (bajan-2k4)

- Deterministic precision/recall + hedge-fidelity scoring
  (`bajan eval claims`), human-adjudicated gold corpus `gold-fi4e-v1`, first
  scored run, run-manifest export with corpus/run cross-check (bajan-t6z).

### Query & entity resolution

- Coverage gaps include orphaned re-extractions; contradict carries coverage
  (bajan-uah); parked/pending-episode surfacing note (bajan-913).
- Curated alias tables for deterministic ER (bajan-amp).
- Strict `bajan.toml` workspace settings: staleness window, default budget,
  LLM retry budget — flat `key = <integer>` only (bajan-bku).

## v0.1.0 — first release

Spec-complete deterministic pipeline: ingest → extract → SQLite → query →
adopt, bounded traversals, lineage-traceable provenance, HITL entity review
and contradiction proposal queues.
