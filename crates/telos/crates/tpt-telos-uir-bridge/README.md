# tpt-telos-uir-bridge

The **Prover Bridge** for [`tpt-uir`](https://github.com/tpt-solutions/tpt-uir)
(Phase 4 of that project).

It consumes a TPT-UIR `Region` — produced by the `tpt-gpu` / `tpt-crucible`
ingestion adapters and serialized as a `.tptuir` file — and formally proves that
the `tpt_memory.alloc` allocations inside each memory scope never exceed the target
hardware's physical-memory budget.

## How it works

1. An adapter lowers a model to a TPT-UIR `Region` and serializes it (postcard).
   The `tpt-uir-dialects` liveness pass has already wrapped each alloc-bearing
   operation with `tpt_memory.scope_begin` / `tpt_memory.alloc` /
   `tpt_memory.scope_end`.
2. `extract_allocs` walks the region and, for every `mem.alloc`, resolves a
   symbolic byte-size expression from the allocated tensor's type (a `tensor`/`type`
   attribute, or a block argument). `extract_symbolic_dims` / `extract_bounded_dims`
   collect the `Dimension::Symbolic` / `Dimension::Bounded` variables.
3. `prove_memory_bounds` proves, per scope, that `sum(alloc sizes) <= budget` for
   **all** dimension assignments. Showing the negation (`sum > budget`) is
   *unsatisfiable* proves the scope safe; if it is *satisfiable*, the solver returns
   a concrete witness.

## API

```rust
use tpt_telos_uir_bridge::{prove_memory_bounds, MemoryLimits, ProofResult};
use tpt_uir_core::ir::Region;

let result = prove_memory_bounds(&region, &MemoryLimits::with_default(4096));
match result {
    ProofResult::Valid => { /* every scope within budget */ }
    ProofResult::Counterexample { scope, model, total_bytes, limit_bytes } => {
        // `model` binds the symbolic dimensions to a concrete overflow witness.
    }
    ProofResult::Inconclusive { reason } => { /* needs the `z3` feature */ }
}
```

`prove_tptuir_bytes` / `prove_tptuir_file` accept serialized `.tptuir` input.

## Engines

- **Default (Fourier-Motzkin):** tpt-telos' built-in SMT core, sound over integers,
  no external dependency. Decides linear allocation sizes (fixed dims, or one
  symbolic dim per tensor).
- **`z3` feature:** routes nonlinear sizes (a tensor with two symbolic dimensions,
  i.e. a product of symbolic variables) through the Z3 SMT solver for exact integer
  arithmetic.

## Features

- `uir` (default off): enables consumption of the sibling `tpt-uir` workspace and
  the tpt-telos solver core. The crate compiles to a thin stub without it so the
  default `cargo test --workspace` does not require the `tpt-uir` repo to be present.
- `z3`: exact nonlinear solving via the Z3 SMT solver (requires `libz3`).

## CLI

```sh
telos-uir-prove model.tptuir --default-limit 1048576 --scope weights 262144
# exit 0 = valid, 1 = counterexample (over budget), 2 = inconclusive / error
```

## Testing

```sh
cargo test -p tpt-telos-uir-bridge --features uir
cargo test -p tpt-telos-uir-bridge --features uir,z3   # if Z3 is installed
```
