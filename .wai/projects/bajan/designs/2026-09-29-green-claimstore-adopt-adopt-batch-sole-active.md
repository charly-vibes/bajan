---
tags: [pipeline-run:tdd-ro5-2026-09-29-bajan-0hs-4-human-only-adopt-path, pipeline-step:green]
---

GREEN: ClaimStore::adopt/adopt_batch (sole Active-setting API — refusal is structural), AuditRecord{claim_key,actor,adopted_at}, StoreError::{ClaimNotFound,AdoptRefused}; CLI Command::Adopt{claims,--actor} wired to session store via adopt_envelope — honest ok:false on empty store (no persistence engine yet), ok:true test via seeded store + adopt_envelope (persistence-meter deferred to vertical slice). 26 tests + clippy -D warnings green.
