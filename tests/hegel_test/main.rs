//! Hegel property tests for bajan (bajan-llg).
//!
//! Purpose: event-sourced property tests per the antithesis/scratchbook plan
//! (bajan-hsf): dates, claim-key identity, dump/restore decision trail, and
//! the two-store differential state machine.
//! Responsibilities: own the single `hegel_test` integration-test target;
//! each module owns one property and never touches the existing proptest
//! blocks in tests/.
//! Rationale: hegeltest (static-engine) runs via cargo test like any other
//! harness; the plan's predicted red/green is recorded per module, with
//! confirmed defects as ignore-gated red tests linked to bug tickets.

pub mod dates;
pub mod decision_trail;
pub mod differential;
pub mod key_identity;
