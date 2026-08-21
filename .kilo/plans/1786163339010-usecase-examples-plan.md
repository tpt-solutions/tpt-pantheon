# Plan: Highest-use-case showcase examples for tpt-telos

## Goal
Add a curated "highest-use-case" showcase set to `examples/` so new users/integrators
immediately see what Telos is *for*: verification-first, contract-based, multi-backend
(Rust/Go/Python) systems code. Per user decision this is **showcase / adoption**
material — every file must **verify green** end-to-end (no deliberate-fail demos;
`broken.telos` / `unsatisfiable.telos` remain the fail-path fixtures).

## The set (6 files)
Two already exist and need only an adoption banner; four are new.

| # | File | Boundary → backend | What it showcases |
|---|------|-------------------|-------------------|
| 1 | `examples/wallet.telos` (exists) | `cpu_bound` → Rust | Verified financial **state machine** (invariant `balance >= 0` + `transfer` with `old(...)`). |
| 2 | `examples/real_time.telos` (NEW) | `real_time, zero_allocation` → Rust | **Hard-real-time / embedded control**: integer-bounded actuator, `zero_allocation` → Rust (NOT Go, so no `real_time_go_conflict` warning). |
| 3 | `examples/microservice.telos` (exists) | `cpu_bound`+`zero_allocation` → Rust **and** `network_io`+`high_concurrency` → Go | **Dual-backend + automatic FFI bridge** (the Rust/Go split with zero glue). |
| 4 | `examples/crypto.telos` (NEW) | `crypto` → Rust | **Cryptographic / sensitive-op safety**: a secret value with a `value >= 0` invariant that must hold across consume/refresh. |
| 5 | `examples/cryptocurrency.telos` (NEW) | `cpu_bound` → Rust | **Coin conservation**: a `balance_a + balance_b == RESERVE` invariant preserved by a transfer (distinct from `wallet.telos` which only asserts `balance >= 0`). |
| 6 | `examples/distributed.telos` (NEW) | `distributed` → Go | **Distributed / network service**: a replicated-log coordinator (`commit_index >= 0`, monotonic append) routed to Go. |

## Content sketches (all kept to proven, already-verified constructs:
`module` / `invariant` / `requires` / `ensures` / `old(...)` / `mutate state` /
`PositiveInt`. **Do NOT use `forall` / aggregate (`sum`/`min`/`max`)** — implemented but
untested in fixtures; risk of unsound/unsupported lowerings. Keep bodies branch-free
(plain `+=`/`-=` in `mutate state`) so the verifier proves `ensures` trivially, exactly
like `wallet.telos`.)

- **`real_time.telos`**
  ```telos
  // Showcase #2: hard-real-time embedded controller.
  // @boundary(real_time, zero_allocation) routes to Rust (deterministic, no GC).
  @boundary(real_time, zero_allocation)
  module Controller {
      invariant Actuator { position >= 0 && position <= 1000 }
      func step(s: Actuator, delta: Int)
          requires s.position + delta >= 0 && s.position + delta <= 1000
          ensures s.position == old(s.position) + delta
      {
          mutate state { s.position += delta }
      }
  }
  ```
- **`crypto.telos`**
  ```telos
  // Showcase #4: sensitive/secret state on the crypto backend (Rust).
  @boundary(crypto)
  module SecretStore {
      invariant Secret { value >= 0 }
      func consume(s: Secret, amount: PositiveInt)
          requires s.value >= amount
          ensures s.value == old(s.value) - amount
      { mutate state { s.value -= amount } }
      func refresh(s: Secret, add: PositiveInt)
          requires s.value >= 0
          ensures s.value == old(s.value) + add
      { mutate state { s.value += add } }
  }
  ```
- **`cryptocurrency.telos`**
  ```telos
  // Showcase #5: cryptocurrency conservation (total supply is invariant).
  @boundary(cpu_bound)
  module Token {
      invariant Vault { balance_a + balance_b == 1_000_000 }
      func move(c: Vault, amt: PositiveInt)
          requires c.balance_a >= amt && amt <= c.balance_b
          ensures c.balance_a == old(c.balance_a) - amt
          ensures c.balance_b == old(c.balance_b) + amt
      { mutate state { c.balance_a -= amt; c.balance_b += amt } }
  }
  ```
- **`distributed.telos`**
  ```telos
  // Showcase #6: distributed coordinator on the Go backend.
  @boundary(distributed)
  module Coordinator {
      invariant Log { commit_index >= 0 }
      func append(l: Log)
          requires l.commit_index >= 0
          ensures l.commit_index == old(l.commit_index) + 1
      { mutate state { l.commit_index += 1 } }
  }
  ```

## Polish the two existing files (no logic change — keep them regression-PASS)
- `examples/wallet.telos`: prepend a banner comment marking it **Showcase #1** with a
  one-line "what it demonstrates" and the run commands
  (`telos verify examples/wallet.telos`, `telos build examples/wallet.telos --out-dir ./gen-wallet`).
- `examples/microservice.telos`: prepend a banner marking it **Showcase #3** (dual-backend
  FFI) and add `telos project examples/microservice.telos --check` to the run commands.

## Wiring & docs
1. **Integration tests** — add one `verify`-passes test per NEW example to
   `crates/tpt-telos-verifier/tests/nested.rs` (alongside `nested_example_passes` /
   `cross_module_example_passes`), following the existing `read_to_string -> parse ->
   extract -> verify -> assert!(r.all_passed)` pattern. This is backend-independent
   (Rust-only verifier; no `go`/`gofmt` needed in CI).
2. **`examples/README.md`** — update the table: tag the 6 files as the "Showcase:
   highest-use-case set" (e.g. a leading note + the existing one-line "Demonstrates"
   column), and reference them from the root `README.md` showcase section.
3. (Optional, toolchain-dependent) Confirm `telos build` on the Rust-routed files
   (`wallet`, `real_time`, `crypto`, `cryptocurrency`) and `telos project --check` on
   `microservice` + `distributed` — needs `cargo` and `go`/`gofmt` on PATH; do not block
   the PR on Go if the CI matrix lacks it, but record results.

## Validation (CI gates)
- `cargo test -p tpt-telos-verifier` — new example tests pass.
- `telos verify examples/<each>.telos` — all 6 PASS.
- `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo llvm-cov --workspace --fail-under-lines 75` — not lowered.

## Risks / open notes
- `real_time` must route to **Rust**, never Go, to avoid the `WARNING [real_time_go_conflict]`.
- Avoid `forall`/aggregates in v1 (untested lowerings). If a future example wants a
  sensor-array loop, validate `telos verify` first and add a regression test.
- Keep the showcase set entirely green; the fail-path story stays in `broken.telos` /
  `unsatisfiable.telos` / `interval.telos` (`[interval-bounded]`).
