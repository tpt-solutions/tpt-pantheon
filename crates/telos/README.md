# tpt-telos

**tpt-telos** is a compiler for the *tpt-telos* language: a formally-specified,
verification-first source language that lowers to both **Rust** and **Go** behind
an automatic FFI bridge.

The pipeline is:

```
parser → IR / constraint extraction → SMT-style verifier → agentic transpiler
       → context router → Rust/Go codegen → FFI bridge → eject hatch → LSP
```

Every `func` carries `requires`/`ensures` contracts that are extracted to
**QF_LRA** linear arithmetic and discharged by a self-contained
Fourier–Motzkin SMT-style solver — **no external Z3 dependency** is required to
build or run.

> **Status:** v0.2.0 — Phases 7–10 complete (see [`TODO.md`](TODO.md)).
> Counterexamples are surfaced by `telos verify`/`build` and the LSP; structs,
> enums, bounded `forall`/aggregates, disjunction, and modular `Call`/
> `MethodCall` verification are all wired into `tpt-telos-ir`/`tpt-telos-codegen`.

## Install

```sh
cargo install tpt-telos
```

Or build from this workspace:

```sh
cargo build --release -p tpt-telos
```

## Usage

```sh
# Scaffold a starter module (templates: simple [default], dual-backend, eject,
# real-time, python-ml, cross-module)
telos init --module MyModule --out my_module.telos
telos init --module Svc --template dual-backend --out svc.telos

# Scaffold a whole project directory (module + README) ready for `telos project`
telos new --name MyProject --template cross-module

# Parse and type/contract-check a .telos file
telos parse  examples/wallet.telos

# Run formal verification (requires/ensures → QF_LRA); prints a counterexample on FAIL
telos verify examples/wallet.telos

# Machine-readable output for CI/editors, or watch the file and re-verify on save
telos verify examples/wallet.telos --json
telos verify examples/wallet.telos --watch

# Exact nonlinear arithmetic via Z3 (requires building with --features z3)
telos verify examples/wallet.telos --solver z3

# Transpile to a single self-contained Rust file
telos transpile examples/wallet.telos --out wallet.rs

# Build a verified, compiling Rust crate (writes telos-proof.json alongside it)
telos build examples/wallet.telos --out-dir ./gen

# Generate a dual Rust+Go project with the FFI bridge
telos project examples/microservice.telos --out-dir ./gen-project --check

# Fail the build if a real_time/zero_allocation module got routed to Go
telos project examples/microservice.telos --check --strict-rt

# Eject a function to a hand-tunable opaque block wrapped by a contract guard
telos eject examples/microservice.telos --func withdraw

# Re-hash source against a previously generated telos-proof.json to detect drift
telos verify-manifest gen/telos-proof.json examples/wallet.telos

# Generate shell completions (bash, zsh, fish, powershell, elvish)
telos completions bash > /etc/bash_completion.d/telos

# Run the language server (JSON-RPC 2.0 over stdio)
telos lsp
```

### LLM-backed agent

By default the agentic transpiler runs the fully offline `StaticAgent`. To use a
real LLM backend, build with the `llm` feature and pass `--llm`:

```sh
cargo run -p tpt-telos --features llm -- transpile examples/intent.telos --llm
```

At runtime it needs `TELAS_LLM_KEY` and `TELAS_LLM_PROVIDER`
(`openai` default | `ollama` | `openrouter` | `grok` | `anthropic`); optionally
`TELAS_LLM_MODEL` / `TELAS_LLM_URL` / `TELAS_LLM_MAX_TOKENS`.

## Crates

| Crate           | Purpose                                                       |
|-----------------|---------------------------------------------------------------|
| `tpt-telos`     | The `telos` binary and CLI surface (incl. `init`/`verify-manifest`, `--json`/`--watch`/`--strict-rt`/`--solver`). |
| `tpt-telos-parser`  | Lexer, parser, and AST.                                       |
| `tpt-telos-ir`      | AST → IR lowering + QF_LRA constraint extraction, disjunction/DNF, bounded `forall`/aggregates, modular `Call`/`MethodCall` verification. |
| `tpt-telos-verifier`| Self-contained Fourier–Motzkin SMT-style solver, plus an optional Z3 backend (`--features z3`) and a gRPC solver cluster. |
| `tpt-telos-router`  | Classifies modules to Rust/Go/Python from `@boundary(...)`, storage class from `@state(...)`, and real-time/Go conflict warnings. |
| `tpt-telos-agent`   | `CodeAgent` trait: `StaticAgent` + `LlmAgent` (behind `llm`). |
| `tpt-telos-codegen` | Rust/Go/Python backends, FFI bridge, eject, project assembly, cryptographic proof manifest. |
| `tpt-telos-lsp`     | JSON-RPC 2.0 language server over stdio (diagnostics, hover, quick-fix code actions, `telos/verify`, `telos/eject`). |
| `tpt-telos-sdk`   | Programmatic orchestration API: one-call `compile`/`compile_static` pipeline, counterexample → hint formatter, and `compile_project` build step for integrators. |
| `tpt-telos-uir-bridge` | The **Prover Bridge** for `tpt-uir` (Phase 4): consumes a TPT-UIR `Region` and formally proves each `tpt_memory` scope's allocations stay within the target hardware's physical-memory budget (FM engine, optional Z3 for nonlinear sizes). |
| `out-telos-wasm`   | WASM bindings over `parser` + `verifier` for the zero-install browser playground. |

## Architecture note: divergence from spec.txt

`spec.txt` sketched an aspirational directory layout with top-level `compiler/`,
`verifier/`, and `ai-orchestrator/` sibling directories, and assumed Z3/CVC5 as the
primary solver, a LangGraph-style orchestration layer, and vLLM for local inference.

What was built instead is a flat **Cargo workspace** under `crates/` with eleven
focused members (parser, ir, verifier, router, agent, codegen, lsp, cli, the
`out-telos-wasm` browser-playground bindings, the `tpt-telos-sdk` orchestration
API, and the `tpt-telos-uir-bridge` prover bridge for `tpt-uir`). The reasons:

- A single workspace makes cross-crate refactoring, CI, and coverage tooling
  straightforward with standard Cargo tooling and no per-directory build wiring.
- The self-contained Fourier–Motzkin solver eliminates the Z3 C-library build
  dependency for the common case; Z3 is available behind `--features z3` for
  exact nonlinear arithmetic when needed.
- `StaticAgent` (fully offline, deterministic synthesis) satisfies the
  Generate → Verify → Counter-example → Rewrite loop without requiring a running
  LLM service; `LlmAgent` behind `--features llm` adds real-LLM support without
  making it a hard dependency.

The `compiler/` / `verifier/` / `ai-orchestrator/` names from spec.txt are
**not** present on disk; the mapping is: parser+ir ≈ compiler front-end,
verifier ≈ verifier, agent+router+codegen ≈ ai-orchestrator.

## Crate documentation

Every crate under `crates/` has its own README with usage details specific to that
crate; the table in the [Crates](#crates) section above is a quick-reference summary,
not a substitute. The source-of-truth for the grammar is
`crates/tpt-telos-parser/src/grammar.ebnf`; for the full feature and phase history see
[`TODO.md`](TODO.md); for example `.telos` files see [`examples/README.md`](examples/README.md).

## Editor integration

A VS Code extension (syntax highlighting + LSP client) lives in
[`vscode-telos/`](vscode-telos/README.md). For Neovim or Helix, see
[`docs/editors.md`](docs/editors.md) for `nvim-lspconfig` / `languages.toml` setup against
the same `telos lsp` server — see [`tpt-telos-lsp`](crates/tpt-telos-lsp/README.md) for what
the server itself provides (diagnostics, hover, definition/references, completion,
formatting, quick-fixes).

## Troubleshooting

**`gofmt: command not found` / `go: command not found`**
`telos project --check` and `telos eject` shell out to `go build` and `gofmt` to
compile and canonicalize generated Go. If Go is not on your `PATH`, these commands
fall back to a warning rather than a hard failure; the generated source is still
written to disk. Install Go ≥ 1.21 and ensure `$GOPATH/bin` (or the Go install
`bin/`) is on your `PATH`. Run `telos doctor` to check which optional tools
(`go`, `gofmt`, `z3`) are currently available on `PATH` before running a command
that needs them.

**`--features z3` fails to build**
The `z3` Cargo feature links against the Z3 C library via `z3-sys`, which requires
`z3.h` on the compiler's include path and `libz3` at link time. Z3 is not vendored.
Install Z3 development headers (e.g. `apt install libz3-dev` or `brew install z3`)
before building with `--features z3`. The default feature set builds without any
external C dependencies.

**`cargo llvm-cov` not found**
Coverage reporting requires `cargo-llvm-cov`. Install it with:
```sh
cargo install cargo-llvm-cov
```

## Documentation

- **[Language Guide](docs/LANGUAGE.md)** — syntax, types, contracts, and every supported feature.
- **[Getting Started Tutorial](docs/TUTORIAL.md)** — from install to a verified, compiling artifact.
- **[CLI Reference](docs/CLI.md)** — every `telos` subcommand, flag, and exit code.
- **[SDK Integration Guide](docs/SDK.md)** — embed tpt-telos in your own Rust tooling via `tpt-telos-sdk`.

For example `.telos` files, see [`examples/README.md`](examples/README.md).

## License

Licensed under either of:

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual-licensed as above, without any additional terms or conditions.

## Using the SDK

If you are building an integration (CI gate, orchestration service, sibling repo
like `tpt-nexus`), depend on `tpt-telos-sdk` instead of the six pipeline crates
directly. It exposes a one-call pipeline plus hint formatting and a build step:

```rust
use tpt_telos_sdk::{compile, format_outcome_hints, StaticAgent};

let artifact = compile(source, &StaticAgent::new())?;
// artifact.all_verified, artifact.outcomes, artifact.project, artifact.manifest
let hints = format_outcome_hints(&artifact.outcomes); // counterexample -> text
```

Verification *failure* is **not** an error — it is reported via
`VerifiedArtifact::all_verified`. `SdkError` is only for pipeline failures (parse,
transpile, codegen). See `crates/tpt-telos-sdk/examples/sdk_usage.rs` for a runnable
example (`cargo run -p tpt-telos-sdk --example sdk_usage`).

## TPT-UIR Prover Bridge

`tpt-telos-uir-bridge` (Phase 4 of [`tpt-uir`](https://github.com/tpt-solutions/tpt-uir))
formally proves that a model's memory allocations fit the target hardware. The flow:

1. An ingestion adapter in `tpt-gpu` / `tpt-crucible` lowers its model to a TPT-UIR
   `Region` and writes it to a `.tptuir` file (postcard). The `tpt-uir-dialects`
   liveness pass has already wrapped each alloc-bearing operation with
   `tpt_memory.scope_begin` / `tpt_memory.alloc` / `tpt_memory.scope_end`, so every
   allocation carries a `scope` and (via a `tensor`/`type` attribute, or a block
   argument) a `TensorType` describing its size.
2. The bridge walks the region and extracts each `mem.alloc`'s byte size as a
   symbolic expression over the region's `Dimension::Symbolic` variables, then
   proves per scope that `sum(alloc sizes) <= budget` for **all** dimension
   assignments. The negation (`sum > budget`) being *unsatisfiable* means the scope
   is safe; if it is *satisfiable*, the solver returns a concrete witness
   (`ProofResult::Counterexample`) showing the offending dimension values.

```rust
use tpt_telos_uir_bridge::{prove_tptuir_file, MemoryLimits, ProofResult};

// Prove that every scope in `model.tptuir` fits a 1 MiB budget, with the
// "weights" scope capped at 256 KiB.
let limits = MemoryLimits::with_default(1024 * 1024).with_scope("weights", 256 * 1024);
match prove_tptuir_file("model.tptuir", &limits).unwrap() {
    ProofResult::Valid => println!("memory budget proven safe"),
    ProofResult::Counterexample { scope, model, .. } => {
        println!("scope {scope} overflows; witness: {model:?}")
    }
    ProofResult::Inconclusive { reason } => println!("needs Z3: {reason}"),
}
```

The standalone CLI mirrors this:

```sh
# Default 1 MiB budget; "weights" scope capped at 256 KiB.
telos-uir-prove model.tptuir --default-limit 1048576 --scope weights 262144
# exit 0 = valid, 1 = counterexample (over budget), 2 = inconclusive / error
```

The default engine is tpt-telos' built-in Fourier-Motzkin SMT core (sound over
integers, no external dependency) and handles linear allocation sizes (fixed
dims, or a single symbolic dim per tensor). Build with `--features uir,z3` to route
nonlinear sizes (e.g. a tensor with two symbolic dimensions) through the Z3 solver
for exact integer arithmetic. The crate is gated behind the `uir` feature so the
default `cargo test --workspace` needs no sibling `tpt-uir` checkout; build and test
it with `cargo test -p tpt-telos-uir-bridge --features uir`.
