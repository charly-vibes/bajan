# graph-model Specification

## Purpose
TBD - created by archiving change update-extraction-provenance. Update Purpose after archive.
## Requirements
### Requirement: No claim reaches active without an explicit human accept action

The system SHALL require an explicit human accept action for a claim to
move from `proposed` to `active` (in addition to the reification guard);
batch accepts of multiple claims in one session are permitted, and every
accepted claim's transition is individually recorded with actor and
timestamp; no automated path may set `active`.

#### Scenario: Automated promotion is impossible

- **WHEN** any pipeline stage, re-ingest, or merge runs
- **THEN** no claim's status changes to `active` without a recorded human
  accept action; `proposed` claims remain `proposed`

#### Scenario: Batch accept is per-claim audited

- **WHEN** a reviewer accepts multiple `proposed` claims in one session
  with a single confirmation
- **THEN** each claim's transition to `active` is individually recorded
  with the session's actor and timestamp, preserving per-claim reversibility

### Requirement: Claim nodes carry a verbatim evidence span (schema v2)

The system SHALL extend the claim-node schema from seven frozen fields
(text, valid_at, invalid_at, data_cutoff, status, scope, source_type) to
eight by adding `evidence` — a verbatim span of the supporting episode text
with its episode locator, or the literal `unknown` when sentence alignment
failed — and SHALL preserve hedge markers present in the supporting
episode text within the evidence span.

#### Scenario: Claim nodes expose exactly the v2 field set

- **WHEN** any claim node is inspected
- **THEN** it carries exactly the seven v1 fields plus `evidence` (verbatim
  span + locator, or `unknown`), with types preserved and no additional
  fields such as confidence or vector blobs

#### Scenario: Evidence span does not upgrade hedged statements

- **WHEN** the supporting episode text contains a hedge marker within the
  sentence the evidence span covers
- **THEN** the evidence span preserves the hedge marker verbatim

#### Scenario: Existing graphs migrate with unknown spans, loudly

- **WHEN** a pre-v2 graph is migrated to the v2 schema
- **THEN** every existing claim node's `evidence` is backfilled as
  `unknown`, the migration output reports the count of backfilled nodes,
  and no claim node is dropped or rewritten in the migration

