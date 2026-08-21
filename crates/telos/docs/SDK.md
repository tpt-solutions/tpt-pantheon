# tpt-telos SDK Integration Guide

`tpt-telos-sdk` is the programmatic orchestration API for integrators (CI gates,
orchestration services, sibling repos). Depend on it instead of the six pipeline crates
directly — it re-exports the types you need and exposes a one-call pipeline plus hint
formatting and a build step.

> The source of truth is `crates/tpt-telos-sdk/src/lib.rs`. A runnable example lives at
> `crates/tpt-telos-sdk/examples/sdk_usage.rs` (`cargo run -p tpt-telos-sdk --example
> sdk_usage`).

---

## 1. Add the dependency

```toml
[dependencies]
tpt-telos-sdk = "0.2.0"
```

---

## 2. The one-call pipeline: `compile`

`compile(source, &agent)` runs the full pipeline — parse → agentic transpile → codegen →
proof manifest — and returns a `VerifiedArtifact`.

```rust
use tpt_telos_sdk::{compile, StaticAgent};

let src = r#"
    module Bank {
        invariant Wallet { balance >= 0 }
        func deposit(w: Wallet, amount: Int)
            requires amount > 0
            ensures w.balance == old(w.balance) + amount
        {
            mutate state { w.balance += amount }
        }
    }
"#;

let artifact = compile(src, &StaticAgent::new())
    .expect("the pipeline (parse/transpile/codegen) should run");

assert!(artifact.all_verified, "deposit must verify");
```

Important semantics:

- `Err` is returned **only** when the pipeline itself could not run (parse, transpile, or
  codegen failure) — surfaced as `SdkError`.
- A program whose contracts are **not** satisfied still returns `Ok(...)` with
  `all_verified == false`. Verification failure is a *result*, not an error.

`compile_static(source_bytes, &agent)` is the same but accepts raw `&[u8]` (e.g. read
from disk).

```rust
use tpt_telos_sdk::{compile_static, StaticAgent};

let src = b"module Bank { func noop() ; }";
let artifact = compile_static(src, &StaticAgent::new()).unwrap();
assert!(artifact.all_verified);
```

---

## 3. Reading the `VerifiedArtifact`

```rust
pub struct VerifiedArtifact {
    pub source: Vec<u8>,                                  // original source bytes
    pub modules: Vec<tpt_telos_parser::ast::Module>,      // parsed modules
    pub outcomes: Vec<tpt_telos_agent::FuncOutcome>,      // per-function results
    pub project: tpt_telos_codegen::project::Project,     // assembled Rust/Go/FFI
    pub manifest: tpt_telos_codegen::proof::ProofManifest,// source hash + records
    pub all_verified: bool,                               // every func verified?
}
```

Common reads:

```rust
println!("all_verified = {}", artifact.all_verified);
println!("functions   = {}", artifact.outcomes.len());
println!("has rust    = {}", artifact.project.has_rust);
println!("has go      = {}", artifact.project.has_go);
println!("manifest    = {}", artifact.manifest.manifest_hash);
```

The `StaticAgent` is fully offline and deterministic. To use a real LLM backend, build
the SDK with the `llm` feature and use `LlmAgent` (re-exported when the feature is on):

```rust
use tpt_telos_sdk::{compile, LlmAgent}; // requires `llm` feature

let agent = LlmAgent::from_env()?; // needs TELAS_LLM_KEY / TELAS_LLM_PROVIDER
let artifact = compile(src, &agent)?;
```

---

## 4. Counterexample hints

`format_outcome_hints(&outcome)` renders a human/LLM-readable hint for a function's
verification outcome, including any counterexample. `format_hint(&check)` formats a single
`CheckResult`.

```rust
use tpt_telos_sdk::{compile, format_outcome_hints, StaticAgent};

let artifact = compile(src, &StaticAgent::new())?;
for outcome in &artifact.outcomes {
    let hint = format_outcome_hints(outcome);
    println!("hint: {hint}");
}
```

These are the same hints the CLI prints; useful for surfacing failures in your own
reports or LLM prompts.

---

## 5. Building the generated project

`compile_project(artifact)` and `compile_project_tempdir(artifact)` drive the
`cargo`/`go` build step, returning a `BuildOutput`. Use this when you want to compile
inside your own tooling rather than via the `telos build`/`telos project` CLI.

```rust
use tpt_telos_sdk::{compile, compile_project, StaticAgent};

let artifact = compile(src, &StaticAgent::new())?;
let output = compile_project(&artifact)?;
// output has the build status for the generated crate(s).
```

---

## 6. Re-exports you get for free

`tpt-telos-sdk` re-exports the underlying crate types so you don't need six direct
dependencies:

- `error::SdkError`
- `CodeAgent`, `FuncOutcome`, `StaticAgent` (and `LlmAgent` under `llm`)
- `codegen::project::Project`, `codegen::proof::ProofManifest`
- `ir::{Constraint, Linear, Relation}`
- `parser::ast::Module`
- `router::Target`
- `verifier::{entails, is_unsat, model, unsat_checked, CheckResult, Model, VerificationResult}`
- `build::{compile_project, compile_project_tempdir, BuildOutput}`
- `contradiction::{check_contradictions, Contradiction, ContradictionReport, NamedConstraints}`
- `hint::{format_hint, format_outcome_hints}`
- `json::{parse_json, JsonError, JsonValue}`
- `prove::{build_report, parse_groups, ProveReport}`

`prove::build_report` / `contradiction::check_contradictions` let you run the same
QF_LRA reasoning directly over named constraint groups, and `json` is a tiny JSON
parser if you need to feed external constraint data in.

---

## 7. Error handling

Only the pipeline-failure path produces an `Err(SdkError)`. Treat `all_verified ==
false` as a first-class result and branch on it:

```rust
match compile(src, &StaticAgent::new()) {
    Ok(artifact) if artifact.all_verified => { /* ship it */ }
    Ok(artifact) => { /* report counterexamples via format_outcome_hints */ }
    Err(e) => { /* pipeline broken: parse/transpile/codegen IO or syntax */ }
}
```

---

## 8. See also

- Example: `crates/tpt-telos-sdk/examples/sdk_usage.rs`
- CLI equivalents: [`docs/CLI.md`](CLI.md) (`build`, `project`, `verify`, `eject`)
- Language reference: [`docs/LANGUAGE.md`](LANGUAGE.md)
