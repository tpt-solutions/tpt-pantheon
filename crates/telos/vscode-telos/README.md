# telos VS Code Extension

Language support for [tpt-telos](https://github.com/tpt-solutions/tpt-telos) `.telos` source files.

For Neovim or Helix instead of VS Code, see [`docs/editors.md`](../docs/editors.md).

## Features

- Syntax highlighting for `.telos` files (keywords, attributes, operators, types)
- Real-time diagnostics: parse errors and unsatisfied contracts, via the `telos lsp` language server
- Hover information: function signature, routing target, verification status, and contracts
- Code actions: quick-fix `requires` suggestions derived from counterexample witnesses
- Document formatting: canonically reformat the current file (`Shift+Alt+F`)

## Prerequisites

The `telos` binary must be available on your PATH (or configured via `telos.serverPath`).

Build it from source:

```sh
cargo install --path crates/tpt-telos-cli
```

Or use the pre-built binary from the project releases.

## Installation

### From a VSIX package

```sh
vsce package          # builds telos-0.1.1.vsix
code --install-extension telos-0.1.0.vsix
```

### From source (development)

1. Open `vscode-telos/` in VS Code.
2. Run `npm install` then `npm run compile`.
3. Press `F5` to launch an Extension Development Host.

## Configuration

| Setting | Default | Description |
|---|---|---|
| `telos.serverPath` | `"telos"` | Path to the `telos` binary. Override if the binary is not on PATH. |

## License

Apache-2.0 OR MIT — same as the tpt-telos project.
