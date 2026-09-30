# Change: Add Optional Jev Verification to the Extraction Seam

## Why

A Rule-of-5 review (2026-09-29, grounded against the live TypeSafe docs at
`docs.typesafe.ai/introduction` and the `sde_cascade` / `date_extraction`
cookbooks) evaluated where TypeSafe's Jev model fits bajan's pipeline. Two
findings drive this change:

1. **Jev is not an extractor.** Jev is a System One model: it answers
   Choice / Score / Noul questions against a state and returns typed
   answers with probabilities and confidence — no text generation. It
   cannot "propose candidate claims." The SDE-cascade cookbook establishes
   the proven division of labor: a text LLM extracts cheaply, then Jev
   *verifies* per field with Noul questions ("is this value absent from the
   source?", "was it lifted from unrelated text?"), each returning P(wrong).
2. **Verification is genuinely valuable at bajan's extraction seam** — it
   adds calibrated per-field probability behind the existing deterministic
   gate (evidence containment, hedge anchoring) that a plain text check
   cannot provide — **but it is a paid API call per episode.** It must
   therefore be an optional feature, disabled by default, with zero cost
   and zero behavior change when disabled.

This preserves every existing invariant: `ex_single_call` (one *extraction*
call per episode — Jev verifies, it never extracts), the deterministic
post-processing rule (verification checks fields; it never dedupes, merges,
or matches entities), and `ic_no_format_parsing` (ingest persist stays
AI-free).

## What Changes

Spec-level (apply stage lands as specodelic constraint edits to
`specs/extraction-claims.md`; this change records the deltas and the design
decisions behind them):

- **extraction-claims** — four new invariants plus two scoped amendments:
  - `ex_jev_optional`: Jev verification is opt-in configuration, disabled
    by default; with it disabled the pipeline makes no verification calls,
    incurs no cost, and produces observably identical outcomes (no API
    calls, no new reasons). Configuration also supports a record-only
    mode — verification results recorded on the run row without gating —
    so the threshold can be calibrated on a real corpus before
    enforcement ("eval before adoption", project.md).
  - `ex_jev_verify`: when enabled, verification runs as one batched Jev
    request per episode (all per-field questions in a single call, per the
    TypeSafe parallel-evaluation contract); the question set is restricted
    to genuinely semantic fields and never re-checks what the
    deterministic gate verifies exactly (hedge preservation, containment);
    the threshold is validated strictly between 0 and 1; zero-candidate
    episodes skip the request; the deterministic gate rejects any
    candidate whose verified field exceeds the threshold, recording the
    machine-readable reason `jev-verification-failed` — candidates are
    rejected, never repaired.
  - `ex_jev_infra`: Jev transport failures (timeout, rate limit, connection)
    are infrastructure failures, classified like `RepeatedCallFailure` —
    never gate rejections, never parked as semantically invalid;
    uninterpretable verification answers park on the same infrastructure
    path rather than passing or failing any candidate.
  - `ex_jev_version`: the verifier model id and question-set version form
    the verifier version, cached per episode separately from the
    `(episode id, extractor version)` extraction cache — a verifier bump
    re-verifies against cached extraction output without a new extraction
    call; an extractor bump re-extracts and re-verifies as new work.
    Verification signals live on run/rejection records only — claim-node
    schema v2 field set is closed and unchanged.
- **extraction-claims, amendments to existing constraints** (recorded in
  the delta; applied at apply stage):
  - `ex_no_llm_post` is scoped to its named phases (normalization, dedup,
    entity matching, conflict detection); opt-in verification is carved
    out per `ex_jev_verify` — without this amendment the corpus
    self-contradicts ("never invoke an LLM" after the extraction call vs.
    the verifier).
  - `ex_run_record` gains verifier fields on the extraction run row when
    enabled (verifier model id, question-set version, per-field results);
    the Jev request is not a separate run row and cache economics are
    unchanged.
  - when verification is enabled, the `refuse` transition guard extends to
    the conjunction `[[ex_typed_gate]] [[ex_jev_verify]]` (conjunction-
    guard precedent: `gm_human_adopt`).
- **graph-model** — unchanged. No schema change: verification is not a
  claim-node field, and human-only adoption is untouched.

Not in this change: Jev-assisted entity alignment for `resolve` (deferred —
the entity-alignment cookbook is the template when entity-review needs
semantic disambiguation), Jev date-parts extraction as a separate
mechanism (subsumable into the verifier question set at apply time), any
implementation beyond what the new invariants' tests require.

## Impact

- **Affected specs:** `specs/extraction-claims.md` (specodelic; four new
  constraints with deriving properties plus two scoped amendments:
  `ex_no_llm_post`, `ex_run_record`). `specs/graph-model.md` untouched.
- **Affected code:** `src/extract.rs` (verification hook after candidate
  proposal, before the deterministic gate), `src/cli.rs` (opt-in
  configuration surface: enable flag, record-only mode, validated
  threshold, pinned model id, question-set version), new optional TypeSafe
  client module behind a trait so the default build carries no API
  dependency in the call path.
- **Cost:** zero by default; when enabled, one batched Jev request per
  episode at the published verifier rate ($0.042/1M input tokens, output
  free); record-only mode costs the same per episode but gates nothing;
  a verifier bump re-verifies against cached extraction output (no
  re-extraction cost).
- **Risk:** low — the disabled path is the existing pipeline; the enabled
  path only *tightens* the existing gate (it can only reject, never admit,
  and never mutates claim nodes).