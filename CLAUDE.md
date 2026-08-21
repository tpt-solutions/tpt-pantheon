# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

tpt-telos is an intent-based, constraint-declarative language and compiler. Developers write `.telos`
source (module/invariant/func with `requires`/`ensures` contracts); the pipeline parses it, extracts
mathematical constraints, verifies them with a self-contained SMT-style solver, and uses an agentic
"Generate -> Verify -> Counter-example -> Rewrite" loop to synthesize compiling Rust and/or Go code for
each function, routed by `@boundary(...)` metadata. The full pipeline (all 4 planned phases) is
implemented; see `TODO.md` for what each phase built and `spec.txt` for the original design doc (note:
`spec.txt`'s aspirational directory layout — `compiler/`, `verifier/`, `ai-orchestrator/` — was **not**
what was built; the actual layout is the Cargo workspace under `crates/` described below).

## Commands

```
cargo build --workspace                       # build everything
cargo test --workspace                        # run all unit + integration tests
cargo test -p tpt-telos-verifier                  # test a single crate
cargo test -p tpt-telos-verifier extended_tests    # run a specific test module/fn (cargo test filters by name)
cargo test --workspace --features llm         # include the LLM-agent feature in the build/tests
cargo fmt --all -- --check                    # CI formatting check
cargo clippy --workspace --all-targets -- -D warnings   # CI lint check (matches RUSTFLAGS=-D warnings in CI)
cargo llvm-cov --workspace --fail-under-lines 75         # CI coverage gate (75% floor; don't lower it)
```

Running the compiled CLI directly (subcommands: `init | parse | verify | transpile | build | project
| eject | verify-manifest | lsp`):

```
cargo run -p tpt-telos -- init --module MyModule --out my_module.telos
cargo run -p tpt-telos -- verify examples/wallet.telos
cargo run -p tpt-telos -- verify examples/wallet.telos --json      # machine-readable output for CI/editors
cargo run -p tpt-telos -- verify examples/wallet.telos --watch     # poll the file and re-verify on change
cargo run -p tpt-telos -- verify examples/wallet.telos --solver z3 # requires --features z3 + Z3 on PATH
cargo run -p tpt-telos -- build examples/wallet.telos --out-dir gen
cargo run -p tpt-telos -- project examples/microservice.telos --out-dir gen-project --check
cargo run -p tpt-telos -- project examples/microservice.telos --check --strict-rt  # non-zero exit on real_time/Go conflict
cargo run -p tpt-telos -- eject examples/eject.telos --func withdraw
cargo run -p tpt-telos -- verify-manifest gen/telos-proof.json examples/wallet.telos  # detect source drift
cargo run -p tpt-telos -- transpile examples/wallet.telos --llm   # requires --features llm and TELAS_LLM_* env vars
```

`telos project --check` shells out to `cargo` and `go`; `eject`/`project` also invoke `gofmt` to
canonicalize generated Go (falls back to a warning, not a failure, if `gofmt`/`go` aren't on PATH).
`build`/`project` also write a `telos-proof.json` manifest (SHA-256 of source + per-function
verification outcomes + tamper-evident `manifest_hash`); `verify-manifest` re-hashes source against
a saved manifest to detect drift.

CI (`.github/workflows/ci.yml`) runs three jobs: format+clippy+test, a `feature-matrix` job building/
testing/linting `--features llm` and `--features z3` separately, and a `cargo-llvm-cov` coverage job
with an enforced 75% line-coverage floor.

## Workspace layout

Eleven crates under `crates/` (currently version **0.2.0**, see `Cargo.toml` `[workspace.package]`), each with a focused responsibility in the pipeline:

- **tpt-telos-parser** — hand-written lexer/parser/AST for `.telos` source. Grammar is the source of truth
  at `crates/tpt-telos-parser/src/grammar.ebnf`; keep it in sync with `lexer.rs`/`parser.rs`/`ast.rs` when
  the language changes. The AST also covers generics, tuples, `struct`/`enum` definitions, calls,
  and control-flow expressions (`if`/`match`/`forall`/aggregates/`?`) — as of Phase 7 these are fully
  lowered by `tpt-telos-ir`/`tpt-telos-codegen` too, not just parsed (see Phase 7 in `TODO.md`).
- **tpt-telos-ir** (`extract.rs`) — lowers the AST into `VerificationProblem`s, translating `requires`/
  `ensures`/invariants into a linear-arithmetic constraint model (QF_LRA-ish). Also handles disjunction
  (`requires a || b`) via DNF expansion into independent branches, bounded `forall`/aggregate unrolling
  over constant-bound ranges, general nested `if`/`match` as arithmetic sub-expressions, modular
  (Dafny-style) verification of `Call`/`MethodCall` by substituting callee `ensures` as premises
  (rejecting recursive call cycles), and constant-index array/slice access. Runs a cross-module type
  resolution pass first, so one module's invariant types can appear in another's function signatures.
- **tpt-telos-verifier** (`solver.rs`, `verify.rs`) — a self-contained Fourier-Motzkin-based SMT-style
  solver (no external Z3/CVC5 dependency required). Sound over integers. Produces pass/fail plus, on
  failure, a concrete counter-example `Model` used to drive agent rewrites and surfaced by the CLI/LSP.
  Behind the `z3` feature, `set_solver_backend(SolverBackend::Z3)` routes verification through Z3
  instead for exact nonlinear arithmetic; `cluster.rs` supports gRPC dispatch of `VerificationProblem`s
  to a pool of solver workers for CI-scale verification.
- **tpt-telos-router** — pure classification: reads `@boundary(...)` attributes and decides `Target::Rust`
  vs `Target::Go` vs `Target::Python` per module/function (`cpu_bound`/`zero_allocation`/`crypto`/
  `real_time` -> Rust; `network_io`/`high_concurrency`/`distributed`/`high_latency` -> Go;
  `ml_training`/`python`/`jax` -> Python, which beats everything else). Also reads `@state(persistent|
  ephemeral)` into a `StorageClass`, and `route_checked` emits a `RoutingDiagnostic` (`real_time`/
  `zero_allocation` routed to Go's non-deterministic GC) that the CLI's `--strict-rt` turns into a
  non-zero exit.
- **tpt-telos-agent** — the `CodeAgent` trait plus the Generate -> Verify -> Counter-example -> Rewrite
  loop (`transpile_module`). `StaticAgent` (`static_agent.rs`) is the default, fully offline,
  deterministic synthesizer (translates the developer's body when present, else derives one from
  `ensures`, including case-split `if`/`match`, bounded loops, and direct calls). `llm_agent.rs`
  (behind the `llm` Cargo feature) calls a real LLM over an OpenAI-compatible or native Anthropic wire
  format — see env vars below; its `pretty`/`pretty_stmt` AST pretty-printers must stay exhaustive over
  every `Expr`/`Stmt` variant or `--features llm` fails to compile under `-D warnings`.
- **tpt-telos-codegen** — lowers verified `Candidate`s into target source: `lib.rs`/`eject.rs` for the Rust
  backend (structs, invariant `impl` methods, contracts as doc comments), `go.rs` for the Go backend
  (mirrors the Rust one), `python.rs` for `@boundary(ml_training|python|jax)` modules (`@dataclass`
  structs + runtime `assert` guards, `jnp.int64` annotations under `jax`), `ffi.rs` for the
  bidirectional C-ABI FFI bridge between Rust and Go, `proof.rs` for the `telos-proof.json` manifest
  (generate + verify), and `project.rs` to assemble a full buildable project tree (`Cargo.toml` +
  `go.mod` + FFI glue) routed per-module via `tpt-telos-router`.
- **tpt-telos-lsp** — dependency-light JSON-RPC 2.0 LSP server over stdio (`Content-Length` framing):
  diagnostics (with counterexamples), hover, `textDocument/codeAction` (quick-fix `requires !(...)`
  suggestions derived from a failing check's counterexample), and custom `telos/verify` / `telos/eject`
  requests. Message handling (`analysis.rs`) is decoupled from stdio I/O for unit testing.
- **tpt-telos** — the `telos` binary (clap). Thin orchestration layer over the crates above; also
  contains the AST pretty-printer used by `telos parse` and the `init` scaffold / `verify-manifest`
  drift-check commands.
- **tpt-telos-sdk** — programmatic orchestration API for integration harnesses (e.g. `tpt-nexus`).
  Wraps the six crates above into one-call pipeline entry points (`compile`/`compile_static`),
   a counterexample -> hint formatter (`format_hint`/`format_outcome_hints`), a generic
   `.telos`-source-independent [`check_contradictions`] entry point over just the solver core
   (plus a hand-rolled JSON reader and the `telos-prove` standalone binary for callers who want
   to contradiction-check named constraint groups without a library dependency), and a build step
    (`compile_project`/`compile_project_tempdir`) that reads back compiled artifact bytes. See
    `crates/tpt-telos-sdk/README.md`.
- **tpt-telos-uir-bridge** — the **Prover Bridge** for `tpt-uir` (Phase 4 of that repo). Consumes a
  TPT-UIR `Region` (serialized `.tptuir`) and formally proves each `tpt_memory` scope's
  `mem.alloc` totals stay within a per-scope physical-memory budget for all symbolic-dimension
  assignments. Extraction (`extract_allocs` / `extract_symbolic_dims`) plus the proof
  (`prove_memory_bounds` → `ProofResult::Valid` / `Counterexample(model)` / `Inconclusive`) reuse
  tpt-telos' Fourier-Motzkin SMT core; the `z3` feature routes nonlinear allocation sizes (products
  of symbolic dimensions) through the Z3 solver. Gated behind the `uir` feature so the default build
  needs no sibling `tpt-uir` checkout; `src/bin/telos-uir-prove.rs` is the standalone CLI.

### Pipeline data flow

`.telos` source -> `telos_parser::parse` (`Vec<Module>`) -> `telos_ir::extract` (`VerificationProblem`s)
-> `telos_verifier::verify` (pass/fail + counterexample) -> `telos_agent::transpile_module` (runs the
verify/rewrite loop per function, using `telos_router::route` for target selection) -> `telos_codegen::
generate_program` (single Rust output) or `generate_project` (dual-backend tree with FFI) ->
optional `cargo build` / `go build` / `gofmt` invocation by the CLI.

### Key language semantics (see `grammar.ebnf` for full grammar)

- `@boundary(...)` / `@state(...)` are architectural metadata attributes on modules/functions.
- `invariant T { c }` must hold at function entry and after every `mutate state` block.
- `requires`/`ensures` are pre-/post-conditions; `old(e)` refers to the pre-state value of `e`.
- `@eject` marks a function to compile to a trusted opaque block (`f_impl`/`fImpl`) wrapped by a
  generated guard function that still enforces `requires`/`ensures` at runtime via `assert!`/`panic`.
- No implicit type coercion, no hidden allocation — every operation is explicitly named (by design,
  to keep the language easy for both humans and the LLM agent to reason about).

### Historical archives (do not clean up)

The `history/TODO 1260723.md` and `history/TODO 1260713.md` files are **intentional
historical snapshots** of an older TODO (they reference pre-rename crate paths like
`crates/telos-parser`). They are NOT the source of truth — `TODO.md` at the repo root is.
Do not delete, move, or "tidy" these files; leave them in place.

- The LSP's `textDocument/codeAction` quick-fix only ever suggests excluding the exact counterexample
  witness (`requires !(v1 == a && v2 == b && ...)`) — it's a starting point for the developer to
  refine, not a guaranteed or general fix.
- `--features z3` requires Z3's headers/library available at build time (`z3-sys`'s build script
  needs `z3.h` on the include path); it is not vendored, so this feature will fail to build on a
  machine without Z3 installed even though the default feature set is unaffected.
- Non-constant/symbolic array and slice indices are still rejected in contracts (only compile-time-
  constant indices are unrolled); full array theory remains out of scope.
- `TODO.md`'s Phase `[x]` marks have been spot-checked once (see its "Re-audit Phase 6's `[x]` claims"
  entry) but treat any *new* `[x]` mark with the same skepticism until verified against the code.

### LLM agent environment variables (only relevant behind the `llm` feature)

`TELAS_LLM_PROVIDER` (`openai` default, or `ollama`/`openrouter`/`grok`/`anthropic`), `TELAS_LLM_URL`,
`TELAS_LLM_KEY` (required), `TELAS_LLM_MODEL`, `TELAS_LLM_MAX_TOKENS` (Anthropic only).

## Testing conventions

Every crate has unit tests colocated in `src/` plus at least one integration test under `tests/`.
`tpt-telos/tests/cli.rs` drives the actual binary end-to-end. `examples/*.telos` are fixtures used by
integration tests (`wallet.telos`/`broken.telos` for pass/fail verification, `nested.telos` for nested
struct fields, `microservice.telos` for dual-backend + FFI, `eject.telos` for the eject hatch). When
fixing a bug, add a regression fixture under `examples/` and wire it into an existing or new
integration test, matching the existing pattern.
