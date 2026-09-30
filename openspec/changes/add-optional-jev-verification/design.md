# Design — add-optional-jev-verification

## D1: Jev verifies; it never extracts

Jev's capability model (introduction.md) is three question primitives —
Choice, Score, Noul — evaluated in isolation against a state. It cannot
produce the open-ended artifact "candidate claims." The extraction call
therefore remains the spec's single text-LLM call per episode
(`ex_single_call`); Jev enters *after* candidate proposal and *before* the
deterministic gate, as a verifier rung per the SDE-cascade pattern:

```
episode → extraction LLM (proposes) → Jev verifier (batched Nouls)
        → deterministic gate (containment, hedge) → staged / rejected(reason)
```

Consequence: the verifier can only *tighten* the gate. It never admits a
candidate the deterministic gate would reject, never repairs, never
touches claim nodes.

## D2: Opt-in, default off, zero-cost disabled path

The user's constraint: this is additional cost, so it ships optional.
Configuration is explicit (CLI flag / config key, name fixed at apply).
With verification disabled:

- no TypeSafe client is constructed,
- no question-set code runs,
- no new reason codes can appear,
- extraction output is identical to today's.

Test shape: a property asserting the disabled path produces run records and
outcomes indistinguishable from the current implementation (regression
guard, not just absence of calls).

## D3: One batched request per episode, semantic fields only

TypeSafe evaluates all questions in parallel in isolation against the same
state — "adding questions barely changes the response time." Verification
of N candidate claims × K checked fields is batched into **one** request
per episode. Never one request per question: that would multiply cost and
latency for no semantic gain.

Question-set discipline (Rule-of-5 EDGE-001): the set covers only
genuinely semantic fields — evidence-support Noul, date-parts Choice where
applicable, absence/unrelated-text checks per sde_cascade. It MUST NOT
cover fields the deterministic gate already verifies exactly: hedge
preservation (`gm_hedge_anchor` / `HedgeMarkerDropped`) and evidence
containment (`ex_evidence_containment`) are exact code checks; paying a
probabilistic Noul to re-buy them wastes cost per episode and creates two
sources of truth that can contradict.

Threshold: per-field P(wrong) above the configured threshold (cascade
reference: 0.7) fails the candidate with reason `jev-verification-failed`.
The threshold is configuration, not a constant, and is validated to lie
strictly between 0 and 1 at configuration time (EDGE-002: 0 would reject
every candidate unconditionally; 1 would silently disable gating while
still paying per-episode cost). Calibration is aided by a record-only
mode: verification runs and results are recorded on the run row but gate
nothing, so the P(wrong) distribution on a real corpus can be reviewed
before enforcement — the project's "eval before adoption" principle
applied to the verifier itself.

Zero-candidate episodes (EDGE-003): an episode cached as legitimately
empty per `ex_typed_gate` skips the TypeSafe request entirely — a request
with zero questions is pure round-trip cost.

## D4: Infrastructure failures are never gate rejections

A Jev transport failure (timeout, rate limit, auth, connection) is an
availability problem, not a semantic judgment. Existing invariant already
encodes the rule (`infrastructure_failures_are_never_gate_rejections` in
`src/extract.rs`); the verifier reuses it: transport failures classify like
`RepeatedCallFailure` — attempt counter, park with an infrastructure
reason, cached output untouched. A *fires-but-fails* verification (model
answered, code can't parse the answer) parks for review rather than
silently passing — a failed verifier check must never widen the gate.

## D5: Version pinning and cache identity — split, not merged

Verification results depend on the model and the question set, so the
verifier model id (e.g. `jev-1.12`) and question-set version together form
the verifier version. Cache identities are split (Rule-of-5 CORR-002):

- extraction output stays cached by `(episode id, extractor version)` —
  unchanged from `ex_single_call`;
- verification results are keyed by `(episode id, verifier version)` and
  computed against the cached extraction output.

A verifier bump therefore re-verifies the episode against its cached
extraction output — no new extraction call — because D1's own pipeline
makes extraction output verifier-independent; merging the identities would
pay the expensive extraction call to reproduce identical output. An
extractor bump re-extracts and re-verifies as new work per the existing
supersession and cache requirements. No cache reuse across versions on
either axis; `ic_idempotent` holds. Model jaggedness is documented
per-version upstream (model-jaggedness/jev-1.13); pinning is mandatory,
"latest" is never implicitly used.

Run-row identity (Rule-of-5 CLAR-001): the Jev request is not a separate
run row. When enabled, the extraction run row additionally carries the
verifier fields (verifier model id, question-set version, per-field
results), mirroring `ex_run_record`'s "parallel to, and without changing,
cache economics" extension pattern. At apply, the `refuse` transition
guard extends to the conjunction
`[[extraction.claims.ex_typed_gate]] [[extraction.claims.ex_jev_verify]]`
when verification is enabled — the conjunction-guard precedent of
`gm_human_adopt` applies (specodelic guards accept conjunction citations).

## D6: No claim-node schema change

Verification signals (P(wrong) values, fired checks, verifier version) are
provenance-of-rejection, not claim content. They live on run/rejection
records — claim nodes keep the frozen v2 field set (`gm_schema_v2`; the
field list stays closed, no confidence field). Review of a
`jev-verification-failed` rejection reads the run record, same as every
other gate rejection.

## D7: Deferred — entity alignment and date-parts as standalone mechanisms

The entity-alignment cookbook (Score + companion Nouls over candidate
pairs) is the template for `resolve`/entity-review when that module needs
semantic disambiguation — but resolve is deterministic per spec and stays
so in this change. Jev date-parts extraction (date_extraction cookbook:
Choice reads the parts, code resolves the calendar) can enter as verifier
questions over claim dates at apply time if the question set wants it;
it is not a separate mechanism here.