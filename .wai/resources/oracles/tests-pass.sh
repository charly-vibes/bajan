#!/usr/bin/env bash
# tests-pass.sh — oracle: the relevant test suite must pass (exit 0).
# Ignores artifact paths; the suite itself is the gate for TDD steps.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
cargo test --quiet 2>&1 | tail -5
