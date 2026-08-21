# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-08-12

### Added
- `tpt-telos-sdk` orchestration API and `out-telos-wasm` browser-playground bindings
  (released as part of 0.1.1) are now versioned together with the workspace at 0.2.0.
- (Phase 7–10 features consolidated: arrow/intent syntax, nested struct fields,
  `project` dual-backend assembly with Python/JAX backend, LSP quick-fix code
  actions, and the `verify-manifest` drift check.)
- `tpt-telos-sdk`: `telos-prove` standalone CLI for constraint-group
  contradiction checking, plus a `.telos`-source-independent
  `check_contradictions` entry point over the solver core.
- New crate `tpt-telos-uir-bridge`: the Prover Bridge for `tpt-uir` (Phase 4 of
  that project), proving `tpt_memory` allocation totals stay within a per-scope
  physical-memory budget for all symbolic-dimension assignments. First release,
  gated behind the `uir` feature.
- `telos fmt`/`telos doctor`/`telos new` CLI commands; LSP `textDocument/
  formatting`/`definition`/`references`/`completion`/`inlayHint` backed by a
  workspace-wide symbol index; three new `init --template` options
  (`real-time`, `python-ml`, `cross-module`).
- User-facing docs: `docs/LANGUAGE.md`, `docs/CLI.md`, `docs/SDK.md`.

### Changed
- All crates bumped to `0.2.0`; intra-workspace dependency pins updated accordingly.
- Documentation pass aligning README, AGENTS.md, CLAUDE.md, and CONTRIBUTING.md with
  the 0.2.0 release and an issues-only contribution model.

### Fixed
- **Soundness:** `if`/`else` and `match` contract lowering (`tpt-telos-ir`) now
  correctly guards each branch with its own premise, instead of accepting
  every branch unconditionally.
- **Soundness:** the Fourier-Motzkin solver (`tpt-telos-verifier`) uses checked
  `i128` arithmetic and never returns a spurious contradiction on overflow.
- FFI codegen rejects non-integer types crossing an FFI boundary instead of
  silently coercing them to `i64`.

[0.2.0]: https://github.com/tpt-solutions/tpt-telos/releases/tag/v0.2.0

## [0.1.1] - 2026-08-08

### Added
- `tpt-telos-sdk`: programmatic orchestration API (`compile`/`compile_static`,
  counterexample → hint formatter, `compile_project` build step).
- `out-telos-wasm`: WASM bindings over `parser` + `verifier` for a browser playground.

### Changed
- Documentation consistency pass: crate counts (10 members) and version labels
  aligned across README, ARCHITECTURE, CONTRIBUTING, and the grammar reference.

[0.1.1]: https://github.com/tpt-solutions/tpt-telos/releases/tag/v0.1.1

## [0.1.0] - 2026-07-14

### Added
- Initial public release of the tpt-telos compiler workspace.
- `tpt-telos-parser`: hand-written lexer, parser, and AST for the tpt-telos language.
- `tpt-telos-ir`: AST → IR lowering and `requires`/`ensures` → QF_LRA constraint extraction.
- `tpt-telos-verifier`: self-contained Fourier–Motzkin SMT-style solver (sound over
  integers, no external Z3 dependency) with `model()` / `counterexample()` support.
- `tpt-telos-router`: classifies modules to Rust/Go backends from `@boundary(...)`.
- `tpt-telos-agent`: `CodeAgent` trait with the offline `StaticAgent` and an `LlmAgent`
  behind the `llm` feature (OpenAI-compatible + native Anthropic providers).
- `tpt-telos-codegen`: dual Rust/Go backends, automatic FFI bridge, eject hatch, and
  project assembly.
- `tpt-telos-lsp`: JSON-RPC 2.0 language server over stdio (diagnostics, hover,
  `telos/verify`, `telos/eject`).
- `tpt-telos`: the `telos` binary exposing `parse`, `verify`, `transpile`,
  `build`, `project`, `eject`, and `lsp`.

[0.1.0]: https://github.com/tpt-solutions/tpt-telos/releases/tag/v0.1.0
