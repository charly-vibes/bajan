# extraction-claims delta — optional Jev verification at the gate

Lands at apply as edits to the specodelic spec `specs/extraction-claims.md`
(new constraints `ex_jev_optional`, `ex_jev_verify`, `ex_jev_infra`,
`ex_jev_version` with deriving properties; amendments to existing
constraints as noted below). Recorded here as ADDED requirements because
the normative format is specodelic, not openspec; this delta is the
reviewable contract for those edits. `graph-model` is untouched by this
change.

**Amendments to existing constraints at apply** (Rule-of-5 review
2026-09-29):

- `ex_no_llm_post` — scoped to its named phases (normalization, dedup,
  entity matching, conflict detection); opt-in verification is carved out
  per `ex_jev_verify`, so the corpus no longer implies "never invoke an
  LLM after the extraction call" without exception.
- `ex_run_record` — when verification is enabled, the extraction run row
  additionally carries the verifier fields (verifier model id,
  question-set version, per-field results). The Jev request is not a
  separate run row; cache economics are unchanged.
- `ex_typed_gate` — unchanged in text; the verification hook sits between
  candidate proposal and the gate and can only reject (`refuse`
  transition guard extends to the conjunction
  `[[extraction.claims.ex_typed_gate]] [[extraction.claims.ex_jev_verify]]`
  when enabled, per the conjunction-guard precedent of `gm_human_adopt`).

## ADDED Requirements

### Requirement: Jev verification is optional and disabled by default

The system SHALL treat TypeSafe (Jev) verification of extracted candidates
as opt-in configuration that is disabled by default, and SHALL guarantee
that with verification disabled the extraction pipeline makes no
verification calls, incurs no cost, emits no verification-specific
reasons, and produces outcomes observably identical to the pipeline
without this feature. The configuration SHALL also support a record-only
mode in which verification results are computed and recorded on the run
row without gating candidates, for threshold calibration before
enforcement.

#### Scenario: Disabled path is the unchanged pipeline

- **WHEN** verification is not enabled in configuration and an episode is
  extracted
- **THEN** no TypeSafe request is made, the run record shows the
  extraction call only, no verification-specific reason can appear, and
  the outcome matches the pre-feature pipeline exactly

#### Scenario: Verification only tightens the gate

- **WHEN** verification is enabled and a candidate passes Jev
  verification
- **THEN** the candidate still passes through the deterministic
  containment and hedge gates unchanged — verification can reject
  candidates the deterministic gate would accept, but never admits one it
  would reject

#### Scenario: Record-only mode calibrates without gating

- **WHEN** verification runs in record-only mode over a corpus
- **THEN** per-field verification results are recorded on the run rows,
  no candidate is rejected with `jev-verification-failed`, and the
  recorded P(wrong) distribution is resolvable for threshold calibration

### Requirement: Enabled verification batches per-field checks into one request per episode

The system SHALL, when verification is enabled, evaluate all per-field
verification questions for an episode's candidates in one batched TypeSafe
request per episode — each question returning a typed answer with
probability — and SHALL reject any candidate whose verified field's
P(wrong) exceeds the configured threshold with reason
`jev-verification-failed`, never repairing or auto-correcting the
candidate. The question set SHALL be restricted to genuinely semantic
fields (evidence support, date reading, absence or unrelated-text checks)
and SHALL NOT include any field the deterministic gate already verifies
exactly (hedge preservation, evidence containment); the threshold SHALL
be validated to lie strictly between 0 and 1 at configuration time; and
an episode with zero candidates SHALL skip the verification request
entirely.

#### Scenario: Batched verification in one request

- **WHEN** verification is enabled and extraction produces N candidate
  claims with K checked fields for one episode
- **THEN** all N×K verification questions are evaluated in a single
  TypeSafe request for that episode, not one request per question

#### Scenario: Fired verification rejects with a reason

- **WHEN** a candidate's verified field reports P(wrong) above the
  configured threshold
- **THEN** the candidate is rejected with reason
  `jev-verification-failed`, the reason is recorded on the run/rejection
  record per the run-record requirement, and the candidate is never
  repaired

#### Scenario: Below-threshold verification does not bypass the deterministic gate

- **WHEN** a candidate's verification reports all fields below threshold
- **THEN** the candidate proceeds to the deterministic evidence-containment
  and hedge gates as usual; verification passing does not skip any
  existing check

#### Scenario: Deterministic-verifiable fields are never verified probabilistically

- **WHEN** the verifier question set is constructed for an episode
- **THEN** no question covers hedge preservation or evidence containment —
  both are enforced exactly by the deterministic gate — and the set covers
  only semantic fields

#### Scenario: Out-of-range threshold is rejected at configuration time

- **WHEN** verification is configured with a threshold of 0, 1, or a
  value outside [0, 1]
- **THEN** configuration fails with a validation error before any
  extraction runs; a threshold of 0 can never reject every candidate and
  a threshold of 1 can never silently disable gating while paying cost

#### Scenario: Zero candidates skip the verification request

- **WHEN** an episode extracts to zero candidates (cached as legitimately
  empty per the typed-gate requirement)
- **THEN** no TypeSafe request is made for that episode and the cached
  empty output is unchanged

### Requirement: Jev transport failures are infrastructure failures, never gate rejections

The system SHALL classify TypeSafe request failures (timeout, rate limit,
authentication, connection) as infrastructure failures classified like
repeated call failure — with an attempt counter and an infrastructure
reason — and SHALL never park or reject an episode's candidates as
semantically invalid because verification was unavailable; when
verification answers arrive but cannot be interpreted, the episode SHALL
park on the same infrastructure path (reason and attempt count recorded),
rather than passing or failing any candidate on the strength of an
unusable verification result.

#### Scenario: Jev outage parks with infrastructure reason

- **WHEN** the TypeSafe request for an episode fails on transport after
  repeated attempts
- **THEN** the episode is parked with an infrastructure failure reason and
  attempt count, its cached extraction output is untouched, and no
  candidate is rejected with `jev-verification-failed`

#### Scenario: Uninterpretable verification answers do not widen the gate

- **WHEN** verification answers arrive but cannot be evaluated against the
  threshold
- **THEN** the episode parks on the infrastructure path — the same reason
  class and attempt-count mechanism as a transport failure, recorded as
  the run record's infrastructure outcome — and no candidate passes or
  fails the gate on unusable verification

### Requirement: Verifier identity is pinned and versioned outside the claim node

The system SHALL pin the verifier model id and question-set version as the
verifier version, keyed per episode alongside — but separately from — the
`(episode id, extractor version)` extraction cache: a verifier bump
re-verifies the episode against its cached extraction output without a new
extraction call, while an extractor bump re-extracts and re-verifies as
new work per the existing cache economics; and the system SHALL record
verification signals (verifier model id, question-set version, per-field
results) on run/rejection records only — the claim-node field set remains
the frozen v2 set with no verification or confidence fields.

#### Scenario: Verifier bump re-verifies without re-extracting

- **WHEN** verification is enabled and the verifier model id or
  question-set version changes
- **THEN** affected episodes are re-verified against their cached
  extraction output under the new verifier version, no new extraction
  call is started, and no verification results are reused across verifier
  versions

#### Scenario: Extractor bump re-extracts and re-verifies

- **WHEN** the extractor version changes
- **THEN** the episode re-extracts under the new extractor version per the
  existing supersession and cache requirements, and the new output is
  verified under the pinned verifier version

#### Scenario: Verification signals never reach claim nodes

- **WHEN** any verification result is recorded
- **THEN** claim nodes expose exactly the v2 field set with no confidence
  or verification fields, and the signals are resolvable from the
  run/rejection record of the extraction run that produced the candidate