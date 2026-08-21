# tpt-telos CLI Reference

The `telos` binary is the compiler frontend. This page documents every subcommand, its
flags, and its exit codes. For a walkthrough, see [`docs/TUTORIAL.md`](TUTORIAL.md).

General notes:

- The agentic transpiler runs the offline `StaticAgent` by default. Pass `--llm` (only
  available when built with `--features llm`) to use a real LLM backend. Without the
  feature, `--llm` errors with "the `llm` agent requires building telos with the `llm`
  feature".
- `--json` switches most commands to machine-readable JSON on stdout (for CI/editors).
- Watch mode (`--watch`) re-runs on any `.telos` change in the file's directory after a
  short debounce.

---

## `telos init`

Scaffold a starter `.telos` module file.

| Flag         | Default     | Description |
|--------------|-------------|-------------|
| `--module`   | `MyModule`  | Module name to generate. |
| `--out`      | `<module>.telos` | Output file path. |
| `--template` | `simple`    | One of: `simple`, `dual-backend`, `eject`, `real-time`, `python-ml`, `cross-module`. |

```sh
telos init --module Wallet --out wallet.telos
telos init --module Svc --template dual-backend --out svc.telos
```

---

## `telos new`

Scaffold a project directory (a `<name>.telos` module plus a `README.md`) ready to pass
to `telos project --check`.

| Flag         | Default     | Description |
|--------------|-------------|-------------|
| `--name`     | `MyProject` | Project (and module) name. |
| `--out-dir`  | `<name>`    | Output directory. |
| `--template` | `simple`    | As in `init`. |

```sh
telos new --name MyProject --template cross-module
```

---

## `telos parse <file>`

Parse a `.telos` file and print its AST (human-readable, or JSON).

| Flag      | Description |
|-----------|-------------|
| `--json`  | Emit machine-readable JSON (modules, functions, params, requires/ensures). |

```sh
telos parse examples/wallet.telos
telos parse examples/wallet.telos --json
```

---

## `telos verify <file>`

Run formal verification (requires/ensures → QF_LRA) and print a pass/fail report. Exits
**non-zero** when any constraint fails.

| Flag       | Default            | Description |
|------------|--------------------|-------------|
| `--solver` | `fourier-motzkin`  | `fourier-motzkin` (built-in) or `z3` (exact nonlinear arithmetic; requires `--features z3`). |
| `--json`   | —                  | Machine-readable JSON. |
| `--watch`  | —                  | Re-verify on `.telos` change. |

```sh
telos verify examples/wallet.telos
telos verify examples/wallet.telos --json
telos verify examples/wallet.telos --watch
telos verify examples/wallet.telos --solver z3
```

Failure output includes a `counterexample` (concrete assignment) and a `^` caret at the
offending source span. Some failures may be solver limitations (FM is incomplete for
nonlinear arithmetic); try `--solver z3` for exact results.

---

## `telos transpile <file>`

Run the agentic transpiler and print (or write) the generated Rust.

| Flag    | Description |
|---------|-------------|
| `--llm` | Use the LLM-backed agent instead of `StaticAgent`. |
| `--out` | Write generated Rust to this path instead of stdout. |

```sh
telos transpile examples/wallet.telos --out wallet.rs
telos transpile examples/intent.telos --llm
```

---

## `telos build <file>`

Transpile *and* compile the generated Rust (requires `cargo`/`rustc`). Writes a
`telos-proof.json` attestation manifest alongside the crate. Exits non-zero if the
generated Rust fails to compile.

| Flag       | Default     | Description |
|------------|-------------|-------------|
| `--out-dir`| `gen`       | Directory for the generated crate. |
| `--llm`    | —           | Use the LLM-backed agent. |
| `--solver` | `fourier-motzkin` | Solver backend. |
| `--json`   | —           | Machine-readable JSON (includes `proof_hash`). |
| `--watch`  | —           | Re-build on `.telos` change. |

```sh
telos build examples/wallet.telos --out-dir ./gen
```

---

## `telos project <file>`

Generate a dual-backend (Rust + Go) project with an automatic FFI bridge.

| Flag         | Default      | Description |
|--------------|--------------|-------------|
| `--out-dir`  | `gen-project`| Directory for the generated project. |
| `--llm`      | —            | Use the LLM-backed agent. |
| `--check`    | —            | After generating, compile the Rust crate (`cargo`) and vet the Go package (`go`) to prove both backends build. |
| `--strict-rt`| —            | Exit non-zero if any `real_time`/`zero_allocation` module is routed to Go (GC conflict). |
| `--json`     | —            | Machine-readable JSON. |
| `--watch`    | —            | Re-generate/check on `.telos` change. |

```sh
telos project examples/microservice.telos --out-dir ./gen-project --check
telos project examples/microservice.telos --check --strict-rt
```

Note: `go build` skips cgo files, so the Go side is validated with `gofmt -l` rather
than a full `go build`.

---

## `telos eject <file>`

Eject functions to raw Rust/Go opaque blocks guarded by their contracts.

| Flag         | Default   | Description |
|--------------|-----------|-------------|
| `--out-dir`  | `ejected` | Directory for the ejected project. |
| `--func`     | (all)     | Eject only this function (by name). Default: every function. |
| `--llm`      | —         | Use the LLM-backed agent. |
| `--strict-rt`| —         | Exit non-zero on a real-time/zero-allocation → Go routing conflict. |
| `--json`     | —         | Machine-readable JSON. |

```sh
telos eject examples/microservice.telos --func withdraw
```

---

## `telos verify-manifest <manifest> <source>`

Verify a proof manifest (`telos-proof.json`) against the current source file by
re-hashing the source. Exits non-zero on mismatch/drift.

```sh
telos verify-manifest gen/telos-proof.json examples/wallet.telos
```

---

## `telos completions <shell>`

Print shell completions (bash, zsh, fish, powershell, elvish) and exit.

```sh
telos completions bash > /etc/bash_completion.d/telos
```

---

## `telos fmt <file>`

Reformat a `.telos` file canonically (reuses the LSP's `format_source`).

| Flag      | Description |
|-----------|-------------|
| `--check` | Check formatting without writing; exit non-zero if changes are needed. |
| `--stdout`| Print the formatted output to stdout instead of writing back. |

```sh
telos fmt examples/wallet.telos --check
telos fmt examples/wallet.telos --stdout
```

---

## `telos doctor`

Check for the optional external tools some commands shell out to (`go`, `gofmt`,
`--solver z3`). Reports what's available upfront so missing tools are diagnosed early.

| Flag    | Description |
|---------|-------------|
| `--json`| Machine-readable JSON. |

```sh
telos doctor
telos doctor --json
```

---

## `telos lsp`

Run the language server (JSON-RPC 2.0 over stdio) for IDE integration. Provides
diagnostics, hover, quick-fix code actions, and `telos/verify` + `telos/eject` custom
methods. No arguments.

```sh
telos lsp
```

---

## Exit codes

| Command | `0` | non-zero |
|---------|-----|----------|
| `verify` / `build` / `project` | all constraints satisfied / compiled | a constraint failed, or (build/project) the generated code did not compile. |
| `fmt`   | file already formatted (or reformatted) | `--check` found it needs reformatting. |
| `project --strict-rt` / `eject --strict-rt` | no routing conflict | a `real_time`/`zero_allocation` module was routed to Go. |
| `verify-manifest` | hash matches | mismatch / drift. |
| others | success | a pipeline error (parse, transpile, codegen, IO). |
