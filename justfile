# bajan — dev commands
#
# Same commands run locally and in CI for consistent diagnostics.
# Run `just` for default (build + test), `just ci` for full pipeline,
# `just gates` for the spec-corpus gates (spk lint + ah check).

set shell := ["bash", "-uc"]

# Default: build and test
default: build test

# === Build Commands ===

# Build debug binary
build:
    cargo build

# Build release binary (optimized)
build-release:
    cargo build --release

# Run with arguments (e.g., `just run ingest`)
run *args:
    cargo run -- {{args}}

# Install locally to ~/.cargo/bin
install:
    cargo install --path .

# === Test Commands ===

# Run tests with output
test-verbose:
    cargo test -- --nocapture

# Run a specific test
test-one name:
    cargo test {{name}} -- --nocapture

# TDD loop (most obvious entry, DDL test-runner standard — see the
# dulce-de-leche repo ticket): testaruda exec — select the tests the
# working diff affects, run only those, ingest, calibrate. The gate script
# owns the tolerance contract (0/10/20 green, calibration veto per
# tambor-cpd). Local store (.testaruda/, gitignored) initializes on first
# use; a stale store self-heals (wild-z56).
test:
    #!/usr/bin/env bash
    set -uo pipefail
    if [ ! -d .testaruda ]; then
      testaruda init || { echo "❌ testaruda init failed" >&2; exit 1; }
    fi
    code=0
    tools/gates/testaruda-exec.sh || code=$?
    case "$code" in
      0|10|20) exit 0 ;;
      4)
        echo "⚠️ runner exit 4 — pruning stale local calibration store and retrying once (wild-z56)" >&2
        rm -rf .testaruda
        testaruda init || { echo "❌ testaruda re-init failed" >&2; exit 1; }
        tools/gates/testaruda-exec.sh ;;
      *) exit "$code" ;;
    esac

# Full suite, no selection (plain cargo test — slowest, complete signal).
# CI parity: `just ci` runs this through the full pipeline.
test-full:
    cargo test

# Blast-radius preview: which tests a change WOULD affect — runs nothing.
# Useful before a refactor; optionally name files: just test-blast src/foo.rs
test-blast *files:
    #!/usr/bin/env bash
    if [ -z "{{files}}" ]; then
      testaruda select --pre-edit --human
    else
      testaruda select --pre-edit --human --files "{{files}}"
    fi

# === Lint Commands ===

# Run clippy linter (must match CI)
lint:
    cargo clippy --all-targets -- -D warnings

# Format all Rust files
fmt:
    cargo fmt

# Check formatting (no changes)
fmt-check:
    cargo fmt -- --check

# === Spec-Corpus Gates ===

# Lint the specodelic spec corpus
spk-lint:
    spk lint specs

# Validate spec-test correspondence (espectacular), running mapped tests
ah-check:
    ah check --run-tests

# Full local gate set: cargo pipeline + spec corpus
gates: ci spk-lint ah-check
    @echo "✅ All gates passed"

# === CI Commands ===

# Full CI pipeline (must match .github/workflows/ci.yml)
ci: fmt-check lint test build-release
    @echo "✅ CI pipeline passed"

# Pre-push checks (fast gate)
pre-push: fmt-check lint test
    @echo "✅ Pre-push checks passed"

# === Utility Commands ===

# Clean build artifacts
clean:
    cargo clean

# Check without building (faster feedback)
check:
    cargo check

# Show available commands
help:
    @just --list
