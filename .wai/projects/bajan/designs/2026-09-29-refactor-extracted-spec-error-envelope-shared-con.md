---
tags: [pipeline-run:tdd-ro5-2026-09-29-bajan-0hs-4-human-only-adopt-path, pipeline-step:refactor]
---

REFACTOR: extracted spec_error_envelope shared construction point (Invariant 3.2.5 by construction) — stub_envelope + adopt_error_envelope now delegate; stub_envelope returns Value, dispatch un-nested. No behavior change: 25 tests + clippy -D warnings green.
