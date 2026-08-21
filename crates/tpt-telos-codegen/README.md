# tpt-telos-codegen

**Rust/Go dual-backend code generation and FFI bridge for tpt-telos.**

Part of the [tpt-telos](https://github.com/tpt-solutions/tpt-telos) compiler workspace.

## Overview

`tpt-telos-codegen` is the final emission stage of the tpt-telos pipeline. It consumes the verified
`FuncOutcome`s produced by `tpt-telos-agent` and emits human-readable, compilable source code.

- **`generate_program`** — emits a single self-contained Rust library file (structs, invariant
  `impl` methods, contract doc-comments, one `fn` per function).
- **`generate_project`** — assembles a full dual-backend project tree: a Rust crate, a Go module,
  and a C-ABI FFI bridge between them, with each module routed to Rust, Go, or Python by
  `tpt-telos-router`.
- **`python` module** — emits a Python backend for `@boundary(ml_training|python|jax)` modules:
  `@dataclass` structs with `satisfies_invariants()` and runtime `assert` guards for every contract;
  the `jax` flag additionally annotates fields with `jnp.int64`.
- **`proof` module** — generates `telos-proof.json` on every `build`/`project` run (SHA-256 of the
  source, per-function verification outcomes, a tamper-evident `manifest_hash`) and embeds it as
  `#[used] static TELOS_PROOF_MANIFEST` in the generated Rust binary; `verify_manifest` re-hashes a
  source file against a saved manifest to detect drift (`telos verify-manifest`).
- **eject hatch** — functions marked `@eject` are compiled to a trusted opaque `f_impl`/`fImpl` stub
  wrapped by a generated guard that enforces `requires`/`ensures` at runtime via `assert!`/`panic`.

## Usage

```rust
use tpt_telos_codegen::{generate_program, generate_project};

// Single Rust file
let rust_src = generate_program(&modules, &outcomes);
std::fs::write("output.rs", rust_src).unwrap();

// Full dual-backend project
let project = generate_project(&modules, &outcomes);
for file in &project.files {
    let path = out_dir.join(&file.path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, &file.contents).unwrap();
}
```

## Key API

| Item | Description |
|------|-------------|
| `generate_program(modules, outcomes) -> String` | Single Rust library source |
| `generate_project(modules, outcomes) -> Project` | Full Rust+Go(+Python) project tree |
| `Project` | Collection of `GeneratedFile { path, contents }` |
| `eject` module | Renders `@eject` functions as an opaque stub + contract-guarded wrapper (invoked automatically by `generate_program`/`generate_project`, not a public entry point) |
| `go` module | Go backend emitter |
| `python` module | Python/JAX backend emitter |
| `ffi` module | C-ABI FFI bridge generator |
| `proof::generate_manifest` / `proof::verify_manifest` | Cryptographic proof manifest write/verify |

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your option.
