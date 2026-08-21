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

> **Status:** v1.2, Phase 7 and Phase 8 complete (see [`TODO.md`](TODO.md)).
> Counterexamples are surfaced by `telos verify`/`build` and the LSP; structs,
> enums, bounded `forall`/aggregates, disjunction, and modular `Call`/
> `MethodCall` verification are all wired into `telos-ir`/`telos-codegen`.

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
# Scaffold a starter module
telos init --module MyModule --out my_module.telos

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

## License

Licensed under either of:

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual-licensed as above, without any additional terms or conditions.
