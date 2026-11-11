# ingestion-contract delta — coverage declarations on import runs

## ADDED Requirements

### Requirement: Import runs persist an immutable coverage declaration

The system SHALL persist, when an import run is accepted, a coverage
declaration recording the declared episode stable-id set for that run,
bounded per design decision D1, and SHALL keep the declaration immutable
once the run completes, so that reads can distinguish episodes declared
but not yet persisted from ids never declared.

#### Scenario: Partial run leaves an honest declaration

- **WHEN** an import run declaring five episode ids persists only three
  before completing
- **THEN** the coverage declaration for that run still records all five
  declared ids, unchanged by the partial persistence

#### Scenario: Re-run converges without rewriting history

- **WHEN** the remaining declared ids are persisted by a later run
- **THEN** the later run writes its own coverage declaration, and the
  original declaration is never mutated
