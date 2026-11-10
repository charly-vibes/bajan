# graph-model delta — resolvable evidence references

## ADDED Requirements

### Requirement: Evidence references are side records that leave the claim node frozen

The system SHALL persist evidence references as side records keyed by
claim — episode id, source revision digest, locator scheme version,
resolution state, last-checked timestamp, and the supporting episode's
authority tier as observed at creation — without altering the frozen
ClaimNode schema, and a claim without an evidence reference SHALL remain
fully valid and queryable.

#### Scenario: Reference is created without touching the claim

- **WHEN** an evidence reference is created for an evidence-backed claim
- **THEN** the claim node's fields, status, and lineage are unchanged,
  and the reference row carries episode id, digest, scheme version,
  tier, and last-checked timestamp

#### Scenario: Reference-less claim behaves as before

- **WHEN** a query or review path touches a claim that has no evidence
  reference
- **THEN** the outcome is identical to the pre-feature behavior, with no
  new states and no new requirements

### Requirement: Resolution yields an explicit tri-state and never fabricates an anchor

The system SHALL resolve an evidence reference to exactly one explicit
state — `resolved`, `unresolvable`, or `tombstoned` — by rechecking
persisted store facts including span containment, SHALL NOT fabricate a
resolved anchor from any other content, SHALL report a policy-deleted
episode's reference as `tombstoned` and its subsequent resolution as
`resolved` after the episode is revived, and SHALL keep an unresolvable
claim inspectable rather than deleting or hiding it.

#### Scenario: Tombstone window is reported honestly

- **WHEN** an evidence reference is resolved while its episode is
  tombstoned, and resolved again after the episode is revived
- **THEN** the first resolution reports `tombstoned`, the second reports
  `resolved`, and no reference row was mutated by the revive itself

#### Scenario: Missing source is unresolvable, never fabricated

- **WHEN** an evidence reference names an episode that is not persisted
  in the store
- **THEN** resolution reports `unresolvable`, the claim stays
  inspectable with its evidence flagged as unresolvable, and no anchor
  content is invented

### Requirement: Resolution never re-ranks claims

The system SHALL treat the authority tier recorded on an evidence
reference as provenance only, and SHALL NOT re-rank, re-weight, or
re-order claims during resolution — ranking remains a query-time concern
per `qt_ranking`.

#### Scenario: Resolution output carries tier as data

- **WHEN** an evidence reference is resolved
- **THEN** the resolution reports the recorded tier as a fact about the
  supporting episode and changes no claim ordering anywhere in the store
