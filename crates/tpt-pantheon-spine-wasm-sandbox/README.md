# tpt-pantheon-spine-wasm-sandbox

## Crate gate

1. **What does this solve that nothing else does?** It is the single, mandatory
   policy layer that translates a `SandboxPermissions` request into correct
   `wasmtime` / `cap-std` configuration, so sandboxing policy lives in exactly
   one reviewed crate instead of being spread across every consumer. The closest
   existing thing is hand-rolled `wasmtime` setup per consumer — which would
   duplicate and drift.
2. **Which spine contracts does it satisfy, or except itself from?** Satisfies
   §5.3 (sandbox) by definition — it is the one crate permitted a direct
   `wasmtime` dependency. Excepted from §5.1 (storage) and §5.2 (audit) as a pure
   policy→config translation layer with no persistent state of its own —
   registered in SPINE.md §5 Boundary Register.
3. **Is this Pantheon, or a different program?** Pantheon-specific spine
   infrastructure; the policy translation (not a general sandbox product) is the
   point.

---

Thin WASM sandbox policy layer for the Pantheon platform.

Per [`SPINE.md`](../../SPINE.md) §5.3 this crate owns **none** of the actual WASM
execution, memory isolation, or fuel accounting. Its entire job is to translate
a `SandboxPermissions` struct into correct configuration of `wasmtime` /
`wasmtime-wasi` / `cap-std`.

The translation is split in two:

- `SandboxPermissions::resolve` (always available, fully tested) validates the
  requested permissions and produces a backend-neutral `ResolvedConfig`. This is
  the policy core and the only part compiled in the default build.
- With the `wasmtime` feature, the `wasm` module consumes a `ResolvedConfig` to
  build a `wasmtime::Store` (fuel / memory / epoch), a `cap-std`-backed
  `WasiCtx` (only preopened paths, no ambient authority), and to register host
  functions on a `Linker` **conditionally** — an ungranted capability gets no
  import in the instantiated module's environment, rather than a runtime-checked
  denial.

Keeping the binding behind the `wasmtime` feature lets the default workspace
build stay light and keeps this crate a thin policy layer (the §5.3 drift
signal: if this crate exceeds a few hundred lines of config-translation, it has
drifted).

## Layout

- `SandboxPermissions` / `ResolvedConfig` — the request and its resolved form.
- `HostFn` — individual host capabilities (an ungranted one is simply absent).
- `resolve` — pure, backend-free policy validation.
- `wasm` (feature `wasmtime`) — the thin `wasmtime` / `cap-std` binding.
