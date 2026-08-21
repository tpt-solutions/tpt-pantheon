# tpt-telos-sdk

**Programmatic orchestration API over the tpt-telos parse → transpile → codegen → attest pipeline.**

Part of the [tpt-telos](https://github.com/tpt-solutions/tpt-telos) compiler workspace.

## Overview

`telos`'s library crates (`parser`, `ir`, `verifier`, `router`, `agent`, `codegen`) are individually
excellent but require a consumer to wire them together by hand. `tpt-telos-sdk` provides a single
entry point for integration harnesses (e.g. `tpt-nexus`) that want the whole pipeline as one call,
plus the missing "agent_hint" surface (human/LLM-readable counterexample formatting) and a build
step that reads back compiled artifact bytes.

## Usage

```rust
use tpt_telos_sdk::{compile, format_outcome_hints, StaticAgent};

let src = r#"
    module Bank {
        invariant Wallet { balance >= 0 }
        func deposit(w: Wallet, amount: PositiveInt)
            ensures w.balance == old(w.balance) + amount
        ;
    }
"#;

// One call: parse -> agentic transpile -> codegen -> attest.
let artifact = compile(src, &StaticAgent::new()).unwrap();
if !artifact.all_verified {
    // Verification failure is *not* an error — surface the hints instead.
    for outcome in &artifact.outcomes {
        eprintln!("{}", format_outcome_hints(outcome));
    }
}

// artifacts:
//   artifact.modules   — parsed AST
//   artifact.outcomes  — per-function transpilation results
//   artifact.project   — assembled Rust/Go/FFI sources (write + build via compile_project)
//   artifact.manifest  — attestation manifest (source hash + per-fn record)
//   artifact.all_verified
```

`compile_static` accepts raw bytes; `compile_project` / `compile_project_tempdir` write the
[`Project`](crate::Project) to disk and shell `cargo build` / `go build`, returning the compiled
Rust artifact bytes.

## Errors

`SdkError` distinguishes pipeline failures (`Parse`, `Transpile`, `Codegen`, `Io`,
`ToolNotFound`) from genuine verification failure, which is reported via
[`VerifiedArtifact::all_verified`](crate::VerifiedArtifact::all_verified) rather than as an error.

## Generic contradiction checking (no `.telos` source required)

Everything above orchestrates the full `.telos` pipeline. [`check_contradictions`] is a separate,
standalone entry point over just the solver core, for callers that have their own domain rules —
not `.telos` source — but still want the QF_LRA solver to check them for mutual consistency. This
is meant as an opt-in building block for other tools in the workspace: translate your rules into
[`Constraint`]s and call it directly, no dependency on the rest of the pipeline required.

```rust
use tpt_telos_sdk::{check_contradictions, NamedConstraints};
use tpt_telos_ir::{Constraint, Linear, Relation};

// Two disjoint threshold rules on the same metric, e.g. from an
// alerting-rule engine: "value <= 200" and "value >= 500".
let low = NamedConstraints {
    label: "latency_ok".to_string(),
    constraints: vec![Constraint(
        Linear::var("value").sub(&Linear::constant_only(200)),
        Relation::Le,
    )],
};
let high = NamedConstraints {
    label: "latency_critical".to_string(),
    constraints: vec![Constraint(
        Linear::var("value").sub(&Linear::constant_only(500)),
        Relation::Ge,
    )],
};

let report = check_contradictions(&[low, high]);
// The two rules can never both hold (no value is <= 200 and >= 500).
assert!(report.overall_unsat == Some(true));
assert_eq!(report.pairs[0].a, "latency_ok");
assert_eq!(report.pairs[0].b, "latency_critical");
```

This only reasons over linear arithmetic — the same fragment the core solver always has. A rule
with a nonlinear term (e.g. an area or product computation) must be pre-bounded into a linear
over-approximation by the caller before being passed in, the same way `.telos` source itself is
handled internally by the IR layer's interval bounding.

## `telos-prove` — standalone contradiction-checking CLI

For callers who have constraint groups but no `.telos` source and don't want a
library dependency, `tpt-telos-sdk` ships a `telos-prove` binary (auto-discovered
from `src/bin/telos-prove.rs`). It reads named constraint groups as JSON (from a
file argument or stdin) and reports pairwise/joint contradictions via the
self-contained FM solver.

```bash
# JSON from a file (top-level array of groups, or { "groups": [...] })
telos-prove tests/fixtures/contradiction.json
telos-prove --json tests/fixtures/contradiction.json   # machine-readable
telos-prove --strict tests/fixtures/consistent.json    # non-zero exit on any conflict/undecided
# or pipe JSON on stdin
echo '[{ "label": "low", "constraints": [{ "linear": { "terms": [["x", 1]], "constant": 0 }, "relation": "<=" }] },
       { "label": "high", "constraints": [{ "linear": { "terms": [["x", 1]], "constant": -1 }, "relation": ">=" }] }]' \
  | telos-prove
```

Input schema (one group per named rule/assumption):

```json
{
  "groups": [
    {
      "label": "latency_ok",
      "constraints": [
        { "linear": { "terms": [["value", 1]], "constant": -200 }, "relation": "<=" }
      ]
    }
  ]
}
```

`relation` is one of `<=`, `>=`, `==`, `<`, `>`, `!=`. A `linear`'s `terms` are
`[variable, coefficient]` pairs; `constant` is added directly — so `["value", 1]`
with `constant: -200` and `"<="` encodes `value <= 200`. The output flags each
group's self-unsatisfiability (a group whose own constraints can never hold) and
pairwise contradictions between groups, plus an `overall_unsat` flag for the
joint set (which catches jointly-unsat-but-no-pairwise-conflict inputs). No new
`ConstraintSolver` trait, `Constraint` variants, or `SatResult` type are
introduced — it is pure I/O plumbing over [`check_contradictions`].

