# Getting Started with tpt-telos

This tutorial takes you from zero to a verified, compiling artifact. You will write a
small `.telos` module, prove its contracts, look at a failing case, and compile it to
Rust (and to a dual Rust+Go project).

> Prerequisites: a Rust toolchain (`cargo`/`rustc` on `PATH`). For the dual-backend
> section you also need `go` and `gofmt` (optional — the source is still generated
> without them). For exact nonlinear arithmetic you need Z3 and a build with
> `--features z3` (optional).

---

## 1. Install

From crates.io:

```sh
cargo install tpt-telos
```

Or build the workspace:

```sh
git clone https://github.com/tpt-solutions/tpt-telos
cd tpt-telos
cargo build --release -p tpt-telos
```

Verify the install:

```sh
telos --help
```

---

## 2. Scaffold your first module

The fastest way to start is `telos init`, which writes a starter `.telos` file:

```sh
telos init --module Wallet --out wallet.telos
```

This produces a `cpu_bound` module with a `Counter` invariant and `increment`/
`decrement` functions. (Templates: `simple` (default), `dual-backend`, `eject`,
`real-time`, `python-ml`, `cross-module`.)

You can also scaffold a whole project directory with `telos new`:

```sh
telos new --name MyProject --template cross-module
```

That creates `MyProject/MyProject.telos` plus a `README.md` with next steps, ready to
feed straight into `telos project --check`.

---

## 3. Read the annotated example

The repository ships `examples/START-HERE.telos`, a fully-commented single-module
walkthrough of every core construct. Open it and read it top to bottom — it explains
`@boundary`, `invariant`, `requires`/`ensures`/`old`, and `mutate state` inline.

The whole example in brief:

```telos
@boundary(cpu_bound)
module Wallet {
    invariant Account {
        balance >= 0
    }

    func deposit(acc: Account, amount: Int)
        requires amount > 0
        requires acc.balance >= 0
        ensures acc.balance == old(acc.balance) + amount
    {
        mutate state {
            acc.balance += amount
        }
    }

    func withdraw(acc: Account, amount: Int)
        requires amount > 0
        requires acc.balance >= amount
        ensures acc.balance == old(acc.balance) - amount
    {
        mutate state {
            acc.balance -= amount
        }
    }
}
```

The key idea: `mutate state` is the *only* place state changes, and the verifier proves
each `ensures` against the assignments inside it using `old(...)` to refer to the
pre-state.

---

## 4. Verify the contracts

```sh
telos verify wallet.telos
```

You should see output like:

```
Verifying wallet.telos

  function deposit:
    [PASS] requires amount > 0
    [PASS] requires acc.balance >= 0
    [PASS] ensures acc.balance == old(acc.balance) + amount
    => PASS

  function withdraw:
    [PASS] requires amount > 0
    [PASS] requires acc.balance >= amount
    [PASS] ensures acc.balance == old(acc.balance) - amount
    => PASS

RESULT: all constraints satisfied.
```

`telos verify` exits non-zero when any constraint fails (useful as a CI gate).

### Machine-readable output

For CI or editors, add `--json`:

```sh
telos verify wallet.telos --json
```

### Watch mode

Re-verify automatically on save:

```sh
telos verify wallet.telos --watch
```

---

## 5. See a counterexample

Open `examples/broken.telos` — it intentionally adds instead of subtracting, so the
`ensures` is violated. Run:

```sh
telos verify examples/broken.telos
```

The output shows a `FAIL` with a `counterexample` (a concrete assignment that breaks the
clause) and a `^` caret pointing at the offending source span. Counterexamples are how
you debug contracts.

---

## 6. Transpile to Rust

The agentic transpiler turns your module into a self-contained Rust file. By default it
runs the fully offline `StaticAgent` (no network, deterministic):

```sh
telos transpile wallet.telos --out wallet.rs
cat wallet.rs
```

To use a real LLM backend instead, build with the `llm` feature and pass `--llm` (needs
`TELAS_LLM_KEY` / `TELAS_LLM_PROVIDER` at runtime):

```sh
cargo run -p tpt-telos --features llm -- transpile wallet.telos --llm
```

---

## 7. Build a verified, compiling crate

`telos build` transpiles *and* compiles the generated Rust with `cargo`, writing a
proof manifest (`telos-proof.json`) alongside it:

```sh
telos build wallet.telos --out-dir ./gen
```

It exits non-zero if the generated Rust fails to compile, or (with `--json`) reports
`all_verified`.

### Detect drift with the manifest

Later, re-hash the source against the recorded manifest to detect tampering/drift:

```sh
telos verify-manifest gen/telos-proof.json wallet.telos
```

---

## 8. Generate a dual Rust+Go project (FFI bridge)

A single `.telos` file can contain modules routed to different backends. The compiler
generates the FFI bridge for you with zero hand-written glue. See
`examples/microservice.telos` for a `cpu_bound` Rust `Ledger` plus a `network_io` Go
`GatewayApi`.

```sh
telos project examples/microservice.telos --out-dir ./gen-project --check
```

`--check` compiles the Rust crate (`cargo`) and vets the Go package (`go build`) to
prove both backends build. `go build` skips cgo files, so the Go side is validated with
`gofmt -l` rather than a full `go build`.

To fail the build if a `real_time`/`zero_allocation` module is accidentally routed to Go
(GC is non-deterministic):

```sh
telos project examples/microservice.telos --check --strict-rt
```

---

## 9. The eject hatch

Sometimes you need hand-tuned code (e.g. calling a native library). Mark the function
`@eject`; the compiler emits a trusted opaque block guarded by its contract:

```sh
telos eject examples/eject.telos --func withdraw
```

---

## 10. Where to go next

- **Language reference:** [`docs/LANGUAGE.md`](LANGUAGE.md) — every type, clause, and
  expression.
- **CLI reference:** [`docs/CLI.md`](CLI.md) — all subcommands, flags, and exit codes.
- **SDK / integration:** [`docs/SDK.md`](SDK.md) — drive tpt-telos from your own Rust
  code.
- **Runnable fixtures:** `examples/README.md` (try `wallet.telos`, `real_time.telos`,
  `crypto.telos`, `distributed.telos`).
- **Editor support:** `telos lsp` provides diagnostics, hover, quick-fixes, and
  `telos/verify` + `telos/eject` code actions over JSON-RPC 2.0 stdio.
