# tpt-telos-lsp

**JSON-RPC 2.0 language server for tpt-telos — real-time in-editor verification.**

Part of the [tpt-telos](https://github.com/tpt-solutions/tpt-telos) compiler workspace.

## Overview

`tpt-telos-lsp` is a standard LSP server that provides real-time feedback for `.telos` files. It
speaks JSON-RPC 2.0 over stdio with `Content-Length` framing (compatible with VS Code, Neovim,
Helix, and any other LSP-capable editor). A ready-made VS Code extension lives in
[`vscode-telos/`](../../vscode-telos/README.md); for other editors see
[`docs/editors.md`](../../docs/editors.md).

On every document change it runs the full pipeline — parse → IR → verify → route → agent → codegen
— and surfaces the results as:

- **Diagnostics** — parse errors and unsatisfied `requires`/`ensures` contracts, with source spans
  and, where the solver found one, a concrete counterexample. Ejected functions are reported as an
  informational note (trusted, boundary-guarded) rather than an error.
- **Hover** — function signature, routing target (Rust/Go), contracts, and verification status.
- **Code actions** — a `quickfix` per failing check with a counterexample, inserting a `requires
  !(...)` clause that excludes the concrete witness the solver found (a starting point to refine,
  not a guaranteed fix).
- **`telos/verify`** — custom request returning a full verification summary for the open file.
- **`telos/eject`** — custom request returning a preview of the ejected Rust/Go with contract guards.

The `Server` message handler is decoupled from stdio I/O for unit testing without spawning a process.

## Usage

Launch via the `telos` CLI:

```sh
telos lsp
```

Or embed directly:

```rust
use tpt_telos_lsp::{Server, run_stdio};

// Standalone stdio server
run_stdio().unwrap();

// Embedded / testable server
let mut server = Server::new();
let responses = server.handle(&serde_json::json!({
    "jsonrpc": "2.0", "id": 1,
    "method": "initialize",
    "params": { "capabilities": {} }
}));
```

## Handled JSON-RPC methods

| Method | Description |
|--------|-------------|
| `initialize` / `initialized` / `shutdown` / `exit` | Lifecycle |
| `textDocument/didOpen`, `didChange`, `didSave`, `didClose` | Document sync |
| `textDocument/hover` | Hover info |
| `textDocument/codeAction` | Quick-fix `requires` suggestions from failing checks' counterexamples |
| `textDocument/formatting` | Canonical reformatting (also exposed standalone as `telos fmt`) |
| `textDocument/definition` / `references` | Go-to-definition / find-references, indexed over currently open documents |
| `textDocument/completion` | Symbol completion, indexed over currently open documents |
| `textDocument/inlayHint` | Inferred type/routing hints |
| `telos/verify` | Full verification summary (custom) |
| `telos/eject` | Ejected code preview (custom) |

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your option.
