# examples/

Fixture `.telos` files used by integration tests and to demonstrate language features.

## Showcase: the highest-use-case set

These six files are the fastest way to see what tpt-telos is *for*. Each verifies
green end-to-end and is wired into a `tpt-telos-verifier` integration test. Start
with `wallet.telos` and `microservice.telos`, then read the domain-focused demos.

| # | File | Demonstrates |
|---|------|--------------|
| 1 | `wallet.telos` | Verified financial **state machine**: `@boundary(cpu_bound)` module with a non-negative balance invariant and a `transfer` proven correct via `old(...)`. |
| 2 | `real_time.telos` | **Hard-real-time / embedded control**: `@boundary(real_time, zero_allocation)` → Rust controller with a verified safe integer range (no GC, no `real_time_go_conflict`). |
| 3 | `microservice.telos` | **Dual-backend + automatic FFI**: `@boundary(cpu_bound, zero_allocation)` Ledger → Rust + `@boundary(network_io, high_concurrency)` GatewayApi → Go, bridged with zero hand-written glue. |
| 4 | `crypto.telos` | **Cryptographic / sensitive-op safety**: `@boundary(crypto)` → Rust secret store whose `value >= 0` invariant is preserved across `consume`/`refresh`. |
| 5 | `cryptocurrency.telos` | **Coin conservation**: a `balance_a + balance_b == 1000000` invariant preserved by a transfer (no mint/burn), distinct from `wallet.telos`'s single-balance invariant. |
| 6 | `distributed.telos` | **Distributed / network service**: `@boundary(distributed)` → Go replicated-log coordinator with a monotonically growing `commit_index`. |

## All fixtures

| File | Demonstrates |
|------|--------------|
| `START-HERE.telos` | **Start here.** An annotated, fully-commented single-module walkthrough of every core construct (module, `@boundary`, invariant, `requires`/`ensures`/`old`, `mutate state`); verifies and is referenced from the README. |
| `wallet.telos` | Showcase #1: `@boundary(cpu_bound)` module with a non-negative balance invariant, `transfer` with `requires`/`ensures` and `old(...)`. |
| `broken.telos` | Core fail case: intentionally wrong mutation body (adds instead of subtracts) so `telos verify` exits non-zero with a counterexample. |
| `nested.telos` | Nested arithmetic in `ensures` (`old(c.value) * 2 + a + b`), `&&`-flattened `requires`, and scalar equality post-conditions. |
| `microservice.telos` | Showcase #3: dual-backend compilation with the automatic FFI bridge. |
| `eject.telos` | The eject hatch: `@eject(rust)` marks a function as a trusted opaque block (`*_impl`) wrapped by a generated contract guard enforcing `requires`/`ensures` at runtime. |
| `float.telos` | `Float32`/`Float64` type parsing and codegen; documents that the IR currently treats floats as integers (QF_LRA). |
| `array_test.telos` | `[T; N]` fixed-array type in function signatures; shows parser/codegen support and documents the absence of IR length-constraint extraction. |
| `disjunction.telos` | `requires a \|\| b` DNF expansion into independent verification sub-problems; `ensures` disjunction satisfied when at least one branch holds. |
| `intent.telos` | Body-elided (intent-only) functions: the compiler synthesizes implementations from `ensures` contracts alone, then verifies them. |
| `interval.telos` | Nonlinear interval bounding: `ensures x * y <= 50` over-approximated via interval arithmetic when both variables have bounded `requires`; result tagged `[interval-bounded]`. |
| `cross_module.telos` | Cross-module invariant references: `Operations` uses `Counter` declared in `Core`; demonstrates the global type-resolution pass in `tpt-telos-ir`. |
| `physics.telos` | `@boundary(ml_training)` routed to the Python/JAX backend; demonstrates the Python codegen target with `@dataclass` structs and runtime `assert` guards. |
| `overflow.telos` | Integer bounds at the `i64` extremes; exercises the solver's checked-`i128` arithmetic so large magnitudes degrade conservatively (no panic, no spurious contradiction) instead of overflowing. |
