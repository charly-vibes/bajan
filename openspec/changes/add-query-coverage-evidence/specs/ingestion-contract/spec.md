# ingestion-contract delta — coverage declarations on batch imports

## ADDED Requirements

### Requirement: Batch imports persist an immutable coverage declaration

The system SHALL persist, when a batch import is accepted, a coverage
declaration recording the declared episode stable-id set for that run,
bounded per design decision D1, and SHALL keep the declaration immutable
once the run completes, so that reads can distinguish episodes declared
but not yet persisted from ids never declared.

#### Scenario: Partial run leaves an honest declaration

- **WHEN** a batch import declaring five episode ids persists only three
  before completing
- **THEN** the coverage declaration for that run still records all five
  declared ids, unchanged by the partial persistence

#### Scenario: Re-run converges without rewriting history

- **WHEN** the remaining declared ids are persisted by a later run
- **THEN** the later run writes its own coverage declaration, and the
  original declaration is never mutated
