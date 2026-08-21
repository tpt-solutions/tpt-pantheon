# tpt-telos Architecture

## Pipeline

```
Source (.telos) → Parser → AST → IR/Constraint Extraction → QF_LRA Verifier
                                                           ↓
                                          Agentic Transpiler (Generate → Verify → Counter-example → Rewrite)
                                                           ↓
                                              Context Router → Rust / Go / Python Backend
                                                           ↓
                                              Code Generation → FFI Bridge (if dual-backend)
                                                           ↓
                                              Eject Hatch → LSP Server
```

## Crate Map

| Crate | Role |
|-------|------|
| `tpt-telos-parser` | Lexer, parser, AST definitions |
| `tpt-telos-ir` | AST → IR lowering, QF_LRA constraint extraction |
| `tpt-telos-verifier` | Self-contained Fourier-Motzkin SMT solver (no external Z3) |
| `tpt-telos-agent` | `CodeAgent` trait; `StaticAgent` (offline) + `LlmAgent` (behind `llm` feature) |
| `tpt-telos-router` | `@boundary(...)` classification → Rust / Go / Python target |
| `tpt-telos-codegen` | Rust, Go, Python backends; FFI bridge; eject hatch; proof manifest |
| `tpt-telos-lsp` | JSON-RPC 2.0 language server over stdio |
| `tpt-telos` | CLI binary (`telos`) |
| `tpt-telos-sdk` | Programmatic orchestration API (one-call pipeline, hints, build) |
| `out-telos-wasm` | WASM bindings (`parser` + `verifier`) for the browser playground |

## Go GC Determinism

### Background

Go uses a concurrent, tri-color mark-and-sweep garbage collector. While Go's GC has
sub-millisecond pause times in practice, it is **non-deterministic**: the exact timing
and duration of GC pauses depend on heap pressure, allocation patterns, and runtime
scheduling. This makes Go unsuitable for hard real-time systems where worst-case
execution time (WCET) must be bounded.

### Safe Go-Routed Modules

The following `@boundary(...)` flags route to the Go backend and are **safe** for
soft real-time and general server workloads:

| Flag | Use Case | GC Impact |
|------|----------|-----------|
| `network_io` | HTTP/gRPC handlers, WebSocket servers | Low — allocation-heavy but latency-tolerant |
| `high_concurrency` | Goroutine pools, fan-out/fan-in | Low — GC scales with heap, not goroutine count |
| `distributed` | Distributed consensus, RPC clients | Low — network latency dominates GC pauses |
| `high_latency` | Background jobs, batch processing | Negligible — latency budget is seconds+ |

These modules are safe because their latency budgets are measured in milliseconds or
seconds, far exceeding typical GC pause times (< 1ms on modern hardware with heaps
under 1 GB).

### Unsafe Go-Routed Modules

The following combinations route to Go but are **unsafe** for hard real-time:

| Flag Combination | Why Unsafe |
|-------------------|------------|
| `real_time` + `network_io` | `real_time` implies WCET requirements; Go GC violates them |
| `zero_allocation` + `high_concurrency` | `zero_allocation` implies no GC; Go always allocates via GC |
| `real_time` + any Go flag | `real_time` requires deterministic execution; Go GC is non-deterministic |

When these combinations are detected, the router emits:
- `WARNING [real_time_go_conflict]` — module has `real_time` but is routed to Go
- `WARNING [zero_alloc_go_conflict]` — module has `zero_allocation` but is routed to Go

Use `--strict-rt` to promote these warnings to hard errors (non-zero exit).

### Hard Real-Time Guidelines

For FADEC (Full Authority Digital Engine Control), PREEMPT_RT, and other hard
real-time targets:

1. **Always use the Rust backend** — Rust has no GC; execution time is bounded.
2. **Use `@boundary(real_time)` or `@boundary(zero_allocation)`** — these route
   to Rust by default.
3. **Never combine `real_time` with Go-bound flags** — the router will warn or
   error with `--strict-rt`.
4. **Avoid heap allocation in hot paths** — use stack-allocated data structures
   and `&mut` references.

### Reference

- Go GC Guide: https://tip.golang.org/doc/gc-guide
- Go Runtime: https://go.dev/s/go15gc
- PREEMPT_RT: https://wiki.linuxfoundation.org/realtime/documentation/howto/tools/rt-tests
