---
tags: [pipeline-run:tdd-ro5-2026-11-11-bajan-5ux-evidence-refs, pipeline-step:ro5u-review]
---

# RO5U bajan-5ux (diff 51ac050..tidy, src/store/sqlite.rs + src/cli.rs + tests/evidence_refs.rs)

Converged stage 1, verdict **GOOD**, 0 critical.

## Findings

- **EDGE-001 MEDIUM deferred with reason** — multi-source claims
  (`gm_reified` N lineage edges) get a reference recording their FIRST
  lineage edge only: design D1 pins the reference schema singular
  (claim_key, episode_id, …), so this is design-faithful, not a defect.
  Multi-source references are a locator-scheme-version extension, not a
  v1 relaxation.
- **EDGE-002 LOW no-fix** — two concurrent check processes could race the
  lazy reference creation; the second INSERT fails the claim_key PRIMARY
  KEY and surfaces as an honest infrastructure error, never a duplicate
  or a fabricated ref. Single-writer busy_timeout discipline unchanged.
- **CORR-001 LOW no-fix (documented)** — a claim with typed-Unknown
  evidence gets a reference (episode_id from lineage) whose state is
  `unresolvable`: the ref records where the claim's evidence SHOULD
  live, the state records that no anchor is verifiable — honest
  combination, never fabricated.
- **CORR-002 LOW no-fix** — post-revive `resolved` requires the
  post-revive RE-INGEST (`ic_deleted_recreate`): revive alone clears the
  tombstone but the episode row stays gone until recreation, so a check
  in that gap is honestly `unresolvable`. Verified as the store's real
  behavior in the tombstone test (caught red, fixture corrected).
- **INTERPLAY-003 MEDIUM discovered red, pinned as assertion** —
  task 3.1's literal "delete episode → check (tombstoned)" is
  unreachable through the live seam: `gm_lineage_survives` refuses
  policy-deleting an episode with a supporting claim
  (`DeleteRefused`, counts all lineage). The tombstone window is
  reached the way a dump-rebuild reaches it: claim lineage predating
  the deletion. The refusal itself is now pinned as an assertion in the
  lifecycle test; the tombstoned→revive→re-ingest→resolved legs live in
  the dedicated test.
- **CLAR-001 LOW no-fix** — `derive_resolution` was extracted pure +
  table-driven at birth (7-case table including whitespace-collapse and
  precedence rows), satisfying task 4.1's intent before the tidy commit;
  the tidy commit carries only the README quickstart.

## Gates at review time

just test 326 ingested green; evidence_refs 7 + cli evidence envelopes 2
green; clippy --all-targets 0; fmt clean; spk lint 7 files 0 issues;
ah check 0; ah check --run-tests 193 passed 0 findings; just test-full
green; spec-drift-gate green at both commits; dont check grounded.

## Risks

- "Claim without reference = not checked" is the default state (design
  risk, accepted): consumers must join evidence_refs to distinguish.
- Check-gated staleness is manual — visible per checked_at, never
  auto-refreshed (design D1, accepted for v1).
