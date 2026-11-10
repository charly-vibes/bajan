# graph-model delta — resolvable evidence references

## ADDED Requirements

### Requirement: Evidence references are side records created by the explicit check command

The system SHALL persist evidence references as side records keyed by
claim — episode id, locator scheme version, and the supporting episode's
authority tier as observed at the most recent check — created only by the
explicit `bajan evidence check` command, without altering the frozen
ClaimNode schema, and a claim without an evidence reference SHALL remain
fully valid and queryable.

#### Scenario: Reference is created without touching the claim

- **WHEN** `bajan evidence check` runs for an evidence-backed claim that
  has no reference yet
- **THEN** the reference row is created, and the claim node's fields,
  status, and lineage are unchanged

#### Scenario: Reference-less claim behaves as before

- **WHEN** a query or review path touches a claim that has no evidence
  reference
- **THEN** the outcome is identical to the pre-feature behavior, with no
  new states and no new requirements

### Requirement: Resolution yields an explicit tri-state and never fabricates an anchor

The system SHALL derive each evidence check's outcome to exactly one
explicit state — `resolved`, `unresolvable`, or `tombstoned` — by
rechecking persisted store facts including span containment, SHALL NOT
fabricate a resolved anchor, SHALL report a policy-deleted episode's
reference as `tombstoned` with subsequent post-revive checks reporting
`resolved`, and SHALL keep an unresolvable claim inspectable rather than
deleting or hiding it.

#### Scenario: Tombstone window is reported honestly

- **WHEN** an evidence check runs while its episode is tombstoned, and a
  further check runs after the episode is revived
- **THEN** the first check records `tombstoned`, the later check records
  `resolved`, and no prior check record is mutated by the revive

#### Scenario: Missing source is unresolvable, never fabricated

- **WHEN** an evidence check names an episode that is not persisted in
  the store
- **THEN** the check records `unresolvable`, the claim stays inspectable
  with its evidence flagged as unresolvable, and no anchor content is
  invented

### Requirement: Evidence checks are writing commands that persist immutable records

The system SHALL persist an immutable check record — claim key, derived
resolution state, checked-at timestamp — for every run of the evidence
check command, and SHALL NOT write reference, check-record, or claim
state from any read command, keeping `qt_readonly` intact: only the
evidence check command writes, and prior check records are never
rewritten.

#### Scenario: Check records accumulate append-only

- **WHEN** the evidence check command runs twice for the same claim
- **THEN** two immutable check records exist with their own states and
  timestamps, and the later check never modifies the earlier record

#### Scenario: Read commands never write check state

- **WHEN** a read command inspects a claim that has evidence references
- **THEN** no check record, reference row, or claim field is written

### Requirement: Resolution never re-ranks claims

The system SHALL treat the authority tier carried on an evidence
reference as provenance only, and SHALL NOT re-rank, re-weight, or
re-order claims during resolution — ranking remains a query-time concern
per `qt_ranking`.

#### Scenario: Check output carries tier as data

- **WHEN** an evidence check resolves a reference
- **THEN** the outcome reports the observed tier as a fact about the
  supporting episode and changes no claim ordering anywhere in the store
