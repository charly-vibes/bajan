# Tasks — add-evidence-references

Ordered, TDD-first (red → green per item; refactors land as separate tidy
commits). Each specodelic edit and its deriving property land in the same
commit so `spk lint specs`, `ah check`, and `dont check` stay green. Do
not start until the proposal is approved.

## 1. Spec deltas (normative specodelic edits)

- [ ] 1.1 `specs/graph-model.md`: add `gm_evidence_ref` constraint +
      deriving property (side record keyed by claim — episode id, source
      revision digest, locator scheme version, resolution state,
      last-checked, observed tier; frozen ClaimNode untouched;
      reference-less claims unchanged)
- [ ] 1.2 `specs/graph-model.md`: add `gm_evidence_resolution` constraint
      + deriving property (tri-state resolved/unresolvable/tombstoned,
      containment re-check per `ex_evidence_containment` comparison, no
      fabrication, revive-survivor semantics, unresolvable stays
      inspectable)
- [ ] 1.3 `specs/graph-model.md`: add `gm_evidence_ref_tier` constraint +
      deriving property (tier = creation-time provenance; resolution
      never re-ranks per `qt_ranking`)
- [ ] 1.4 `spk lint specs`, `ah check`, `dont check` green; contract
      TOMLs for all three deriving properties

## 2. Implementation (red → green)

- [ ] 2.1 `evidence_refs` side table + lazy creation on first reference;
      claim node byte-identical before/after (red first)
- [ ] 2.2 Resolve operation: containment re-check (whitespace-collapsed
      comparison shared with the extraction gate), tri-state derivation
      from store facts, last-checked stamp (green)
- [ ] 2.3 Tombstone interplay: resolve during tombstone window →
      `tombstoned`; revive → subsequent resolve `resolved`; revive never
      mutates reference rows (red → green)
- [ ] 2.4 No-fabrication guard: unresolvable path invents no anchor text;
      claim stays queryable (red → green)

## 3. Verification

- [ ] 3.1 End-to-end: ingest → extract → reference → resolve (resolved);
      delete episode → resolve (tombstoned) → revive → resolve (resolved)
- [ ] 3.2 `just test` green; clippy + fmt clean; `just test-full` green

## 4. Tidy

- [ ] 4.1 Separate tidy commit: resolve logic extracted pure +
      table-driven tests; docs touch-up
