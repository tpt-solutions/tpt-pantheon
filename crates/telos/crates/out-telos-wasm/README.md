# out-telos-wasm

WASM bindings for the `tpt-telos-parser` and `tpt-telos-verifier` crates, intended for use in a
zero-install browser-based playground where users can type `.telos` source and see verification
results in real time.

## Prerequisites

Install [`wasm-pack`](https://rustwasm.github.io/wasm-pack/):

```sh
cargo install wasm-pack
```

## Building

```sh
wasm-pack build crates/out-telos-wasm --target web
```

This produces a `pkg/` directory containing the generated JavaScript bindings and the `.wasm` file,
ready to be served as static assets.

> **Note:** This crate is excluded from the normal `cargo build --workspace` test run for the WASM
> target, but it does compile as a native `rlib` as part of workspace builds so that CI catches
> type errors. Building for the `wasm32-unknown-unknown` target requires `wasm-pack`.

## Exported functions

### `parse(source: string): string`

Parses `.telos` source and returns a JSON string.

**Success:**
```json
{
  "ok": true,
  "modules": [
    { "name": "Wallet", "functions": ["deposit", "withdraw"], "invariants": ["T"] }
  ]
}
```

**Error:**
```json
{ "ok": false, "error": "parse error message" }
```

### `verify(source: string): string`

Parses `.telos` source, extracts IR constraints, and runs the Fourier-Motzkin verifier over all
functions. Returns a JSON string.

**Success:**
```json
{
  "ok": true,
  "passed": true,
  "functions": [
    {
      "name": "withdraw",
      "passed": true,
      "checks": [
        {
          "description": "ensures balance >= 0",
          "passed": true,
          "is_ensures": true,
          "is_approximation": false
        }
      ]
    }
  ]
}
```

**Error:**
```json
{ "ok": false, "error": "..." }
```

When a check fails, the response includes a `counterexample` object mapping variable names to
concrete integer values that violate the contract.
