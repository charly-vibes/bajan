# extraction-claims delta — supersession, run records, evidence containment

Lands at apply as edits to the specodelic spec `specs/extraction-claims.md`
(new constraints `ex_supersession`, `ex_run_record`, `ex_evidence_containment`;
amendments to `ex_single_call` and `ex_typed_gate` as noted). Recorded here
as ADDED requirements because the normative format is specodelic, not
openspec; this delta is the reviewable contract for those edits.

## ADDED Requirements

### Requirement: Version-bump re-extraction supersedes only unadopted claims

The system SHALL ensure that re-extraction of an episode at a new extractor
version supersedes only that episode's `staged` claims (the same state
`graph.model` names `proposed`) derived from prior extractor versions — setting them to `rejected` with reason
`superseded-by-reextraction` while preserving lineage and evidence — and
SHALL preserve all `active` and `rejected` claims untouched, with new
output that conflicts with an `active` claim staging an invalidation
proposal per the existing conflict mechanism.

#### Scenario: Version bump supersedes staged candidates only

- **WHEN** an episode is re-extracted at a new extractor version and its
  prior-version output produced claims in `staged` state
- **THEN** those claims become `rejected` with reason
  `superseded-by-reextraction`, their lineage edges and evidence spans
  remain queryable, and no claim in `active` or `rejected` state from any
  version changes status

#### Scenario: Active claims survive re-extraction conflicts

- **WHEN** new extraction output for an episode conflicts with an
  `active` claim from a prior extractor version
- **THEN** the active claim is not mutated and an invalidation proposal
  carrying the causing-episode lineage is staged, per the existing
  mutation-proposal mechanism

#### Scenario: Superseded claims are not silently resurrected

- **WHEN** a superseded (`rejected`, `superseded-by-reextraction`) claim
  exists and its successor claim from the new version is discarded
- **THEN** the superseded claim stays rejected; re-entry to `staged`
  requires an explicit re-stage action, never an automated path

#### Scenario: Same-version re-ingest changes nothing

- **WHEN** an episode is re-ingested at the same extractor version after
  its prior claims were discarded or superseded
- **THEN** the cached output is reused, no new extraction call occurs, and
  no discarded claim is resurrected

### Requirement: Every extraction run is recorded with outcome and reasons

The system SHALL record one run row per extraction call — episode id,
extractor version, model id when the extractor uses an LLM, started and
finished timestamps, and finish status — parallel to, and without changing, the
`(episode id, extractor version)` cache economics of the single-call
requirement — and SHALL record a machine-readable reason for every
failure outcome: an episode parked in `rejected` after
repeated call failure, and every candidate rejection at the validation
gate. Reasons SHALL NOT be stored on claim nodes.

#### Scenario: Successful run is traceable

- **WHEN** an episode is extracted successfully
- **THEN** a run row exists with the episode id, extractor version, model
  id (if any), timestamps, and a success finish status, resolvable from
  the extraction output

#### Scenario: Failed calls park with a reason

- **WHEN** an extraction call fails repeatedly and the episode is parked
  in `rejected`
- **THEN** the run record carries the failure reason, and the episode's
  cached output is untouched

#### Scenario: Gate rejections carry reasons without polluting claim nodes

- **WHEN** a candidate claim fails validation (schema, lineage, or evidence
  containment)
- **THEN** the rejection reason is recorded with the run or rejection
  record, the claim node's field set is unchanged, and no failure is
  silently dropped

### Requirement: Candidate claims carry a containment-verified evidence span

The system SHALL require every candidate claim to carry an evidence span —
a verbatim span of the supporting episode text plus its episode locator —
validated by containment after whitespace collapse (all whitespace runs →
single space) of both span and episode text; spans failing containment are
rejected with reason `evidence-not-contained`, never repaired; an
alignment-failure episode may persist span `unknown`, flagged by the
deterministic audit pass.

#### Scenario: Span verified by whitespace-collapsed containment

- **WHEN** a candidate claim's whitespace-collapsed evidence span occurs in
  the whitespace-collapsed episode text
- **THEN** the claim passes the evidence gate and its span is persisted
  verbatim

#### Scenario: Non-contained span is rejected with reason

- **WHEN** a candidate claim's whitespace-collapsed evidence span does not
  occur in the whitespace-collapsed episode text
- **THEN** the claim is rejected with reason `evidence-not-contained`, the
  reason is recorded per the run-record requirement, and the span is never
  auto-corrected

#### Scenario: Unknown span is allowed but flagged

- **WHEN** sentence alignment fails for a legitimate candidate and the
  span is persisted as `unknown`
- **THEN** the claim persists, and the deterministic flag pass (the
  reflection mechanism, extended) reports it as flagged, not as a violation