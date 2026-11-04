# Changelog

All notable changes to bajan. The format is loose — this project versions by
milestone, not by cadence. Every entry links the spec or ticket that governs it.

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
