# tpt-telos-ir

**Intermediate representation and constraint extraction for tpt-telos.**

Part of the [tpt-telos](https://github.com/tpt-solutions/tpt-telos) compiler workspace.

## Overview

`tpt-telos-ir` sits between the parser and the verifier. It lowers the typed AST produced by
`tpt-telos-parser` into `VerificationProblem`s — a flat, linear-arithmetic representation of each
function's `requires`/`ensures` contracts and invariants in QF_LRA (quantifier-free linear rational
arithmetic) form. This IR is the contract between the parser-side of the pipeline and the
solver-side.

Beyond straight-line linear lowering, `extract` also handles: disjunction (`requires a || b`) via
DNF normalization into independent verification branches; bounded `forall i in lo..hi { .. }` and
`sum`/`min`/`max`/`count` aggregates, unrolled to conjunctions when the range bounds are constants;
general nested/compound `if`/`match` as arithmetic sub-expressions; modular (Dafny-style)
verification of `Call`/`MethodCall` sites by substituting the callee's `ensures` as premises
(rejecting recursive call cycles); and constant-index array/slice access. It performs a
cross-module type-resolution pass first, so one module's invariant types can appear in another
module's function signatures. `extract` can fail (e.g. on an unsupported construct or a call cycle),
so it returns a `Result`.

## Usage

```rust
use tpt_telos_parser::parse;
use tpt_telos_ir::extract;

let modules = parse(src).unwrap();
let problems = extract(&modules).unwrap();

for problem in &problems {
    println!("{}: {} premise(s), {} conclusion(s)",
        problem.func_name,
        problem.premises.len(),
        problem.conclusions.len());
}
```

## Key types

| Type | Description |
|------|-------------|
| `extract(modules: &[Module]) -> Result<Vec<VerificationProblem>, String>` | AST → IR lowering |
| `VerificationProblem` | Per-function record: `func_name`, `func_span`, `premises`, `conclusions` |
| `Constraint` | A single `Linear` expression with a `Relation` (`≤`, `<`, `=`, …) |
| `Linear` | Sum of `(variable, coefficient)` pairs plus a constant |
| `Conclusion` | One thing to prove: `description`, `constraint`, `is_ensures`, `is_approximation`, `or_group` |
| `Relation` | `Le`, `Lt`, `Ge`, `Gt`, `Eq`, `Ne` |

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your option.
