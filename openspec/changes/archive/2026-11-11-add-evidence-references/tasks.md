# Tasks — add-evidence-references

Ordered, TDD-first (red → green per item; refactors land as separate tidy
commits). Each specodelic edit and its deriving property land in the same
commit so `spk lint specs`, `ah check`, and `dont check` stay green. Do
not start until the proposal is approved.

## 1. Spec deltas (normative specodelic edits)

- [x] 1.1 `specs/graph-model.md`: add `gm_evidence_ref` constraint +
      deriving property (side record keyed by claim — episode id, locator
      scheme version, observed tier; created only by the evidence check
      command; frozen ClaimNode untouched; reference-less claims
      unchanged)
- [x] 1.2 `specs/graph-model.md`: add `gm_evidence_resolution` constraint
      + deriving property (tri-state derivation, containment re-check per
      `ex_evidence_containment` comparison, no fabrication, tombstone /
      post-revive semantics, unresolvable stays inspectable)
- [x] 1.3 `specs/graph-model.md`: add `gm_evidence_check_record`
      constraint + deriving property (append-only immutable check records
      — claim key, state, checked-at; the evidence check command is the
      only writer; `qt_readonly` intact)
- [x] 1.4 `specs/graph-model.md`: add `gm_evidence_ref_tier` constraint +
      deriving property (tier = provenance only; never re-ranks per
      `qt_ranking`)
- [x] 1.5 `spk lint specs`, `ah check`, `dont check` green; contract
      TOMLs for all four deriving properties

## 2. Implementation (red → green)

- [x] 2.1 `evidence_refs` + `evidence_checks` tables; `bajan evidence
      check <claim-id>` creates the reference on first check and appends
      one check record per run; claim node byte-identical before/after
      (red first)
- [x] 2.2 Tri-state derivation: containment re-check (whitespace-collapsed
      comparison shared with the extraction gate), tombstoned/unresolvable
      from store facts (green)
- [x] 2.3 Tombstone interplay: check during tombstone window →
      `tombstoned`; post-revive check → `resolved`; revive never rewrites
      prior check records (red → green)
- [x] 2.4 Readonly discipline: read paths write nothing to either table;
      no-fabrication guard invents no anchor text; claim stays queryable
      (red → green)

## 3. Verification

- [x] 3.1 End-to-end: ingest → extract → check (resolved); delete episode
      → check (tombstoned) → revive → check (resolved); verify both check
      records persist append-only
- [x] 3.2 `just test` green; clippy + fmt clean; `just test-full` green

## 4. Tidy

- [x] 4.1 Separate tidy commit: check derivation extracted pure +
      table-driven tests; docs touch-up
