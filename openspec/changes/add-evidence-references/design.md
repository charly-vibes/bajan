# Design — add-evidence-references

## Context

Artifact B1 (P0): evidence spans + lineage identify the episode, but
nothing makes evidence *re-resolvable* against an immutable source
revision, or reports resolution failure honestly. `episode_tombstones`
(bajan-aod) introduced a policy-deletion state the B1 EARS text predates;
`authority_tier` (ic_verbatim immutable source metadata) predates too and
needs an explicit non-interaction clause.

## Goals / Non-Goals

- Goal: side-record evidence references with honest tri-state resolution,
  revive-survivor semantics, tier provenance, and no ClaimNode migration.
- Non-Goal: no automatic re-resolution daemon (last-checked is recorded,
  refresh policy is an apply-stage decision), no export-format work
  (consumers read via the inspection surface), no sunset-anchor scheme
  dependency yet (locator scheme version field is where it plugs in).

## Decisions

### D1 — Side record keyed by claim key

`evidence_refs` table: claim_key, episode_id, source_revision_digest,
locator_scheme_version, resolution_state, last_checked_at. One row per
evidence-bearing claim that has been referenced; created lazily on first
reference (not on extraction) — extraction stays byte-identical and
`ex_single_call` economics untouched.

### D2 — Tri-state resolution, never fabricated

`resolved` (episode persisted, digest match, span containment re-check
passes), `unresolvable` (episode or revision unavailable), `tombstoned`
(policy-deleted, revivable). The span containment check reuses the
`ex_evidence_containment` whitespace-collapsed comparison — resolution
never invents anchor content. States are recomputed at resolve time from
store facts; `last_checked_at` records when.

### D3 — Revive survivor semantics

Reviving a tombstoned episode flips subsequent resolutions of references
to that episode from `tombstoned` back to `resolved`; no reference row is
mutated by the revive itself (states are derived, so the historical
`tombstoned` reading at time T stays reproducible — an as-of-friendly
property that also serves B5 later).

### D4 — Tier is provenance, not ranking

The reference stores the episode's tier as observed at reference
creation. Because `ic_verbatim` makes source metadata immutable, the
recorded tier always equals the episode's tier; the constraint's job is
to forbid resolution-time re-ranking explicitly so no future consumer
mistakes the reference for a scoring input.

### D5 — Digest scope

`source_revision_digest` is the digest of the persisted episode text +
locator at reference creation (bajans' own immutable revision), not an
external-file digest; external source-anchor schemes arrive later via
`locator_scheme_version` when the sunset S1 anchor contract lands.

## Risks / Trade-offs

- Lazy creation means "claim without reference" is the default state —
  consumers must distinguish "not referenced" from "resolved". Accepted:
  the tri-state vocabulary already forces explicitness, and eager refs on
  100% of claims would duplicate evidence every claim already carries.
- Digest is episode-scoped, not file-scoped (D5) — weaker than artifact
  B1's ideal for external re-resolution, but honest and dependency-free;
  the sunset S1 plug-in point is reserved.

## Migration

New table; no backfill. Existing claims are reference-less until first
referenced; nothing about adoption, review, or query paths changes.

## Contract TOMLs

Contracts land at apply stage; pre-apply `ah check --changes
add-evidence-references` no-toml findings are expected (mirrors
add-optional-jev-verification policy).
