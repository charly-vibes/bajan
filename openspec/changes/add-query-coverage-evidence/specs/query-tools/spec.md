# query-tools delta — coverage status on zero-hit reads

## ADDED Requirements

### Requirement: Zero-hit reads report corpus coverage instead of asserting absence

The system SHALL compute corpus coverage for reads — `coverage-complete`
when every episode declared by the coverage declarations persisted in the
store is persisted, `coverage-partial` with declared and persisted counts
otherwise — and SHALL attach the `coverage-partial` marker and counts to
any read that returns zero hits under partial coverage, so that an
incomplete corpus is never reported as a bare `not-found`.

#### Scenario: Zero hits over a partial corpus carry the marker

- **WHEN** a read returns zero hits while some declared episodes of
  relevant import runs are not persisted
- **THEN** the result reports `coverage-partial` with its declared and
  persisted counts, and never a bare `not-found`

#### Scenario: Zero hits over a complete corpus stay clean

- **WHEN** a read returns zero hits and all declared episodes of relevant
  import runs are persisted
- **THEN** the result reports `not-found` with budget status per
  `qt_bounded_traversal`, with no coverage marker

#### Scenario: Relevance spans all persisted declarations

- **WHEN** a read returns zero hits while any persisted coverage
  declaration names episodes that are not persisted, from any import run
- **THEN** the result reports `coverage-partial` — declarations are
  never scoped or filtered beyond being persisted in the store

#### Scenario: Budget exhaustion outranks coverage in reason precedence

- **WHEN** a read exhausts its traversal budget before completing over a
  partially covered corpus and returns zero hits
- **THEN** the result reports `budget-exhausted` per
  `qt_bounded_traversal`, because a truncated scan is definitive about
  neither the world nor the corpus
