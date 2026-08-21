# Changelog

All notable changes to the `tpt-telos-uir-bridge` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-08-12

Initial release — first publish to crates.io.

### Added

- The **Prover Bridge** for [`tpt-uir`](https://github.com/tpt-solutions/tpt-uir)
  (Phase 4 of that project): consumes a TPT-UIR `Region` (serialized `.tptuir`)
  and formally proves each `tpt_memory` scope's `mem.alloc` totals stay within a
  per-scope physical-memory budget for all symbolic-dimension assignments.
- `extract_allocs`/`extract_symbolic_dims`/`extract_bounded_dims` to resolve
  symbolic byte-size expressions and dimension variables from a region.
- `prove_memory_bounds` -> `ProofResult::Valid` / `Counterexample(model)` /
  `Inconclusive`, plus `prove_tptuir_bytes`/`prove_tptuir_file` for serialized
  `.tptuir` input.
- Reuses tpt-telos' Fourier-Motzkin SMT core by default (sound over integers,
  no external dependency); the `z3` feature routes nonlinear allocation sizes
  (products of symbolic dimensions) through Z3 for exact integer arithmetic.
- Gated behind the `uir` feature (default off) so the default
  `cargo test --workspace` doesn't require a sibling `tpt-uir` checkout; the
  crate compiles to a thin stub without it.
- `telos-uir-prove` standalone CLI (`src/bin/telos-uir-prove.rs`).
