# tpt-pantheon — top-level task runner.
#
# `just` is the single entry point for both the Rust workspace (cargo) and the
# Go Identity service (go). CI invokes the same recipes so local and CI builds
# stay identical.

set dotenv-load := false
fallback := "list"

# List available recipes.
list:
    @just --list

# --- Rust workspace -------------------------------------------------------

# Build every Rust crate in the workspace.
build:
    cargo build --workspace

# Run every Rust test in the workspace.
test:
    cargo test --workspace

# Lint the Rust workspace.
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Format the Rust workspace.
fmt:
    cargo fmt --all

# Check licenses / supply chain (requires `cargo-deny`), plus the §5.3
# sandbox-boundary check (only the wasm-sandbox crate may depend on wasmtime).
deny:
    -cargo deny --all-features check
    pwsh -NoProfile -ExecutionPolicy Bypass -File ci/check-sandbox-boundary.ps1

# --- Go Identity service --------------------------------------------------

# Build the vendored Go Identity service.
build-identity:
    cd services/identity && go build ./...

# Test the vendored Go Identity service.
test-identity:
    cd services/identity && go test ./...

# --- Combined -------------------------------------------------------------

# Build everything (Rust + Go).
build-all: build build-identity

# Test everything (Rust + Go).
test-all: test test-identity

# CI entry point.
ci: fmt lint deny build-all test-all
