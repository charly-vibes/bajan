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

# Run all tests
test:
    cargo test

# Run tests with output
test-verbose:
    cargo test -- --nocapture

# Run a specific test
test-one name:
    cargo test {{name}} -- --nocapture

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
