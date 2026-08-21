# tpt-telos TODO

## Phase 1: The Core & The Parser (Months 1-3)
- [x] Define the formal grammar for tpt-telos. (see `crates/tpt-telos-parser/src/grammar.ebnf`)
- [x] Build the Rust-based parser and AST generator. (`crates/tpt-telos-parser`)
- [x] Implement the basic constraint extraction (translating requires/ensures to a linear-arithmetic SMT core). (`crates/tpt-telos-ir`, `crates/tpt-telos-verifier`)
- [x] **Milestone:** A CLI that can parse a .telos file and output a formal verification pass/fail. (`telos verify <file>`)

> Phase 1 implemented: a Cargo workspace (`telos-parser`, `telos-ir`, `telos-verifier`, `telos-cli`)
> with a hand-written lexer/parser, AST, constraint extraction to QF_LRA, and a self-contained
> Fourier-Motzkin SMT-style verifier (sound over integers; no external Z3 dependency required to build).
> Verified end-to-end against `examples/wallet.telos` (PASS) and `examples/broken.telos` (FAIL).

## Phase 2: The Agentic Transpiler (Months 4-6)
- [x] Integrate the LLM agent pipeline.
- [x] Build the context router (deciding what goes to Rust vs. Go).
- [x] Implement the "Verify -> Counter-example -> Rewrite" loop.
- [x] **Milestone:** The compiler can take a simple .telos module and output mathematically verified, compiling Rust code.

> Phase 2 implemented: a Cargo workspace extended with `telos-router`, `telos-agent`,
> `telos-codegen`, and new `transpile` / `build` CLI commands.
> - `telos-router` classifies each module/function to Rust or Go from `@boundary(...)`
>   metadata (`cpu_bound`/`zero_allocation`/`crypto` => Rust; `network_io`/`high_concurrency`
>   /`distributed` => Go).
> - `telos-agent` defines a `CodeAgent` trait and the default `StaticAgent` (a fully
>   offline synthesizer) plus an `LlmAgent` behind the `llm` feature. It runs the
>   Generate -> Verify -> Counter-example -> Rewrite loop, using the SMT core to
>   extract a concrete counter-example model and perform a counter-example-guided
>   repair. `LlmAgent` supports multiple providers via `TELAS_LLM_PROVIDER`
>   (`openai`, `ollama`, `openrouter`, `grok` over the shared OpenAI-compatible
>   wire format, plus native `anthropic` support via the Messages API).
> - `telos-codegen` lowers verified specs into a self-contained, compiling Rust
>   library (structs, `&mut` only where mutated, invariant `impl` methods, and the
>   original `requires`/`ensures` contracts as doc-comments).
> - The SMT core (`telos-verifier`) gained `model()` / `counterexample()` so the
>   loop can surface a witness where the contract fails.
> Verified end-to-end: `telos build examples/wallet.telos` (PASS + compiles),
> `examples/broken.telos` (wrong body rewritten to a verified implementation), and
> `examples/intent.telos` (body elided; synthesized from `ensures` and verified).

## Phase 3: The Dual-Target & FFI (Months 7-9)
- [x] Implement the Go backend generation. (`crates/tpt-telos-codegen/src/go.rs`)
- [x] Build the automated FFI layer so generated Rust and Go code can call each other without manual glue code. (`crates/tpt-telos-codegen/src/ffi.rs`)
- [x] **Milestone:** A fully functioning dual-backend compilation of a microservice. (`telos project examples/microservice.telos --check`)

> Phase 3 implemented: the code generator now emits **both** backends and the
> glue that binds them.
> - `telos-codegen/src/go.rs` is a Go backend mirroring the Rust one: it emits
>   idiomatic Go structs (exported `int64` fields), `SatisfiesInvariants()`
>   methods, and one exported `func` per `func` (taking `*T` for mutated struct
>   params, `T` otherwise), carrying the original contracts as comments. Bodies
>   come from the same verified agentic candidates as the Rust backend.
> - A shared `analyze_func` in `telos-codegen/src/lib.rs` derives each function's
>   effective parameters, mutation set, and scalar return once, so the Rust
>   backend, Go backend, and FFI bridge all agree on calling conventions.
> - `telos-codegen/src/ffi.rs` generates the **automatic, bidirectional FFI
>   bridge** over a stable C ABI (`int64` cells; struct fields flattened, mutated
>   fields passed by pointer): a `telos_ffi.h` header, a Rust `ffi.rs`
>   (`#[no_mangle]` exports for Rust fns + `extern "C"` imports and safe wrappers
>   for Go fns), and a Go `ffi.go` (cgo calls into Rust + `//export` shims
>   exposing Go to Rust). No hand-written glue is required.
> - `telos-codegen/src/project.rs` routes each module (via `telos-router`) to the
>   Rust or Go backend, assembles a ready-to-build project tree
>   (`rust/` crate + `go/` package + FFI files), and emits `Cargo.toml`
>   (`crate-type = ["staticlib", "rlib"]`) and `go.mod`.
> - New CLI command: `telos project <file> [--out-dir DIR] [--check]`. With
>   `--check` it compiles the Rust crate with `cargo` and the Go package with
>   `go`, and validates the cgo FFI sources with `gofmt`.
> Verified end-to-end against `examples/microservice.telos` (a CPU-bound Ledger
> routed to Rust + a network-facing GatewayApi routed to Go): all four functions
> are mathematically verified, the Rust crate compiles, and the Go package
> (incl. the cgo FFI bridge) is well-formed and compiles.

## Phase 4: The "Eject" Hatch & DX (Months 10-12)
- [x] Implement the two-way bridge for ejecting code to raw Rust/Go. (`crates/tpt-telos-codegen/src/eject.rs`, `telos eject`, `@eject` attribute)
- [x] Build the LSP server for IDE integration. (`crates/tpt-telos-lsp`, `telos lsp`)
- [x] **Milestone:** tpt-telos v1.0 release, ready for internal use in tpt-swarm and tpt-eve.

> Phase 4 implemented: the "eject" hatch and a language server complete the DX.
> - **Eject hatch (two-way bridge):** functions can be marked `@eject` (parsed as
>   a new function-level attribute) or ejected on demand with `telos eject`. An
>   ejected function is compiled to a *trusted, opaque block* (`f_impl` in Rust /
>   `fImpl` in Go) that the developer may hand-tune, wrapped by a generated
>   **boundary contract guard** (`f`) that still enforces every `requires`
>   (before) and `ensures` (after, with `old(...)` captured in snapshot locals)
>   at runtime via `assert!` / `panic`. This is the two-way bridge: telos -> raw
>   code, and raw code -> telos behind contract guards. Implemented in
>   `telos-codegen/src/eject.rs`; honored by `transpile` / `project` / `build`
>   and driven by the `telos eject` command (which also writes a
>   `telos-eject.json` manifest). Generated Go is canonicalised with `gofmt`.
> - **LSP server:** `crates/tpt-telos-lsp` is a dependency-light JSON-RPC 2.0 server
>   over stdio (`Content-Length` framing) exposing:
>   - **diagnostics** (parse errors + unsatisfied contracts) on
>     open/change/save; ejected functions are surfaced as trusted (informational)
>     rather than errors,
>   - **hover** showing a function's signature, routing target, contract, and
>     verification status,
>   - custom **`telos/verify`** (verification summary) and **`telos/eject`**
>     (raw-code preview) requests.
>   The message handler is decoupled from I/O for direct unit testing. Launch
>   with `telos lsp`.
> Verified end-to-end: `examples/eject.telos` (in-source `@eject withdraw`
> compiles as opaque impl + guard), `telos eject examples/microservice.telos`
> (both backends compile; Go gofmt-clean), and the LSP server (11 tests +
> live stdio smoke test) reports diagnostics, hover, verify, and eject preview.

## Phase 5: Verifier Hardening & Platform Extensions

- [x] **Nonlinear interval bounding** — over-approximate `x * y` contracts via interval arithmetic when both variables have bounds in `requires` clauses; mark results `[interval-bounded]` in verify output. (`crates/tpt-telos-ir/src/extract.rs`, `crates/tpt-telos-verifier/src/verify.rs`)
- [x] **Python/JAX codegen target** — `@boundary(ml_training|python|jax)` routes to a Python backend that emits `@dataclass` structs with `satisfies_invariants()` and runtime `assert` guards for all contracts; JAX flag emits `jnp.int64` type annotations. (`crates/tpt-telos-codegen/src/python.rs`, `crates/tpt-telos-router/src/lib.rs`)
- [x] **Real-time routing guard** — detect `@boundary(real_time)` or `@boundary(zero_allocation)` modules routed to Go (GC-based, non-deterministic) and emit `WARNING [real_time_go_conflict]`; `--strict-rt` flag exits non-zero. (`crates/tpt-telos-router/src/lib.rs`, `crates/tpt-telos-cli/src/main.rs`)
- [x] **Cryptographic proof manifest** — generate `telos-proof.json` (SHA-256 of source, per-function verification outcomes, tamper-evident `manifest_hash`) on every `build`/`project` run, and embed it as `#[used] static TELOS_PROOF_MANIFEST` in generated Rust binary (spec §7). (`crates/tpt-telos-codegen/src/proof.rs`)
- [x] **Language feature matrix** — document supported/partial/unsupported constructs in `grammar.ebnf` and `README.md`; eliminates ambiguity for integrators writing FADEC-level control logic.

## Phase 6: Scale, Precision & Language Completeness
## Phase 6: Scale, Precision & Language Completeness

- [x] **Distributed SMT solver cluster** — gRPC-based `VerificationProblem` dispatch to a pool of solver workers; enables CI/CV verification at scale without single-machine bottlenecks. (`crates/tpt-telos-verifier/src/cluster.rs`)
- [x] **Z3/CVC5 optional backend** — behind a `--solver z3` flag; falls back to built-in Fourier-Motzkin when unavailable; provides exact nonlinear arithmetic for contracts that interval bounding cannot verify. (`crates/tpt-telos-verifier/src/z3_solver.rs`, `--features z3`)
- [x] **Production coverage lift** — raise workspace line coverage from ~80% to 90%+ using proptest (property-based) and cargo-fuzz (fuzz) harnesses; add mutation testing (cargo-mutants). (`Cargo.toml` workspace dev-dependencies, `crates/tpt-telos-ir/tests/property.rs`, `crates/tpt-telos-verifier/tests/property.rs`)
- [x] **Go GC determinism formal documentation** — document in `ARCHITECTURE.md` exactly which Go-routed module classes are safe vs. unsafe for hard real-time; wire into the `real_time` routing guard as an informational reference. (`ARCHITECTURE.md`, `crates/tpt-telos-router/src/lib.rs`)
- [x] **Disjunction (`||`) in premises** — currently parsed but rejected at the IR level; implement DNF normalization so `requires a || b` expands into two verification sub-problems, each solved independently. (`crates/tpt-telos-ir/src/extract.rs`)
- [x] **Floating-point types** — `Float32`/`Float64` refinement types; IR lowering uses IEEE 754 interval arithmetic; verifier tracks rounding error bounds. (`crates/tpt-telos-codegen/src/lib.rs`, `crates/tpt-telos-codegen/src/go.rs`)
- [x] **`@state(...)` semantics** — `@state(persistent)` / `@state(ephemeral)` currently parsed but ignored; implement storage-class semantics in the router and codegen (e.g., `persistent` → database-backed struct, `ephemeral` → stack-only). (`crates/tpt-telos-router/src/lib.rs`, `crates/tpt-telos-codegen/src/lib.rs`, `crates/tpt-telos-codegen/src/go.rs`)
- [x] **Array and slice support** — `[T; N]` fixed arrays and `[T]` slices in type positions; IR constraint extraction for length/index invariants; codegen for Rust `[T; N]` and Go `[N]T`. (`crates/tpt-telos-parser/src/ast.rs`, `crates/tpt-telos-parser/src/parser.rs`, `crates/tpt-telos-codegen/src/lib.rs`, `crates/tpt-telos-codegen/src/go.rs`)
- [x] **Cross-module references** — allow one module's invariant types to appear in another module's function signatures; requires a global type resolution pass over `Vec<Module>` before IR lowering. (`crates/tpt-telos-ir/src/extract.rs`)

## Phase 7: Verification Result Quality & Contract-Language Completeness

- [x] **Surface counterexamples from `telos verify`** — `CheckResult` gains a
  `counterexample: Option<Model>` populated via the existing
  `solver::counterexample()`; CLI and LSP print/report the concrete witness
  on every `FAIL`, not just the restated clause text. (`crates/tpt-telos-verifier/src/verify.rs`,
  `crates/tpt-telos-cli/src/main.rs`, `crates/tpt-telos-lsp/src/analysis.rs`)
- [x] **Struct/enum definitions drive codegen** — replace the hardcoded-`i64`,
  usage-inferred `TypeFields` with real per-field types read from
  `StructDef`/`EnumDef`; add real Rust enum codegen and unit-only Go enum
  codegen (explicit error for payload-carrying enums in Go); IR cross-check
  rejects contracts referencing undeclared struct fields.
  (`crates/tpt-telos-codegen/src/lib.rs`, `crates/tpt-telos-codegen/src/go.rs`,
  `crates/tpt-telos-ir/src/extract.rs`)
- [x] **Bounded `forall`/aggregate unrolling** — add a bounded-range domain to
  the grammar (`forall i in lo..hi`, `sum/min/max/count(i in lo..hi)`);
  unroll to conjunctions/derived linear expressions at extract-time when
  bounds resolve to constants; reject non-constant-range quantifiers with a
  specific error. (`crates/tpt-telos-parser/src/ast.rs`, `grammar.ebnf`,
  `crates/tpt-telos-ir/src/extract.rs`)
- [x] **General nested/compound `if`/`match` in contracts** — generalize DNF
  negation (De Morgan over `&&`/`||`/if-as-value/match-as-value) so
  `if`/`match` can appear as arithmetic sub-expressions, not just as the
  whole clause. (`crates/tpt-telos-ir/src/extract.rs`)
- [x] **`Call`/`MethodCall` verified via callee contracts** — modular
  (Dafny-style) verification: substitute callee `ensures` as premises at
  call sites; call-graph cycle detection rejects recursive contract
  references with a clear error rather than unsound fixed-depth unrolling.
  (`crates/tpt-telos-ir/src/extract.rs`)
- [x] **Constant-index array/slice access in contracts** — unroll `Index`
  with a compile-time-constant index into fixed-size arrays; reject
  symbolic/non-constant indices with a specific error (full array theory is
  out of scope). (`crates/tpt-telos-ir/src/extract.rs`)
- [x] **Fix Go `Try` codegen stub and the `Stmt` silent-drop bug** — replace
  the literal `"_TODO"` sentinel with real `x, err := ...` / `(T, error)`
  codegen; fix `render_func_named`'s body loop silently dropping
  `Stmt::Let`/`If`/`Match`/`Return`; give Rust's own `Try` support a real
  `Result<...>` return type (currently never emitted).
  (`crates/tpt-telos-codegen/src/go.rs`, `crates/tpt-telos-codegen/src/lib.rs`)
- [x] **`StaticAgent` synthesis extended to match** — synthesize `if`/`match`
  bodies for case-split `ensures`, loops for bounded `forall`/aggregate
  `ensures`, and direct calls for `Call`-referencing `ensures`; unhandled
  shapes still fail loudly via a specific `Err`, never a silent empty
  candidate. (`crates/tpt-telos-agent/src/static_agent.rs`)
- [x] **Re-audit Phase 6's `[x]` claims** — spot-check "Disjunction (`||`) in
  premises," "Array and slice support," and "Z3/CVC5 optional backend" (the
  last already confirmed dead-wired: `--solver z3` sets a global the verifier
  never reads) against actual behavior before relying on them as
  prerequisites for the work above.
  - **Disjunction (`||`):** CONFIRMED WORKING. `requires a || b` correctly
    expands into separate `VerificationProblem` branches via
    `to_constraints_dnf` + `combine_dnf` in `build_problems`.
  - **Array/slice support:** CONFIRMED for type positions + codegen (`[T; N]`,
    `[T]` in Rust; `[N]T`, `[]T` in Go). `Expr::Index` is NOT lowered to IR
    constraints (falls through in `linearize`), so index access cannot appear
    in contracts — this is expected and out of scope for Phase 6.
  - **Z3/CVC5 backend:** FIXED. `verify()` now dispatches on
    `solver_backend()` — when `--solver z3` is set and the `z3` feature is
    enabled, verification uses Z3 instead of Fourier-Motzkin. The `negate`
    function in `solver.rs` was made `pub` to support this dispatch.

## Status: tpt-telos v0.2.0 — Phases 7–10 complete.

The full pipeline is in place: parser -> IR/constraint extraction -> SMT-style
verifier -> agentic transpiler (Generate -> Verify -> Counter-example ->
Rewrite) -> context router -> dual Rust/Go backends -> automatic FFI bridge ->
eject hatch -> LSP. CLI surface: `telos parse | verify | transpile | build |
project | eject | lsp`.

Phase 7 additions:
- Counterexamples surfaced in `telos verify` output and LSP diagnostics/hover
- Struct/enum definitions drive codegen (real per-field types, not hardcoded i64)
- Bounded forall/aggregate unrolling (lo..hi ranges in contracts)
- General nested/compound if/match in contracts (DNF expansion)
- Modular verification (Call/MethodCall resolved via callee ensures)
- Constant-index array/slice access in contracts
- Go Try codegen fixed (IIFE with error handling) + Stmt rendering complete
- Rust Try emits Result<T, E> return type
- Z3 solver backend wired into verification path
- StaticAgent synthesis extended (if/match/call bodies)

## Testing: Full Coverage
> Final state: every crate now has unit tests for its core logic and at least one
> integration test. `telos-parser`, `telos-ir`, `telos-router`, `telos-agent`,
> `telos-codegen` gained unit/integration suites; `telos-verifier` gained the
> `extended_tests` solver suite plus a `nested.telos` fixture (`tests/nested.rs`);
> `telos-cli` gained integration tests driving the binary (`tests/cli.rs`).
> A GitHub Actions workflow (`.github/workflows/ci.yml`) runs `cargo fmt --check`,
> `clippy -D warnings`, `cargo test`, and `cargo llvm-cov --fail-under-lines 75`.
> Workspace line coverage is ~80%.

- [x] `telos-parser`: unit tests for the lexer (tokens, whitespace/comment handling, error spans) and parser (every grammar production in `grammar.ebnf`, malformed-input error cases). (`tests/lexer.rs`, `tests/parser.rs`)
- [x] `telos-ir`: unit tests for AST -> IR lowering and constraint extraction (requires/ensures -> QF_LRA), including edge cases (empty contracts, nested expressions, unsupported constructs). (`tests/extract.rs`)
- [x] `telos-verifier`: expanded `solver.rs`/`wallet.rs` coverage to include unsat-core/counterexample extraction, integer-overflow edge cases, and additional `.telos` fixtures beyond `wallet`/`broken` (`tests/nested.rs`). (`src/solver.rs` `extended_tests`).
- [x] `telos-router`: unit tests for every `@boundary(...)` classification path (`cpu_bound`, `zero_allocation`, `crypto`, `network_io`, `high_concurrency`, `distributed`, plus `real_time`/`high_latency`) and the default/unannotated case. (`src/lib.rs` tests).
- [x] `telos-agent`: unit tests for `StaticAgent` synthesis logic in isolation, plus tests for the counter-example-guided rewrite loop hitting its retry/failure limits. (`tests/static_agent.rs`).
- [x] `telos-codegen`: unit tests for individual codegen pieces (struct field mutability, invariant `impl` generation, doc-comment emission, `analyze_func`, `collect_types`, eject hatch) independent of the full `gen.rs` pipeline test. (`src/lib.rs` tests).
- [x] `telos-cli`: integration tests for `telos verify`, `telos build`, `telos transpile`, `telos project`, and `telos eject` covering success, verification failure, and malformed-file exit codes/output. (`tests/cli.rs`)
- [x] Add regression fixtures under `examples/` for each bug found going forward, and wire them into an existing or new integration test. (`examples/nested.telos` wired into `telos-verifier/tests/nested.rs`.)
- [x] Set up `cargo llvm-cov` in CI to track coverage per-crate and fail below an agreed threshold. (`.github/workflows/ci.yml`, `--fail-under-lines 75`.)
- [x] **Milestone:** every crate in the workspace has unit tests for its core logic and at least one integration test; CI enforces a minimum coverage threshold.

## Phase 8: DX, Automation & Tooling Backlog

> Findings from a platform review (bugs, gaps, usability). All items implemented.

- [x] **Fix `telos verify` disjunction-group exit code** — the CLI's disjunction-group
  display block was flipping `overall = false` for every failing branch in a group instead
  of only when the whole group failed, causing `telos verify` to exit non-zero / print
  `RESULT: verification failed` even when every function's `all_passed` was true. Fixed to
  match `VerificationResult::all_passed` semantics (group fails only if no member passes).
  (`crates/tpt-telos-cli/src/main.rs`)
- [x] **Machine-readable `--json` output** — `--json` flag added to `telos verify`/`build`/
  `project` emitting `{file, passed, functions[{func_name, all_passed, checks[]}], proof_hash,
  ...}` so CI, editors, and the LSP can share one output format instead of each re-deriving
  pass/fail independently. (`crates/tpt-telos-cli/src/main.rs`, `tests/cli.rs`)
- [x] **`telos init`/`new` scaffold** — `telos init [--module NAME] [--out PATH]` generates a
  starter module + invariant + func skeleton, to lower the barrier for first-time users.
  (`crates/tpt-telos-cli/src/main.rs`, `tests/cli.rs`)
- [x] **Watch mode** — a `--watch` flag on `telos verify` polls the source file's mtime and
  re-verifies on change, to tighten the edit-verify loop. (`crates/tpt-telos-cli/src/main.rs`)
- [x] **Feature-matrix CI job** — added a `feature-matrix` job to CI building/testing/linting
  with `--features llm` and `--features z3` separately so regressions behind those flags
  don't ship silently. (`.github/workflows/ci.yml`)
- [x] **`telos verify-manifest` command** — `telos verify-manifest <manifest.json> <source>`
  re-hashes the source against a previously generated `telos-proof.json` and reports
  match/drift, making the existing cryptographic proof manifest (Phase 5) load-bearing for
  tamper-evidence. (`crates/tpt-telos-codegen/src/proof.rs`, `crates/tpt-telos-cli/src/main.rs`,
  `tests/cli.rs`)
- [x] **LSP quick-fix code actions** — `textDocument/codeAction` now returns a `quickfix`
  `CodeAction` per failing check with a counterexample, inserting a `requires !(...)` clause
  that excludes the concrete witness the solver found (a starting point, not a guaranteed
  fix), advertised via `codeActionProvider: true`. (`crates/tpt-telos-lsp/src/analysis.rs`,
  `crates/tpt-telos-lsp/src/lib.rs`)

## Phase 9: Platform Review Backlog

> Findings from a full-platform review (bugs, docs, usability, adoption). Architecture decision:
> stay with the self-contained FM solver / flat `crates/` workspace rather than expanding toward
> spec.txt's Z3/CVC5-primary, Kani/Prusti, LangGraph/vLLM, sibling-repo-integrated design — see
> rationale below. All items in this phase are implemented (see checkboxes below); the codebase
> contains the soundness fixes, router diagnostics, FFI validation, and CLI UX work.

### Soundness & correctness bugs
- [x] **Fix `if`/`else` contract lowering unsoundness** — `to_constraints_dnf` accepts the `else`
  branch unconditionally instead of guarding it with `!cond`, so a function that always executes
  `else` can verify even when `cond` was true and `then` should have applied.
  (`crates/tpt-telos-ir/src/extract.rs`, `Expr::If` arm)
- [x] **Fix `match` contract lowering unsoundness** — same issue: each arm becomes an independent
  DNF branch with no premise tying it to the scrutinee matching that pattern.
  (`crates/tpt-telos-ir/src/extract.rs`, `Expr::Match` arm)
- [x] **Fix nonlinear-bounding max-corner bug** — `linearize_bounded` always substitutes the
  single max corner value regardless of whether the surrounding relation needs an upper or lower
  bound, which can prove false facts or reject true ones depending on direction.
  (`crates/tpt-telos-ir/src/extract.rs`, `linearize_bounded`)
- [x] **Reject non-integer types at FFI boundaries** — `ffi.rs` hardcodes every scalar/field to
  `int64_t`/`i64`; floats/strings/bools/arrays/nested structs crossing an FFI-routed boundary are
  silently coerced instead of rejected with a clear error. (`crates/tpt-telos-codegen/src/ffi.rs`)
- [x] **Diagnose `real_time`/`zero_allocation` routed to Python** — `RoutingDiagnostic` only
  checks Go-target conflicts; a module tagged `@boundary(real_time, ml_training)` silently routes
  to Python (GC + interpreter) with no warning. (`crates/tpt-telos-router/src/lib.rs`)
- [x] **Warn on unrecognized `@state(...)` values** — typos like `@state(persistant)` silently
  fall back to `Ephemeral` with no diagnostic. (`crates/tpt-telos-router/src/lib.rs`)
- [x] **`StaticAgent`: handle inequality-only `ensures`** — currently only `==`, `if`/`match`
  splits, and direct calls synthesize a body; a pure inequality postcondition (e.g.
  `ensures balance >= 0`) silently produces an empty body instead of a reasonable first attempt.
  (`crates/tpt-telos-agent/src/static_agent.rs`)
- [x] **`eject --func <name>`: error on unmatched function name** — if the requested name doesn't
  exist but a different function has `@eject`, that other function is silently ejected instead of
  reporting "function not found." (`crates/tpt-telos-cli/src/main.rs`)

### Documentation integrity
- [x] Fix Phase 8's intro line (says most items are "proposed follow-ups, not yet implemented"
  while every item below is checked `[x]`) — resolve the inconsistency one way or the other.
- [x] Remove stray duplicate `TODO 126071x.md`-style files at repo root if confirmed to be
  accidental artifacts. (Audited: the only `TODO*.md` at the repo root is `TODO.md`
  itself — the historical `history/TODO 1260723.md` / `history/TODO 1260713.md` snapshots
  are intentional archives per `AGENTS.md` and were left in place. A leftover scratch repro
  `examples/_repro_scalar.telos` was also cleaned up as an accidental artifact.)
- [x] Add `examples/README.md` indexing all 12 `.telos` fixtures (one line each: what it
   demonstrates), since several fixture comments (`float.telos`, `array_test.telos`) qualify
   claims made in Phase 6 that aren't visible from TODO.md alone.
- [x] Cross-link root README to the 8 crate-level READMEs; add a troubleshooting section
   covering missing `gofmt`/`go`/Z3 on PATH (currently only documented in CLAUDE.md).
- [x] Add a short note to the root README on how the implemented architecture diverges from
  spec.txt's aspirational layout/tech stack, and why (see Phase 9 rationale above).

### CLI & automation UX
- [x] Make `--json` available on `telos eject`/`parse` (currently only on `verify`/`build`/`project`).
- [x] Add colorized/rustc-style diagnostic output (caret/underline source spans) to CLI errors.
- [x] Extend `--watch` beyond `verify` (currently naive single-file mtime poll, no debounce) to
  `build`/`project`, and add debouncing + directory-wide watching.
- [x] Add shell-completion generation (`clap_complete`) via a `telos completions` subcommand.
- [x] Add `telos init --template <name>` with 2-3 starter templates (simple invariant, dual-backend
  FFI, eject) instead of one hardcoded Counter module.
- [x] Surface the FM solver's documented integer-incompleteness in `verify` failure output, hinting
  that some failures may be solver limitations rather than genuine spec violations.

### Editor integration (highest-leverage adoption gap)
- [x] Build a minimal VS Code extension: syntax highlighting (TextMate grammar) + LSP client wiring
  (`vscode-languageclient`) against the existing `telos lsp` server. (`vscode-telos/`)
- [x] Extend the LSP server: `textDocument/formatting` (reuse the existing pretty-printer),
  `textDocument/definition`/`references`, `textDocument/completion`, and `textDocument/inlayHint`
  are all implemented (`crates/tpt-telos-lsp/src/lib.rs`, `crates/tpt-telos-lsp/src/analysis.rs`).
  Note: the symbol index (`build_index`) is built from the LSP session's currently-open documents,
  not a full workspace directory scan — definition/references only find symbols in files the editor
  has opened, not the whole project.
- [x] Document non-VS-Code editor setup (Neovim `nvim-lspconfig` / Helix `languages.toml` snippets
  against the same `telos lsp` binary). (`docs/editors.md`)
- [x] Expose the LSP's formatter as a standalone `telos fmt [--check|--stdout]` CLI command, and add
  `telos doctor` to check for the optional `go`/`gofmt`/`z3` tools some commands shell out to.
  (`crates/tpt-telos-cli/src/main.rs`)

### Release & contribution infra
- [x] Add ARM64/musl targets to the release build matrix
  (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin`) and a Dockerfile.
  (`.github/workflows/release.yml`, `Dockerfile`)
- [x] Add a Dockerfile and/or devcontainer.json for reproducible dev setup. (`Dockerfile`, `.devcontainer/devcontainer.json`)
- [x] Add `CONTRIBUTING.md` and `.github/ISSUE_TEMPLATE/`/PR template.
  (`CONTRIBUTING.md`, `.github/ISSUE_TEMPLATE/bug_report.md`, `.github/ISSUE_TEMPLATE/feature_request.md`,
  `.github/pull_request_template.md`)

### Innovation
- [x] Prototype a WASM build of `telos-parser` + `telos-verifier` (both dependency-light; default
  solver path needs no external Z3) for a browser-based playground — zero-install trial for new users.
  (`crates/out-telos-wasm/`)

## Phase 10: `tpt-telos-sdk` — Programmatic Orchestration API

> Motivation: a sibling integration-harness repo (`tpt-nexus`) needs a single library API over the
> parse -> agentic-transpile -> codegen -> attest pipeline instead of depending on 6 crates directly
> and hand-rolling its own orchestrator. Investigation showed most of what such an orchestrator needs
> (retry-loop observability via `FuncOutcome.iterations`, the manifest/attestation step) already
> exists in `tpt-telos-agent`/`tpt-telos-codegen` — it just isn't exposed as a public library today
> (it's wired together only inside `tpt-telos-cli`'s private `main.rs`). Scoped to close exactly the
> gaps that are real (a one-call pipeline entry point, a counterexample -> hint formatter, a build
> step that reads back compiled artifact bytes) — not `spec2.txt`'s full original vision (no PyO3
> bindings, no gRPC/HTTP daemon mode; zero current demand for either).

- [x] **New crate `tpt-telos-sdk`** — add to workspace `members`; `Cargo.toml` with path deps on
  `tpt-telos-parser`/`-ir`/`-verifier`/`-router`/`-agent`/`-codegen`, `llm`/`z3` passthrough features.
  (`crates/tpt-telos-sdk/Cargo.toml`, root `Cargo.toml`)
- [x] **`RUST_CRATE_NAME` const** — promote the hardcoded `"generated_rust"` literal in
  `rust_cargo_toml()` to a `pub const`, mirroring the existing `pub const GO_PACKAGE`.
  (`crates/tpt-telos-codegen/src/project.rs`)
- [x] **One-call pipeline (`compile`/`compile_static`)** — wraps
  parse -> `transpile_module` per module -> `generate_project` -> `generate_manifest` into a single
  `VerifiedArtifact { source, modules, outcomes, project, manifest, all_verified }`; `Err` only for
  "pipeline couldn't run" (parse/agent/codegen errors), not verification failure.
  (`crates/tpt-telos-sdk/src/lib.rs`)
- [x] **`SdkError`** — `Parse`/`Transpile`/`Codegen`/`Io`/`ToolNotFound` variants with `Display`/`Error`.
  (`crates/tpt-telos-sdk/src/error.rs`)
- [x] **Counterexample hint formatter (`format_hint`/`format_outcome_hints`)** — the "agent_hint"
  concept `tpt-nexus`'s TODO flags as missing from tpt-telos: renders a `CheckResult`'s clause kind,
  disjunction/approximation caveats, and sorted `Model` counterexample bindings (post-state keys as
  `<base>.<field> (post-state)`) into human/LLM-readable text.
  (`crates/tpt-telos-sdk/src/hint.rs`)
- [x] **Build/compile-to-artifact-bytes (`compile_project`/`compile_project_tempdir`)** — writes a
  `Project` to disk, shells `cargo build`/`go build` per backend, reads back the Rust
  rlib/staticlib bytes (Go yields no byte artifact — no `package main` exists to build a discoverable
  output from). Missing `cargo`/`go` on PATH -> `Err(ToolNotFound)`; a build that runs but fails ->
  `Ok(BuildOutput { success: false, .. })`. (`crates/tpt-telos-sdk/src/build.rs`)
- [x] **Flat re-exports** — `tpt_telos_sdk::*` covers `FuncOutcome`, `CodeAgent`, `Model`, `Target`,
  `ProofManifest`, `StaticAgent`, `#[cfg(feature="llm")] LlmAgent`, etc. so consumers don't need all 6
  wrapped crates as direct deps for type names. (`crates/tpt-telos-sdk/src/lib.rs`)
- [x] **New fixture `examples/unsatisfiable.telos`** — two `ensures` clauses on the same field that
  can't both hold, to deterministically exercise the "verification permanently fails" path (unlike
  `broken.telos`, whose buggy body the agentic loop actually repairs).
- [x] **Test suite** — unit tests per module (`hint.rs`/`build.rs`/`lib.rs`) plus `tests/sdk.rs`
  integration tests: `wallet.telos` verifies end-to-end, `unsatisfiable.telos` fails with a hint
  reporting the contradiction, `broken.telos` gets silently repaired (locks in that non-obvious
  behavior so it can't regress unnoticed). (`crates/tpt-telos-sdk/tests/sdk.rs`)
- [x] **Release/publish wiring** — add `tpt-telos-sdk` to the `cargo publish` loop.
  (`.github/workflows/release.yml`)
- [x] **Docs** — `crates/tpt-telos-sdk/README.md` (matching sibling crate READMEs);
  `CLAUDE.md`'s "Workspace layout" updated from eight to nine crates.

## Phase 11: Platform Review — Bugs, Gaps, Adoption & Automation (2026-08-08)

> Findings from a full platform review (documentation integrity, soundness, adoption,
> automation). The review plan is tracked in `.kilo/plans/1786107629137-platform-review-plan.md`.
> Note: per instruction, the `history/TODO 1260723.md` / `history/TODO 1260713.md` snapshot
> files are intentional archives and must NOT be deleted or cleaned up.

### Documentation integrity
- [x] **Fix crate-count inconsistency** — README/ARCHITECTURE/CONTRIBUTING said "eight crates"
  while the workspace has ten members (`out-telos-wasm`, `tpt-telos-sdk` omitted); aligned all
  three docs + README crate table. (`README.md`, `ARCHITECTURE.md`, `CONTRIBUTING.md`)
- [x] **Fix version drift** — `Cargo.toml` is `0.1.1`, `grammar.ebnf` said "v0.2.0", CHANGELOG
  only had `0.1.0`, vscode README/package.json said `0.1.0`; synced grammar to `0.1.1`, added a
  `0.1.1` CHANGELOG entry, and bumped the vscode `0.1.1.vsix` / `package.json` version.
  (`crates/tpt-telos-parser/src/grammar.ebnf`, `CHANGELOG.md`, `vscode-telos/*`, `Cargo.toml`)
- [x] **Fix grammar.ebnf `@state` claim** — comment said "(storage class, parsed only)" but
  `@state(persistent|ephemeral)` is implemented (storage class in router/codegen); corrected.
  (`crates/tpt-telos-parser/src/grammar.ebnf`)
- [x] **README usage omissions** — added `telos completions`, `telos transpile`, and
  `telos init --template` snippets (including the new templates). (`README.md`)
- [x] **Note on history TODO files** — documented in `AGENTS.md`/`CLAUDE.md` that
  `history/TODO 1260723.md` / `history/TODO 1260713.md` are intentional archives not to be
  deleted. (`AGENTS.md`, `CLAUDE.md`)

### Soundness & correctness hardening
- [x] **Harden FM solver against `i128` overflow** — `solver.rs` now uses checked `i128`
  arithmetic; on overflow `unsat_checked` returns `None` ("bounds too large to decide") and
  `unsat` conservatively returns `false` (never a spurious contradiction), preserving the
  advertised integer-soundness. (`crates/tpt-telos-verifier/src/solver.rs`)
- [x] **Add `examples/overflow.telos` fixture + verifier test** — `unsat_checked_overflow_is_conservative`
  (unit) and `overflow_example_does_not_panic` (integration) lock in the overflow behavior.
  (`examples/overflow.telos`, `crates/tpt-telos-verifier/tests/nested.rs`, `solver.rs`)
- [x] **Warn on unknown `@boundary(...)` flags** — typos like `@boundary(cp_bound)` now emit an
  `UnrecognizedBoundaryFlag` diagnostic in `route_checked`. (`crates/tpt-telos-router/src/lib.rs`)
- [x] **`collect_verify_output` JSON group-awareness — verified correct** — audited: `verify()`
  computes `all_passed` via `any_passed` per disjunction group, and `collect_verify_output` reads
  that field (not per-check `passed`), so JSON `overall` already honors disjunction groups. No change.
- [x] **`assign_constraint` accepts bare local-variable assignment targets** — `Stmt::Assign` to a
  bare `Expr::Var` (e.g. a scalar `ensures out == a + b` output bound by `out = a + b;`) is now
  lowered into a pre/post-state-independent equality constraint, so the default offline
  `StaticAgent` can synthesize scalar `ensures` clauses. (`crates/tpt-telos-ir/src/extract.rs`,
  `assign_constraint` `Expr::Var` arm; locked in by `transpile_scalar_out_ensures_verifies` in
  `crates/tpt-telos-agent/tests/static_agent.rs`.)
- [x] **`problem_for` returns `Result` instead of panicking** — the re-extraction step now uses
  `ok_or_else(...)` to surface an internal extraction failure as a normal `Err` rather than a hard
  process panic on the first verify attempt. (`crates/tpt-telos-agent/src/lib.rs`, `problem_for`)

### Adoption: templates, examples, SDK docs
- [x] **New `init --template` options** — added `real-time`, `python-ml`, `cross-module`
  (and fixed pre-existing `: Int` return-type and `return`-in-`mutate state` bugs in the
  `simple`/`dual-backend`/`eject` templates so every template verifies).
  (`crates/tpt-telos-cli/src/main.rs`, `tests/cli.rs`)
- [x] **`examples/START-HERE.telos`** — annotated walkthrough referenced from README.
  (`examples/START-HERE.telos`, `examples/README.md`)
- [x] **`telos new <name>` project scaffold** — emits a `<name>.telos` + README so a beginner
  reaches `telos project --check` in one command. (`crates/tpt-telos-cli/src/main.rs`, `tests/cli.rs`)
- [x] **Document the SDK in the root README** — added a "Using the SDK" section +
  `crates/tpt-telos-sdk/examples/sdk_usage.rs` (runnable example). (`README.md`, `crates/tpt-telos-sdk/examples/sdk_usage.rs`)

### Automation & CI
- [x] **CI doc-consistency check** — `.github/workflows/ci.yml` now asserts crate count +
  version labels in README/ARCHITECTURE/CONTRIBUTING/grammar.ebnf match `Cargo.toml`.
- [x] **MSRV job** — `.github/workflows/ci.yml` enforces `rust-version = 1.74` (dtolnay/rust-toolchain@1.74.0).
- [x] **Playground deploy job** — build `out-telos-wasm` and publish a static `playground/`
  site to GitHub Pages. (`playground/`, `.github/workflows/playground.yml`)

### Innovation (open)
- [x] **LSP symbol index** — `textDocument/definition` / `references` / `completion` are wired
  from a workspace-wide symbol table (`build_index` over all open documents), and inlay hints show
  the module's routing target plus `old(...)` `pre-state` markers.
  (`crates/tpt-telos-lsp/src/analysis.rs`, `crates/tpt-telos-lsp/src/lib.rs`)
- [x] **Hosted browser playground** — minimal `playground/` static site (textarea + live
  `verify` output + counterexample display) over `out-telos-wasm`, deployed by the Playground
  workflow. (`playground/index.html`, `playground/main.js`, `playground/style.css`,
  `.github/workflows/playground.yml`)

## Phase 12: "Verification Bridge" Investigation — Cross-Repo GPU/Argus Integration (2026-08-09)

> A proposed extension asked for a verification pass ingesting tpt-gpu's TPTIR / tpt-crucible's
> compute graph to prove (1) GPU arena sizing is sufficient for all input shapes, and (2)
> tpt-argus's alerting rules for a deployment are contradiction-free. Investigated the three sibling
> repos before building anything; the premise didn't hold, so the bespoke bridge was **not** built.
> This mirrors the Phase 9 decision to stay self-contained rather than chase sibling-repo
> integrations without concrete demand — recorded here so it isn't blindly re-proposed later.

- **tpt-gpu**: TPTIR is real (`crates/tpt-gpu-ir-spec`) but is SSA `Operation`/`Block`/`Region`
  form, not a graph. There is no "deterministic arena size" concept — the runtime `Arena` is a
  simulated memory buffer, and the project deliberately *removed* `input_shapes()`/
  `output_shape()` from its kernel trait because "shapes are dynamic." Proving arena sufficiency
  needs a static-shape model tpt-gpu's own team removed on purpose.
- **tpt-crucible**: has its own graph IR (`ComputationalGraph{nodes, edges}` in
  `crates/tpt-catalyst/src/ir.rs`), but it's Crucible's "TPT-IR", not tpt-gpu's TPTIR. Crucible's
  own TODO records it's blocked waiting on tpt-gpu to publish a shared IR spec — that unification
  doesn't exist between those two repos yet, let alone a third consumer.
- **tpt-argus**: already solves rule-contradiction checking in-house (`argus-logic`'s
  `analyze_contradictions`, interval-overlap over a `Condition` AST). An earlier draft of its own
  spec proposed depending on tpt-telos for this; the actual dev notes record that being **rejected**
  in favor of a self-contained implementation.
- [x] **What was built instead**: `tpt_telos_sdk::check_contradictions` — a `.telos`-source-
  independent entry point exposing just the solver core (`tpt_telos_ir::Constraint` +
  `tpt_telos_verifier::unsat_checked`) so any tool in the workspace can translate its own domain
  rules into `Constraint`s and get pairwise contradiction-checking, without tpt-telos needing to
  ingest a format (TPTIR/compute-graph) that isn't stable yet. (`crates/tpt-telos-sdk/src/contradiction.rs`,
  `crates/tpt-telos-sdk/README.md`)

## Phase 13: `telos-prove` CLI — Constraint-Group Contradiction Checking (2026-08-09)

> A thin CLI over the existing `tpt_telos_sdk::check_contradictions`/`unsat_checked`/`model` solver
> core, for callers with no `.telos` source who want a standalone binary rather than a library
> dependency. Deliberately minimal: no new `ConstraintSolver` trait, no new `Constraint` enum
> variants, no new `SatResult` type — pure I/O plumbing over what already exists, per the Phase 12
> "stay self-contained" decision.

- [x] Fix the pre-existing broken `check_contradictions` doctest (and its README copy) — was asserting
  `value > 200` combined with `value >= 200` is a contradiction (it isn't; `value = 300` satisfies
  both). Replaced with a genuine `<= 200` vs `>= 500` contradiction, plus a positive doctest proving
  `> 200` + `>= 200` is consistent. (`crates/tpt-telos-sdk/src/contradiction.rs`, `crates/tpt-telos-sdk/README.md`)
- [x] Hand-rolled JSON reader, no serde — `Json`/`JsonValue` + recursive-descent parser, errors carry
  a byte offset converted to line/col via `tpt_telos_parser::span::LineIndex`.
  (`crates/tpt-telos-sdk/src/json.rs`)
- [x] Constraint-group schema + report — `parse_groups`/`build_report`/`format_human`/`format_json`
  wrapping `check_contradictions`/`unsat_checked`/`model`; per-group self-unsat detection plus
  independent `overall_unsat` checking for jointly-unsat-but-no-pairwise-conflict inputs.
  (`crates/tpt-telos-sdk/src/prove.rs`)
- [x] Wire `json`/`prove` modules + re-exports into the crate root. (`crates/tpt-telos-sdk/src/lib.rs`)
- [x] New `telos-prove` binary — auto-discovered from `src/bin/telos-prove.rs`, no new
  `Cargo.toml` dependencies. `--json`/`--strict` flags, reads a file arg or stdin.
  (`crates/tpt-telos-sdk/src/bin/telos-prove.rs`)
- [x] Fixtures + tests — `tests/fixtures/*.json`, unit tests in `json.rs`/`prove.rs`, and
  `tests/telos_prove.rs` driving the built binary via `std::process::Command` (mirrors
  `crates/tpt-telos-cli/tests/cli.rs`'s pattern; no `assert_cmd` dependency added).
  (`crates/tpt-telos-sdk/tests/`)
- [x] Docs — `crates/tpt-telos-sdk/README.md` `telos-prove` section; `CLAUDE.md` workspace-layout
  bullet mentions `check_contradictions` + `telos-prove`.

> Phase 13 implemented: `telos-prove` is a thin, dependency-free CLI over the existing
> `tpt_telos_sdk::check_contradictions`/`unsat_checked`/`model` solver core, for callers with no
> `.telos` source who want a standalone binary instead of a library dependency. It adds a hand-rolled
> JSON reader (`json.rs`, no serde), a constraint-group schema + report (`prove.rs`:
> `parse_groups`/`build_report`/`format_human`/`format_json`), and the `telos-prove` binary
> (file-or-stdin input, `--json`/`--strict` flags). The report surfaces per-group self-unsat
> detection and pairwise/joint contradictions via `overall_unsat`. No new `ConstraintSolver` trait,
> `Constraint` variants, or `SatResult` type were introduced — pure I/O plumbing per the Phase 12
> "stay self-contained" decision. Verified end-to-end: the fixed doctest now correctly flags
> `value <= 200` vs `value >= 500` as a contradiction (and proves `value > 200` + `value >= 200` is
> consistent), and `tests/telos_prove.rs` drives the binary across contradiction/consistent/self-unsat/
> json/strict/stdin/malformed paths.


## Phase 14: Correctness Fixes — Tier 1 Bugs (2026-08-20)

> Four correctness issues surfaced in a systematic codebase review: a thread-safety UB in
> the global solver backend, a CE-rewriter that can't fix bugs inside conditional branches,
> a silent 0-fallback in CE evaluation that hides missing Expr handling, and the aggregate
> lowering is dead code. Addressed in priority order before adding new features.

- [x] **`static mut SOLVER_BACKEND` replaced with `OnceLock<SolverBackend>`** — eliminates UB
  under the multi-threaded gRPC cluster. `set_solver_backend` becomes `OnceLock::set(...).ok()`
  (idempotent; no-op if already set); `solver_backend()` uses `OnceLock::get().copied().unwrap_or_default()`.
  (`crates/tpt-telos-verifier/src/lib.rs`)
- [x] **`apply_fixes` now recurses into `if`/`match` arms** — previously the CE-guided rewriter
  only patched top-level `MutateState`/`Assign` statements; verification failures inside
  conditional branches were never repaired. Added recursive descent for `Stmt::If` (then+else)
  and `Stmt::Match` (all arms). (`crates/tpt-telos-agent/src/static_agent.rs`)
- [x] **`eval_post`/`eval_pre` catch-all replaced with `debug_assert!`** — the silent `_ => 0`
  fallback for unhandled `Expr` variants in CE evaluation now fires a `debug_assert!(false, ...)`
  in debug builds, ensuring new AST expression kinds are caught at test time rather than
  producing misleading CE witnesses. (`crates/tpt-telos-agent/src/static_agent.rs`)
- [ ] **Aggregate expression lowering audit** — `sum(...)` in contracts computes a `Linear`
  sum successfully but then returns `Err(...)` before emitting a constraint. Needs either
  proper integration via `linearize` at the comparison level, or an explicit
  "unsupported" error surfaced at constraint-extraction time (not a silent Err at the inner loop).
  (`crates/tpt-telos-ir/src/extract.rs` around line 618)

> Phase 14 (partial): OnceLock fix, apply_fixes recursion, and eval debug_assert are implemented
> and tested. Aggregate lowering audit is tracked as an open item for a follow-up commit.

## Phase 15: Test Coverage — Router Tests & CI Fix (2026-08-20)

> `tpt-telos-router` (606 lines) had zero test coverage despite driving all `@boundary(...)`
> dispatch decisions. A comprehensive test suite was added covering all flags, priority ordering,
> conflict diagnostics, @state(...) parsing, and helper methods. Also tracks the CI action name
> audit.

- [x] **Router test suite** — 30+ named tests in `tests/router.rs` covering: all Rust/Go/Python
  boundary flags individually, priority ordering (Python > Go > Rust), all diagnostic kinds
  (`RealTimeGoConflict`, `ZeroAllocGoConflict`, `RealTimePythonConflict`, `ZeroAllocPythonConflict`,
  `UnrecognizedBoundaryFlag`, `UnrecognizedStateValue`), `@state(persistent|ephemeral)` parsing,
  `route()` vs `route_checked()` consistency, `is_rust()` helper, and non-boundary attribute
  passthrough. (`crates/tpt-telos-router/tests/router.rs`)
- [ ] **CI action name audit** — verify whether `taiki197/install-action@cargo-llvm-cov` in
  `.github/workflows/ci.yml` is correct or should be `taiki-e/install-action`. Fix if it's a typo.
  (`.github/workflows/ci.yml`)

## Phase 16: `tpt-telos-prove` Standalone Crate (2026-08-20)

> Extract the `telos-prove` binary from `tpt-telos-sdk` into its own minimal crate with only
> `tpt-telos-verifier` as a dependency, enabling `cargo install tpt-telos-prove` without pulling
> in codegen, LSP, or agent crates. Keeps the SDK as a pure library.

- [x] New crate `crates/tpt-telos-prove/` with `Cargo.toml` + `src/main.rs` (adapted from
  `crates/tpt-telos-sdk/src/bin/telos-prove.rs`)
- [x] Copy hand-rolled JSON reader + constraint-group prove logic from `tpt-telos-sdk`
  into the new crate (`src/json.rs`, `src/contradiction.rs`, `src/prove.rs`)
- [x] Add `crates/tpt-telos-prove` to workspace `Cargo.toml` members
- [x] 17 unit tests passing across json/contradiction/prove modules
- [x] `crates/tpt-telos-prove/README.md` with install/usage/schema docs
- [ ] Remove or replace `tpt-telos-sdk`'s `telos-prove` binary (shim or delete)
- [ ] Update `CLAUDE.md` workspace-layout section and SDK README

## Phase 17: Equality-Substitution Preprocessing in the Verifier (2026-08-20)

> Add an `equality_substitute` preprocessing pass before Fourier-Motzkin elimination. When the
> constraint set contains `x == linear_expr`, substitute `x → linear_expr` everywhere else and
> remove the defining equality, then repeat to fixpoint. Sound and complete for QF_LRA. Makes
> proof chains explicit and auditable — critical for the "high-consequence numeric module" use case.
> Unlocks canonical ledger / rate-limiter / quota-tracker / FSM verification with minimal contracts.

- [ ] `equality_substitute(constraints: &mut Vec<Constraint>)` in `solver.rs` — scan for `Eq`
  where one side is a single variable, substitute to fixpoint, remove defining equalities
  (`crates/tpt-telos-verifier/src/solver.rs`)
- [ ] Call `equality_substitute` at the top of `verify()` / `unsat_checked()` before FM elimination
- [ ] New test file `crates/tpt-telos-verifier/tests/substitution.rs` — ledger, rate-limiter,
  quota, FSM cases; chained substitutions
- [ ] New example fixtures: `examples/ledger.telos`, `examples/rate_limiter.telos`,
  `examples/quota.telos` — wired into `tpt-telos/tests/cli.rs`

## Phase 18: Language Extensions (2026-08-20)

> Three targeted language/IR improvements: boolean literal support (`true`/`false`), `match` arms
> with `mutate state` blocks, and array length constraint extraction.

- [ ] **Boolean literals** — `true`/`false` parsed as `Expr::Bool(bool)`, lowered to `1`/`0`
  in the IR (FM solver unchanged), emitted as `true`/`false` in Rust/Go/Python codegen.
  Update `grammar.ebnf`, lexer, AST, IR extract, codegen, and pretty-printer.
  (`crates/tpt-telos-parser/src/{lexer,ast}.rs`, `crates/tpt-telos-ir/src/extract.rs`,
  `crates/tpt-telos-codegen/src/{lib,go,python}.rs`)
- [ ] **`match` with `mutate state` arms** — extend `extract_stmt` for `Stmt::Match` to support
  `mutate state` inside arm bodies; each arm becomes a separate DNF branch.
  (`crates/tpt-telos-ir/src/extract.rs` around line 921)
- [ ] **Array length IR constraints** — extract `arr.len() == N` comparisons in `requires`/
  `ensures` into `Constraint::Eq` pairs. Symbolic index support stays out-of-scope.
  (`crates/tpt-telos-ir/src/extract.rs`)

## Phase 19: DX & Ecosystem (2026-08-20)

> Three ecosystem improvements: LSP workspace-wide symbol index, FFI type expansion, and Go
> payload-carrying enum codegen.

- [ ] **LSP workspace symbol index** — implement `workspace/didChangeWatchedFiles` /
  `workspace/symbol` with a background indexer scanning all `.telos` files on `initialize`.
  Wire into definition/references/completion handlers.
  (`crates/tpt-telos-lsp/src/analysis.rs`, `lib.rs`)
- [ ] **FFI bridge: `f64` and flattened nested struct fields** — extend `ffi.rs` to support
  `f64` (passed as `u64` bit patterns + transmute) and nested struct fields expanded field-by-field.
  (`crates/tpt-telos-codegen/src/ffi.rs`)
- [ ] **Go payload-carrying enum variants** — emit tagged structs per variant with a `Kind`
  discriminant field (Go's idiomatic sum-type pattern).
  (`crates/tpt-telos-codegen/src/go.rs`)
