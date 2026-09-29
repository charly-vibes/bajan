# Tasks — update-extraction-provenance

Ordered, TDD-first (red → green per item; refactors land as separate
tidy commits per project convention). Each specodelic edit and its
deriving property land in the same commit so `spk lint`, `ah check`, and
`dont check` stay green throughout.

## 1. Spec deltas (normative specodelic edits)

- [x] 1.1 `specs/graph-model.md`: add `gm_human_adopt` constraint
      (human-only `adopt` actor, batch accept audited per claim) + deriving
      property; update `adopt` transition guard — resolved 2026-07-29:
      specodelic guards DO accept conjunction citations (`spk lint` green),
      so the guard is the conjunction `[[gm_human_adopt]] [[gm_reified]]`,
      not the sole-guard fallback
- [x] 1.2 `specs/graph-model.md`: modify `gm_schema_v1` → `gm_schema_v2`
      (eighth field `evidence`: verbatim span + locator, or the typed
      absent marker per the ingestion-contract convention — never a literal
      colliding with a real span); extend `gm_hedge_anchor` to evidence
      spans + deriving property
- [x] 1.3 `specs/extraction-claims.md`: add `ex_supersession` constraint
      + deriving property (version-bump supersession of `proposed` claims
      only; `active`/`rejected` preserved; tombstone reason
      `superseded-by-reextraction`) — done 2026-09-29, incl. new
      `supersede` model transition (staged → rejected)
- [x] 1.4 `specs/extraction-claims.md`: add `ex_run_record` constraint
      + deriving property (run row per extraction call; machine-readable
      reasons for parked episodes and gate rejections; reasons never on
      claim nodes) — done 2026-09-29
- [x] 1.5 `specs/extraction-claims.md`: add `ex_evidence_containment`
      constraint + deriving property (whitespace-collapsed containment at
      the typed gate; `evidence-not-contained` reason; typed-absent spans
      persist flagged by the reflection pass, never reported as
      violations); amend `ex_typed_gate` to require evidence — done
      2026-09-29
- [x] 1.6 Run gates: `spk lint specs`, `ah check`, `dont check` all green
      (new constraints cite this change's docs / upstream fpa files as
      provenance) — all green 2026-09-29; specodelic artifacts for
      graph-model and extraction-claims regenerated in the same commits

## 2. graph-model implementation (schema v2)

- [x] 2.1 Write failing property test: claim nodes expose exactly the v2
      field set (seven v1 fields + `evidence`) — red (3cf426d); green:
      `ClaimNode` with `deny_unknown_fields`, extra/missing-required/
      mistyped rejected; dated fields: absent key ≡ explicit-null (undated)
- [x] 2.2 Add `evidence` to the claim-node schema/types; backfill
      migration sets `evidence = unknown` on existing nodes and reports
      the count loudly — green: `migrate_v1_to_v2` → `MigrationReport`
      (count rendered via Display; `reextraction_scheduled` pinned at 0)
- [x] 2.3 Write failing test: evidence span preserves hedge markers from
      the supporting episode text — red; enforce in span persistence —
      green: `dropped_hedge_markers` + `ClaimStore::insert` rejecting
      `HedgeMarkerDropped` (span must locate in episode, whitespace-
      collapsed; containment supersedes at bajan-0hs.7)

## 3. Adoption actor (human-only adopt)

- [x] 3.1 Write failing test: no automated stage can move proposed→active
      (drive adopt through every non-human path and observe refusal) — red;
      structural guarantee: `ClaimStore::adopt`/`adopt_batch` are the sole
      Active-setting public API
- [x] 3.2 Implement actor check on the `adopt` path (CLI `bajan adopt`);
      batch accept loops with per-claim audit records (actor, timestamp) —
      green: `AuditRecord`, `adopt_batch` per-claim loop, CLI adopt
      envelope; session-scoped store until persistence engine lands

## 4. Supersession (re-extraction safety)

- [x] 4.1 Write failing property test: version-bump re-extract supersedes
      only that episode's `proposed` claims from prior versions; active and
      rejected claims unchanged; lineage/evidence survive tombstoning — red
      (755aa7d)
- [x] 4.2 Implement supersession on the re-extract path (reject with
      reason `superseded-by-reextraction`, keep lineage); route
      active-claim conflicts through the existing invalidation-proposal
      mechanism — green: `ClaimStore::supersede_prior_versions` over
      `Lineage` provenance (episode id + extractor version, parallel to
      the closed node schema), `SupersessionRecord` tombstones with
      `Reason::SupersededByReextraction` (`superseded_by_reextraction`),
      `stage_invalidation_proposal`/`InvalidationProposal` for
      `ex_mutation_proposal`; same-version re-ingest and repeat bumps
      resurrect nothing (no duplicate tombstones)

## 5. Run records + reasons

- [x] 5.1 Write failing test: every extraction call writes a run row
      (episode id, extractor version, model id if any, timestamps, finish
      status); parked episodes and gate rejections carry machine-readable
      reasons; claim nodes carry no reason field — red
- [x] 5.2 Implement run-record store and reason plumbing beside the
      existing `(episode id, extractor version)` cache — green: `Reason`
      (snake_case codes), `Finish` (failure outcomes carry reason
      structurally), `ExtractionRun`, append-only `ExtractionRunStore`;
      claim-nodes-expose-no-reason pinned by test

## 6. Evidence containment (typed gate)

- [ ] 6.1 Write failing property test: spans failing whitespace-collapsed
      containment are rejected with `evidence-not-contained`, never
      repaired; `unknown` spans persist flagged by the audit pass — red
- [ ] 6.2 Extend the gate with containment verification and reason
      recording; unknown-span flagging rides the deterministic reflection
      pass (`ex_reflection` extended) — no second audit mechanism — green

## 7. Validation & tidy

- [ ] 7.1 Full gate run: `spk lint specs`, `ah check`, `dont check`,
      `pretender` pre-commit, full test suite
- [ ] 7.2 Tidy pass: extract shared span-normalization helper
      (whitespace collapse) if duplicated across gate and audit; separate
      tidy commit
- [ ] 7.3 Update `openspec/changes/update-extraction-provenance/tasks.md`
      statuses; request review for archive