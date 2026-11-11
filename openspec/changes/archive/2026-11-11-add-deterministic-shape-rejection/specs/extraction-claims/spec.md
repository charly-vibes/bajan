# extraction-claims delta — deterministic candidate-shape rejection

## ADDED Requirements

### Requirement: Non-claim shapes are rejected deterministically

The system SHALL reject, in the deterministic post-pass and without any
LLM judgment, every candidate claim whose text matches a fixed-in-code
non-claim shape set (interrogative-final form, heading or section-title
fragments, navigation and list boilerplate), recording the rejection with
machine-readable reason `shape-filtered` on the run record per
`ex_run_record` and never mutating or repairing the candidate.

#### Scenario: Interrogative candidate is shape-filtered

- **WHEN** a candidate claim's text is interrogative-final after
  whitespace collapse
- **THEN** the candidate never reaches `staged`, and the run record
  carries reason `shape-filtered`

#### Scenario: Heading fragment is shape-filtered

- **WHEN** a candidate's evidence span aligns inside a converter-supplied
  heading anchor rather than body prose
- **THEN** the candidate never reaches `staged`, and the run record
  carries reason `shape-filtered`

### Requirement: Non-matching candidates and fully filtered episodes behave unchanged

The system SHALL pass candidates that match no non-claim shape through the
post-pass unchanged toward the validation gate, and SHALL cache an episode
whose candidates are all shape-rejected as a legitimately extracted empty
result per `ex_single_call` cache economics, so no re-extraction call is
spent on it.

#### Scenario: Prose candidate is unaffected

- **WHEN** a candidate matches no rule in the shape deny set
- **THEN** it proceeds to the validation gate exactly as before this
  feature, with no new reasons

#### Scenario: Fully filtered episode caches as empty

- **WHEN** every candidate of an episode is shape-rejected
- **THEN** the episode caches as a legitimately extracted empty result
  and a later extraction pass starts no new call for it at the same
  extractor version

#### Scenario: Deny-set changes are forward-only

- **WHEN** the shape deny set changes in a later post-pass version
- **THEN** already cached and staged episodes are not re-filtered,
  because the extraction cache stores post-pass output; the new set
  binds only extractions at the bumped version
