# Editor integration

`tpt-telos-lsp` is a standard, editor-agnostic LSP server (JSON-RPC 2.0 over stdio with
`Content-Length` framing), launched via `telos lsp`. It provides diagnostics (with
counterexamples), hover, go-to-definition, find-references, completion, inlay hints,
formatting, and quick-fix code actions for `.telos` files.

A ready-made VS Code extension is available in [`vscode-telos/`](../vscode-telos/README.md).
This page covers setting up the same server manually in Neovim and Helix.

Either way, the `telos` binary must be on your `PATH`:

```sh
cargo install --path crates/tpt-telos-cli
```

## Neovim

Requires Neovim 0.11+ (built-in `vim.lsp.config`) and a filetype association for `.telos`.

```lua
-- ~/.config/nvim/lua/config/telos.lua (or inline in init.lua)

vim.filetype.add({ extension = { telos = "telos" } })

vim.lsp.config.telos = {
  cmd = { "telos", "lsp" },
  filetypes = { "telos" },
  root_markers = { ".git", "Cargo.toml" },
}
vim.lsp.enable("telos")
```

On older Neovim (< 0.11) with `nvim-lspconfig`, register it as a custom server instead:

```lua
local configs = require("lspconfig.configs")
if not configs.telos then
  configs.telos = {
    default_config = {
      cmd = { "telos", "lsp" },
      filetypes = { "telos" },
      root_dir = require("lspconfig.util").root_pattern(".git", "Cargo.toml"),
    },
  }
end
require("lspconfig").telos.setup({})
```

## Helix

Add both a language server entry and a language entry to `~/.config/helix/languages.toml`:

```toml
[language-server.telos-lsp]
command = "telos"
args = ["lsp"]

[[language]]
name = "telos"
scope = "source.telos"
file-types = ["telos"]
roots = [".git", "Cargo.toml"]
language-servers = ["telos-lsp"]
```

Helix has no built-in `.telos` syntax highlighting grammar (tree-sitter), so highlighting
will be plain text; diagnostics, hover, completion, and formatting via the LSP still work.

## Verifying the setup

Open any file under [`examples/`](../examples/README.md) (e.g. `examples/wallet.telos`).
You should see diagnostics on save/change, and `gq`/your editor's format keybinding should
invoke `telos`'s formatter. If nothing happens, run `telos lsp` by hand from a terminal to
confirm the binary starts without error, and check your editor's LSP log for the exact
command it tried to launch.
