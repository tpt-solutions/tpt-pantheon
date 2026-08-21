# tpt-pantheon — Project Todo (Rev. 2)

Phased checklist derived from `spec.txt` (Rev. 3). Layers are gated: nothing
in a layer starts until the layer below has a real, working, tested proof
in the monorepo (§4) — and that proof is now an explicit task in this list,
not just a stated precondition.

Each phase ends with its own **Definition of Done** and a version tag. A
phase isn't finished until both are checked, the same way the original
wedge and Iteration 2 were closed out.

## Phase 0 — Repo & Tooling Setup

*Prerequisite scaffolding; not a spec layer.*

- [ ] Initialize git repo, `.gitignore`
- [ ] Dual-license files: `LICENSE-MIT`, `LICENSE-APACHE` (copyright TPT Solutions)
- [ ] Workspace root `Cargo.toml` (members under `crates/`; `license = "MIT OR Apache-2.0"`; excludes `services/identity`)
- [ ] `justfile` wiring both `cargo` (Rust crates) and `go build`/`go test` (Identity)
- [ ] CI pipeline: build + test the Rust workspace and the Go service
- [ ] `cargo-deny` config, incl. ban on direct `wasmtime` deps outside `tpt-pantheon-spine-wasm-sandbox`
- [ ] `SPINE.md` skeleton: storage/audit/sandbox/telemetry contracts + boundary register table
- [ ] `NEW_CRATE_TEMPLATE.md` — the one-paragraph gate every future crate must fill in before it exists: (1) what does this solve that nothing else does, (2) which spine contracts does it satisfy or explicitly except itself from and why, (3) is this Pantheon or a different program
- [ ] Root `README.md`

**Definition of Done:** repo builds and CI runs green with zero crates yet
added; `just build` and `just test` both succeed as no-ops.
**Tag:** `v0.0.1` — tooling only, no components.

## Phase 1 — Layer 0: Spine crates

*Starts once Phase 0 tooling is in place.*

- [ ] `tpt-pantheon-spine-audit-log` — shared `AuditSink::append`; CI check tying any Keystone-write crate to this dependency or a `SPINE.md` exemption
- [ ] `tpt-pantheon-spine-wasm-sandbox` — `SandboxPermissions` → `wasmtime::Store` config + `cap-std` `WasiCtx` + conditional `Linker` host-fn registration; watch the size-drift signal (§5.3) — flag in review if this crate exceeds a few hundred lines
- [ ] `tpt-pantheon-spine-telemetry` — shared OTLP emission wrapper

**Definition of Done:**
- [ ] All three crates have unit tests, including a `verify_chain` test in
      the audit crate that deliberately corrupts one record and confirms
      detection
- [ ] `cargo deny check` passes with the `wasmtime` ban active
- [ ] Each crate's own one-paragraph gate (per `NEW_CRATE_TEMPLATE.md`) is
      filled in at the top of its README

**Tag:** `v0.1.0` — spine complete, no consumers yet.

## Phase 2 — Layer 1: Core Primitives (vendored) + Milestone 1 wedge

*Starts once Layer 0 spine crates have a working, tested proof.*

- [ ] `git subtree add` Keystone → `crates/keystone` (name/history preserved)
- [ ] `git subtree add` Telos → `crates/telos`
- [ ] `git subtree add` AppFront → `crates/appfront`
- [ ] Wire all three to `tpt-keystone-sdk` per the storage contract (§5.1)

**Milestone 1 wedge — the proof, not just the vendored crates:**
- [ ] `accounts(id, name, balance)` table + migration
- [ ] `wallet.telos` transfer contract, verified (`telos verify`), compiled
      to the Rust function the app actually executes
- [ ] Glue service (Axum): `GET /accounts`, `POST /transfer`, calling the
      telos-generated function, one Keystone transaction per transfer
- [ ] AppFront (DOM) account list + transfer form, wired end-to-end
- [ ] Every successful/rejected transfer logged via
      `tpt-pantheon-spine-audit-log`; `GET /audit` + UI panel; a test that
      runs `verify_chain` against the demo's real accumulated log
- [ ] One command starts the whole wedge (`docker compose up` or
      documented equivalent)
- [ ] Reference conventional-stack implementation (`reference-stack/` per
      the original demo design) for benchmark comparison
- [ ] Benchmark suite run and `BENCHMARKS.md` written: integration overhead
      metrics, mutation testing comparison, property-fuzz comparison,
      counterexample demonstration, runtime performance — methodology and
      raw numbers, honest about anything that doesn't favor the thesis

**Definition of Done:** all Milestone 1 wedge boxes above checked; recorded
60–120s demo (cold start → transfer → rejected transfer → both visible
directly in Keystone).
**Tag:** `v1.0.0` — this is the platform's first real, demonstrated,
benchmarked claim.

## Phase 3 — Layer 2: Identity & Messaging + Milestone 2 proof

*Starts once Layer 1 has its working, tested proof (Milestone 1, tagged
`v1.0.0`).*

- [ ] `git subtree add` Identity → `services/identity` (Go, SQLite, own
      `go.mod`, wired into root justfile/CI, not a Cargo workspace member)
- [ ] `tpt-pantheon-aegis` — **v1 scope, fixed:** authorization code + PKCE,
      session management, RBAC only. Full original feature set (dynamic
      client registration, MFA, duress codes, etc.) explicitly deferred —
      not built until a real need appears. Own Keystone instance, own
      deployed process, isolated DB role.
- [ ] `tpt-pantheon-synapse` — **v1 scope, fixed:** one wire protocol
      adapter to start (pick the one an actual near-term consumer needs),
      not all four at once. All-Rust, reusing Scheduler's Raft
      implementation — no separate control-plane stack.
- [ ] `SPINE.md` boundary register entries for Identity and Aegis, with
      their stated reasons (§7 of spec)

**Milestone 2 proof:**
- [ ] Wedge's transfer form gated behind real Aegis-issued auth
- [ ] Test confirming the wedge app's Keystone role has no grant on Aegis's
      Keystone instance (the actual isolation property, demonstrated, not
      just asserted)
- [ ] Test confirming Identity is only ever reached through its own API,
      never queried directly by anything outside its service boundary

**Definition of Done:** both Milestone 2 tests passing in CI, wedge demo
updated and re-recorded with the auth flow visible.
**Tag:** `v1.1.0`.

## Phase 4 — Layer 3: Orchestration + Milestone 3 proof

*Starts once Layer 2 has its working, tested proof (`v1.1.0`).*

- [ ] `tpt-pantheon-cli` (binary `pantheon`) — **v1 scope, fixed:** `init`,
      `up`, `down`, `doctor` only, wrapping exactly the commands the wedge
      already runs manually. No plugin system, no daemon mode yet.
- [ ] `tpt-pantheon-workflow` — durable execution, Keystone-native from
      commit one. **v1 scope:** the one workflow the CLI's starter template
      needs (e.g. an onboarding step), not a general BPM feature set.
- [ ] `tpt-pantheon-scheduler` — distributed jobs, Raft-based, intentionally
      not forced to depend on Keystone. **v1 scope:** single-node cron
      replacement only; cluster mode deferred until something needs it.

**Milestone 3 proof:**
- [ ] `pantheon create --template minimal` produces a runnable scaffold:
      Keystone + Telos slot + AppFront + Aegis auth + one Workflow-triggered
      step, started with `pantheon up`
- [ ] This scaffold is run by someone other than the project's own author,
      and their friction points are recorded (the first artifact meant for
      an outside user, per the original spec)

**Definition of Done:** scaffold runs clean on a fresh machine via the
recorded steps; outside-user friction notes captured in `IDEAS.md`.
**Tag:** `v1.2.0`.

## Phase 5 — Layer 4: Production & Delivery + Milestone 4 proof

*Starts once Layer 3 has its working, tested proof (`v1.2.0`).*

- [ ] `tpt-pantheon-zephyr` — edge gateway. **v1 scope:** TLS termination +
      routing for the Milestone 3 scaffold only; WASM-aware caching and
      gossip-based multi-node features deferred.
- [ ] `tpt-pantheon-tectonic` — IaC. **v1 scope:** local + one real provider
      (whichever the actual deployment target is), Keystone-backed state
      from commit one per the original phase plan (file-state bootstrap →
      Keystone-backed once trusted).
- [ ] Starters — **v1 scope:** `minimal` template only remains supported;
      additional templates (saas-kit, etc.) explicitly deferred until a
      real requested use case exists.

**Milestone 4 proof:**
- [ ] Milestone 3 scaffold deployed via Tectonic to a real (even if small/
      single-node) environment, reachable through Zephyr

**Definition of Done:** deployment reproducible from a clean environment via
documented Tectonic + Zephyr steps; smoke test confirms the deployed
scaffold's transfer flow works end-to-end over the network.
**Tag:** `v1.3.0`.

## Phase 6 — Layer 5: Ecosystem & Control Plane

*Starts once Layer 4 has its working, tested proof (`v1.3.0`).*

- [ ] `tpt-pantheon-faros` — **v1 scope:** query + dashboard over the
      Milestone 4 deployment's existing Keystone data and audit log only —
      not built speculatively ahead of real data to show.
- [ ] `tpt-pantheon-studio` — **v1 scope:** Telos live-verification +
      Keystone SQL autocomplete only; other integrations (Tectonic plan/
      apply, AppFront preview) deferred until Studio's core loop is proven
      useful.
- [ ] `tpt-pantheon-marketplace` — **first real consumer of
      `tpt-pantheon-spine-wasm-sandbox`.** Publish/install pipeline + one
      real extension type, exercised through the sandbox contract with a
      test that a module without a granted capability genuinely cannot
      call it (not just returns an error).
- [ ] `tpt-pantheon-chaos` — **v1 scope:** one fault type (process kill)
      against the Milestone 4 deployment, since this needs a real deployed
      system to be meaningful at all.

**Definition of Done:** each component's stated v1 scope demonstrated
against the real Milestone 4 deployment, not a synthetic stand-in.
**Tag:** `v1.4.0`.

## Ongoing / cross-cutting

*No fixed phase — revisit continuously per guiding principles (§3).*

- [ ] Every new crate's README opens with its filled-in
      `NEW_CRATE_TEMPLATE.md` paragraph before any implementation work
      starts — no exceptions, including crates that feel obviously in-scope
- [ ] Keep `SPINE.md` boundary register current as new exceptions arise
- [ ] Keep `cargo-deny` / CI checks green as crates are added
- [ ] Re-check "friction picks the next component" (§3.5) before starting
      each new phase — the phase order above is a default, not a mandate;
      a phase can be skipped or reordered if the friction that justifies it
      hasn't actually appeared yet
- [ ] No phase is re-opened after its tag without a new, explicit decision
      to do so — "done" means done until a real reason says otherwise