# tpt-telos Playground

A zero-install, browser-based playground for tpt-telos. It runs the same
parser + SMT-style verifier as the `telos` CLI, compiled to WebAssembly
(`crates/out-telos-wasm`).

## What it does

- Type `.telos` source in the editor.
- **Verify** runs the Fourier–Motzkin verifier and prints each `ensures` /
  `requires` check plus any concrete counterexample the solver found.
- **Parse** shows the parsed module structure.

## Local development

Build the WASM package and serve the folder over HTTP (a `file://` URL cannot
load the `.wasm` due to CORS):

```sh
# one-time tool install
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
rustup target add wasm32-unknown-unknown

# build the bindings into ./pkg
wasm-pack build crates/out-telos-wasm --target web --out-dir ../playground/pkg

# serve
python3 -m http.server 8080   # then open http://localhost:8080
```

The `pkg/` directory is git-ignored; it is produced by CI and deployed to
GitHub Pages automatically (see `.github/workflows/playground.yml`).
