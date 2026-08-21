# SPINE — Shared Contracts & Boundary Register

This document is the single source of truth for the cross-cutting contracts
every crate in `tpt-pantheon` must obey, and the register of deliberate
exceptions to "one workspace, one language, one storage engine." It is the
materialized form of the spine contracts in `spec.txt` §5, and is enforced by
CI (see `ci/check-sandbox-boundary.sh` and `deny.toml`).

Every new crate MUST be reconciled against this file before it ships. If a
crate needs an exception, add it to the Boundary Register below with a written,
technical reason — silent drift is forbidden (spec §3.1).

---

## 1. Storage contract (§5.1)

- **Rule:** Every Rust crate in `crates/` either depends on `tpt-keystone-sdk`,
  or is listed in the Boundary Register (§4) with a stated reason.
- Crates that are pure policy/translation layers (e.g. the spine crates) are
  registered by default as "no persistent state."
- `services/identity` (Go, outside the Cargo workspace) is registered by
  default per §1 and needs no separate justification.

## 2. Audit contract (§5.2)

- **Rule:** Any crate mutating persistent state depends on
  `tpt-pantheon-spine-audit-log` and calls `AuditSink::append` in its write
  path.
- **CI check:** any crate with a Keystone write dependency must also list
  `tpt-pantheon-spine-audit-log` as a dependency, or be explicitly exempted in
  this file with a reason (e.g. pure read-only services).

## 3. Sandbox contract (§5.3)

- `tpt-pantheon-spine-wasm-sandbox` is, and remains, a **thin policy layer**:
  - Takes a `SandboxPermissions` struct (network / filesystem / Keystone /
    secrets / fuel / memory / timeout).
  - Translates it into `wasmtime::Store` config (fuel, memory limit, epoch
    timeout), a `cap-std`-backed `WasiCtx` (only granted paths preopened, no
    ambient authority), and conditional host-function registration on the
    `Linker`.
  - Owns none of the actual WASM execution, memory isolation, or fuel
    accounting — `wasmtime` / `wasmtime-wasi` / `cap-std` do that.
- **Supply-chain ban:** `cargo deny` forbids any *direct* `wasmtime` dependency
  outside `tpt-pantheon-spine-wasm-sandbox`. Enforced by
  `ci/check-sandbox-boundary.sh`.
- **Drift signal:** if this crate exceeds a few hundred lines of
  configuration-translation logic, that is a red flag it has drifted from its
  job (spec §5.3). Keep it small.

## 4. Telemetry contract (§5.4)

- `tpt-pantheon-spine-telemetry` is the shared OTLP emission wrapper. Every
  crate that emits telemetry does so through this crate, not by constructing
  raw OTLP/exporter configs inline.

---

## 5. Boundary Register

| Component | Storage / runtime | Reason |
|---|---|---|
| `services/identity` | Go, SQLite, own deployment | Domain-mature standards ecosystem (§1) + deliberate trust-boundary isolation |
| `tpt-pantheon-aegis` | Own Keystone instance, own deployment | Deliberate trust-boundary isolation — same engine as everything else, isolated by instance and lifecycle, not by stack |
| `tpt-pantheon-spine-audit-log` | No persistent state | Pure policy/transport wrapper; persistence delegated to its configured sink |
| `tpt-pantheon-spine-telemetry` | No persistent state | Pure emission wrapper; sink is downstream |
| `tpt-pantheon-spine-wasm-sandbox` | No persistent state | Pure policy→config translation (§5.3) |
| `tpt-pantheon-scheduler` | Intentionally NOT forced to depend on Keystone (spec §8) | Per principle 5: no forced dependency; sized to need |

> Add new exceptions here, never by silent omission.
