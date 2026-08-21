# tpt-telos-router

**Context router that classifies tpt-telos modules to Rust or Go backends.**

Part of the [tpt-telos](https://github.com/tpt-solutions/tpt-telos) compiler workspace.

## Overview

`tpt-telos-router` is a pure classification crate with no side effects. It reads the `@boundary(...)`
attribute flags on a module or function and decides whether the code should be emitted to the Rust,
Go, or Python backend. The result is consumed by `tpt-telos-agent` (to record the target in each
`FuncOutcome`) and by `tpt-telos-codegen` (to drive Rust/Go/Python emission and FFI bridge
generation).

**Routing rules:**

| Flag | Target |
|------|--------|
| `cpu_bound`, `zero_allocation`, `crypto`, `real_time` | Rust |
| `network_io`, `high_concurrency`, `distributed`, `high_latency` | Go |
| `ml_training`, `python`, `jax` | Python (JAX/NumPy annotations) |

Python beats everything else (explicit opt-in for ML workloads); otherwise Go wins when any Go flag
is present; Rust is the default when no flags match.

It also reads `@state(persistent)` / `@state(ephemeral)` to derive a `StorageClass` (persistent
structs get `Serialize`/`Deserialize` derives in Rust and JSON tags in Go; ephemeral is stack-only
and the default), and `route_checked` emits a `RoutingDiagnostic` — `RealTimeGoConflict` /
`ZeroAllocGoConflict` — when a `real_time`/`zero_allocation` module gets routed to Go despite its
non-deterministic GC. The CLI's `--strict-rt` flag turns these into a non-zero exit.

## Usage

```rust
use tpt_telos_parser::{parse, ast::Item};
use tpt_telos_router::route;

let modules = parse(src).unwrap();
for module in &modules {
    let r = route(&module.attributes);
    println!("module {} -> {} ({})", module.name, r.target.as_str(), r.reason);
    for item in &module.items {
        if let Item::Func(f) = item {
            let r = route(&f.attributes);
            println!("  {} -> {} ({})", f.name, r.target.as_str(), r.reason);
        }
    }
}
```

## Key API

| Item | Description |
|------|--------------|
| `route(attrs: &[Attribute]) -> Route` | Classify a module's or function's attributes |
| `route_checked(attrs, module_name) -> (Route, Vec<RoutingDiagnostic>)` | Same, plus real-time/Go conflict warnings |
| `Target` | `Rust`, `Go`, or `Python`; `.as_str()` returns `"rust"` / `"go"` / `"python"` |
| `Route` | `{ target: Target, storage: StorageClass, reason: String }`; `.is_rust() -> bool` |
| `StorageClass` | `Persistent` or `Ephemeral` (default), from `@state(...)` |
| `RoutingDiagnostic` / `DiagnosticKind` | `RealTimeGoConflict` / `ZeroAllocGoConflict` warnings |

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your option.
