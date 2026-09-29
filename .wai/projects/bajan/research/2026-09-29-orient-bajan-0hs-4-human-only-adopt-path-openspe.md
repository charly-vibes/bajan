---
tags: [pipeline-run:tdd-ro5-2026-09-29-bajan-0hs-4-human-only-adopt-path, pipeline-step:orient]
---

ORIENT: bajan-0hs.4 human-only adopt path (openspec tasks 3.1-3.2). Unblocked: spec deps .1/.2 closed, scaffold closed. Scope: no automated stage can move proposed->active (test drives every non-human path, red); implement actor check on adopt path (CLI bajan adopt), batch accept with per-claim audit records (actor=suite-config operator identity, timestamp) (green). Files: src/cli.rs, src/store.rs. Anti-goals: no auth model, no interactive prompt, preserves per-claim reversibility. Meter: cargo test red then green; bajan adopt --json | jq '.ok' == true. gm_human_adopt guard is conjunction [[gm_human_adopt]] [[gm_reified]].
