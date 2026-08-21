# tpt-pantheon

A coherent, self-contained platform designed together from day one — not
integrated after the fact. One Rust workspace, one storage engine, with two
deliberate, written-down exceptions (Identity, vendored in Go; and the WASM
sandbox, a thin wrapper over `wasmtime`).

See [`spec.txt`](./spec.txt) for the full design document (Rev. 3) and
[`SPINE.md`](./SPINE.md) for the cross-cutting contracts every crate must obey.

## Layout

```
tpt-pantheon/
├── Cargo.toml                  # workspace root (Rust members only)
├── crates/                     # Rust crates (see spec §2)
│   ├── keystone/  telos/  appfront/   # vendored, unrenamed (Layer 1)
│   ├── tpt-pantheon-spine-*    # Layer 0 spine crates (this repo)
│   └── tpt-pantheon-*          # rewritten components, Layer 2–5
├── services/
│   └── identity/               # vendored Go service (NOT a Cargo member)
├── SPINE.md                    # shared contracts + boundary register
├── deny.toml                   # cargo-deny supply-chain config
├── justfile                    # single entry point: cargo + go
└── spec.txt
```

## Build & test

This repo uses [`just`](https://github.com/casey/just) as the single task
runner for both the Rust workspace and the Go Identity service:

```sh
just build        # cargo build --workspace
just test         # cargo test  --workspace
just build-identity
just test-identity
just deny         # cargo-deny + §5.3 sandbox-boundary check
just ci           # full pipeline (fmt, lint, deny, build, test)
```

If `just` is unavailable, the equivalent `cargo` / `go` commands are shown in
each recipe. CI runs the same recipes on every push/PR (see
`.github/workflows/ci.yml`).

## Licensing

Dual-licensed under [MIT](./LICENSE-MIT) or
[Apache-2.0](./LICENSE-APACHE), at your option. Copyright TPT Solutions.

## Status

Phased build per `todo.md`. Layers are gated: a layer does not start until the
layer below ships a real, working, tested proof in this monorepo.
