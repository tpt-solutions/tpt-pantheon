# Changelog

All notable changes to the `tpt-telos-sdk` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-08-12

Initial release — first publish to crates.io.

### Added

- One-call pipeline (`compile`/`compile_static`): wraps
  parse -> `transpile_module` per module -> `generate_project` -> `generate_manifest`
  into a single `VerifiedArtifact { source, modules, outcomes, project, manifest,
  all_verified }`; `Err` only on a pipeline failure (parse/agent/codegen), not on
  verification failure.
- `SdkError` with `Parse`/`Transpile`/`Codegen`/`Io`/`ToolNotFound` variants.
- Counterexample hint formatter (`format_hint`/`format_outcome_hints`): renders a
  `CheckResult`'s clause kind, disjunction/approximation caveats, and sorted
  `Model` counterexample bindings into human/LLM-readable text.
- Build/compile-to-artifact-bytes (`compile_project`/`compile_project_tempdir`):
  writes a `Project` to disk, shells out to `cargo build`/`go build` per backend,
  and reads back the Rust rlib/staticlib bytes.
- `check_contradictions`/`unsat_checked`/`model` — a `.telos`-source-independent
  entry point over just the solver core, for callers with named constraint groups
  and no `.telos` file.
- `telos-prove` standalone binary: a thin, dependency-free CLI (`--json`/
  `--strict`, file-or-stdin) over `check_contradictions`, with a hand-rolled JSON
  reader (`json.rs`, no serde) and a constraint-group report (`prove.rs`).
- Flat re-exports (`FuncOutcome`, `CodeAgent`, `Model`, `Target`, `ProofManifest`,
  `StaticAgent`, `#[cfg(feature = "llm")] LlmAgent`, etc.) so consumers don't need
  all six wrapped crates as direct dependencies.
- `llm`/`z3` passthrough features.
