# Design — add-evidence-references

## Context

Artifact B1 (P0): evidence spans + lineage identify the episode, but
nothing makes evidence *re-resolvable*, or reports resolution failure
honestly. `episode_tombstones` (bajan-aod) introduced a policy-deletion
state the B1 EARS text predates; `authority_tier` (ic_verbatim immutable
source metadata) needs an explicit non-interaction clause. A Rule-of-5
review (2026-11-10) additionally pinned the readonly-discipline conflict
that shaped D1/D2 below.

## Goals / Non-Goals

- Goal: side-record evidence references with honest tri-state resolution,
  revive-survivor semantics, tier provenance, and no ClaimNode migration.
- Non-Goal: no automatic re-resolution daemon, no export-format work
  (consumers read via the inspection surface), no sunset-anchor scheme
  dependency yet (locator scheme version is where it plugs in), no
  source-revision digest in v1 (D5), no as-of timeline over unchecked
  moments (D3 — B5 territory).

## Decisions

### D1 — Side record + append-only check records, written only by the explicit command

`evidence_refs` table: claim_key, episode_id, locator_scheme_version,
observed_tier. `evidence_checks` table: claim_key, resolution_state,
checked_at — append-only, immutable. Rows are created only by the
explicit writing command `bajan evidence check <claim-id>`: the first
check lazily creates the reference, every check appends one record.
Extraction and read commands never write either table — `qt_readonly`
holds without exception; the check command is the sole writer, the
entry point, and the refresh mechanism (run it again to re-check).

### D2 — Tri-state resolution, never fabricated

Each check derives exactly one state from store facts: `resolved`
(episode persisted and the reference's span passes the containment
re-check, reusing the `ex_evidence_containment` whitespace-collapsed
comparison), `unresolvable` (episode unavailable), `tombstoned`
(policy-deleted, revivable). The derived state is recorded on the check
record with its timestamp. Resolution never invents anchor content and
never writes anywhere except its own check record.

### D3 — Revive flips future checks, not history

Reviving a tombstoned episode changes nothing retroactively: past check
records keep the states they recorded; the next check after revive
derives `resolved`. Reproducibility is therefore exact at checked
moments (append-only records); a full as-of timeline over unchecked
moments requires the B5 event log and is explicitly out of scope here.

### D4 — Tier is provenance, not ranking

The reference carries the episode's authority tier as observed at the
most recent check. Because `ic_verbatim` makes source metadata immutable,
the observed tier always equals the episode's tier; the constraint's job
is to forbid resolution-time re-ranking explicitly so no future consumer
mistakes the reference for a scoring input.

### D5 — Digest deferred

`source_revision_digest` is deferred to the external-anchor stage (sunset
S1 anchor contract) where it earns its place: episodes are immutable
(`ic_mutated_resubmit`), so a digest of local content is constant per
episode_id and adds no information today. The reference identifies the
episode by id; external anchor schemes plug in later via
`locator_scheme_version`.

## Risks / Trade-offs

- Check-gated creation means "claim without reference" is the default
  state — consumers must distinguish "not checked" from "resolved".
  Accepted: the tri-state vocabulary already forces explicitness, and
  eager refs on 100% of claims would duplicate evidence every claim
  already carries.
- Re-checks are manual (`bajan evidence check`) — staleness is visible
  per check timestamp but never auto-refreshed. Accepted for v1; an
  automated sweep would be a separate proposal.

## Migration

New tables; no backfill. Existing claims are reference-less until first
checked; nothing about adoption, review, or query paths changes.

## Contract TOMLs

Contracts land at apply stage; pre-apply `ah check --changes
add-evidence-references` no-toml findings are expected (mirrors
add-optional-jev-verification policy).
