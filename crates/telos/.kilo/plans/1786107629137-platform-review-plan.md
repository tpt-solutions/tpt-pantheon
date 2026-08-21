# tpt-telos Platform Review — Bugs, Gaps, Innovations & Adoption Plan

**Scope:** Review of the tpt-telos v1.2 compiler workspace (10 crates) for bugs, missing
features, documentation integrity, usability, automation, and adoption accelerators.
**Status of review:** Findings gathered by reading code, docs, and a green `cargo build`.
No source changes were made (planning pass only).

> **Note on stray `history/TODO *.md` files:** Per user instruction, do **NOT** delete or
> modify `history/TODO 1260723.md` / `history/TODO 1260713.md`. They are stale duplicates
> of an older TODO (referencing renamed crate paths `crates/telos-parser`), but the user
> wants them left in place. To prevent the same confusion recurring, add a one-line note in
> `AGENTS.md`/`CLAUDE.md` stating these files are intentional historical archives and must
> not be treated as the source-of-truth TODO. Do **not** add any automated cleanup.

---

## 1. Confirmed Bugs & Doc-Integrity Defects (do these first)

Concrete, verifiable, low-risk. They erode trust because docs contradict code.

### 1.1 Crate-count inconsistency `[bug]`
- `README.md` (L109), `ARCHITECTURE.md` (L30 "eight focused crates"), `CONTRIBUTING.md`
  (L48 "eight crates") claim **8** crates.
- `Cargo.toml` lists **10** members: parser, ir, verifier, router, agent, codegen, lsp,
  cli, **out-telos-wasm**, **tpt-telos-sdk**.
- `CLAUDE.md` (L58) says "Nine crates" but omits `out-telos-wasm` too.
- **Fix:** Generate one crate table (or a tiny `cargo` introspection script) referenced
  from all four docs; treat `out-telos-wasm` + `tpt-telos-sdk` as first-class members.

### 1.2 Version drift `[bug]`
- `Cargo.toml` `[workspace.package] version = "0.1.1"` (L17).
- `grammar.ebnf` header says **"v0.2.0"** (L1).
- `CHANGELOG.md` documents only `[0.1.0]`; README/VS Code reference `0.1.0`
  (vscode README: `telos-0.1.0.vsix`).
- **Fix:** Pick the real version; make one source of truth; add a CI check that
  `grammar.ebnf` and `CHANGELOG` versions match the workspace version.

### 1.3 Grammar/feature claim mismatch `[docs]`
- `grammar.ebnf` L20: `@state(...) (storage class, parsed only)` — contradicts Phase 6/9
  claims that `@state(persistent|ephemeral)` is **implemented** in router/codegen.
- **Fix:** Verify against `tpt-telos-router/src/lib.rs`; correct the comment or the docs.

### 1.4 README usage omissions `[docs]`
- README "Usage" omits `telos completions <shell>` (shipped command) and never shows
  `telos transpile` or `init --template`. New users can't discover completions.
- **Fix:** Add `completions`, `transpile`, and `init --template` snippets.

---

## 2. Soundness & Correctness Hardening (highest-value engineering)

### 2.1 Integer-overflow in the FM solver `[soundness]`
- `solver.rs` uses `i128`; `to_inequalities` casts and Fourier-Motzkin elimination can
  overflow on adversarial `requires` bounds, silently producing a wrong UNSAT/SAT.
- The solver is advertised as **sound over integers**; overflow breaks that promise.
- **Fix:** Use checked math; on overflow, take an explicit "bounds too large to decide"
  path instead of a silent verdict. Add `examples/overflow.telos` fixture + test.

### 2.2 Unknown `@boundary(...)` flag silently defaults to Rust `[robustness]`
- Phase 9 warns on unrecognized `@state(...)` but not on typos in `@boundary(...)`
  (e.g. `@boundary(cp_bound)`), which silently fall back to Rust.
- **Fix:** Emit an "unknown boundary flag" warning in `tpt-telos-router`.

### 2.3 `--json` disjunction aggregation `[robustness]`
- `run_verify` human output fixed disjunction groups in Phase 8, but `collect_verify_output`
  (used for `--json`) ORs per-check `passed` without honoring disjunction-group semantics,
  so a branch FAIL inside a passing group can still set `overall=false` in JSON.
- **Fix:** Make `collect_verify_output` group-aware so JSON matches human output.

---

## 3. Missing Features & Innovation Opportunities

### 3.1 LSP gaps — largest adoption lever `[IDE]`
- `textDocument/definition`, `references`, `completion` are explicitly **open** in Phase 9
  (need a workspace-wide symbol index).
- **Innovation:** Add a symbol index (e.g. `telos/index` command or LSP-internal) over all
  `.telos` in a workspace; wire `definition`/`references`/`completion`. Add **inlay hints**
  for resolved `old(...)` values and routing target.

### 3.2 Web playground undocumented & not wired `[adoption]`
- `out-telos-wasm` builds but there is **no hosted playground** and no `index.html`/JS shim.
- **Fix/Innovation:** Add a minimal `playground/` static site (textarea + live `verify`
  output + counterexample display) and deploy to GitHub Pages from CI. Fastest zero-install
  trial for new users.

### 3.3 Templates & examples for faster adoption `[adoption]`
- `telos init --template` has only 3 templates (simple, dual-backend, eject).
- **Add:** `real-time` (`@boundary(real_time, zero_allocation)` + `--strict-rt`),
  `python-ml` (`@boundary(ml_training)` JAX skeleton), `cross-module` (global type
  resolution). Add `examples/START-HERE.telos` annotated walkthrough referenced from README.

### 3.4 `telos new` project scaffold `[adoption]`
- `init` makes one file; no `telos new <name>` that scaffolds a full project (with
  `Cargo.toml`/module layout) so a beginner reaches `telos project --check` in one command.

### 3.5 CI/automation improvements `[automation]`
- Add **release-playground** job (build wasm pkg -> GitHub Pages).
- Add **MSRV** job (Cargo.toml declares `rust-version = 1.74`) to actually enforce it.
- Add **mutation-testing** job (`cargo-mutants`) — Phase 6 claimed it but no CI job exists.
- Add **doc-consistency** CI step (crate count + version match `Cargo.toml`) to prevent §1
  regressions.

### 3.6 SDK ergonomics `[api]`
- `tpt-telos-sdk` is undocumented in the root README. Add a "Using the SDK" section +
  `examples/sdk_usage.rs` snippet.

---

## 4. Recommended Prioritization (ordered)
1. §1 doc-integrity fixes (1.1-1.4) — cheap, restores trust.
2. §2.1 solver overflow — protects the core soundness claim.
3. §2.3 JSON disjunction + §2.2 boundary typos — correctness/robustness.
4. §3.3 templates + START-HERE + §3.4 `telos new` — fastest adoption win.
5. §3.2 playground + CI deploy — zero-install trial.
6. §3.1 LSP definition/references/completion — IDE maturity.
7. §3.5/§3.6 automation + SDK docs — long-term scale.

---

## 5. Validation Plan
- After §1: `cargo build --workspace` green + new `ci-doc-consistency` check passes.
- After §2.1: new `examples/overflow.telos` fixture + verifier test; `cargo test -p
  tpt-telos-verifier` green; clippy `-D warnings` clean.
- After §3.3/3.4: `telos init --template real-time` and `telos new demo` produce files that
  pass `telos verify` / `telos project --check` (where Go/cargo available).
- After §3.2: playground served from GitHub Pages; manual browser smoke test.
- After §3.1: LSP `definition`/`references`/`completion` unit tests in `tpt-telos-lsp`.

## 6. Open Questions
- Is `grammar.ebnf`'s "v0.2.0" label intentional (language ahead of `0.1.1` release) or
  should it sync to `0.1.1`?
- Should the browser playground be a hosted GitHub Pages site (needs deploy secret) or
  remain local-only `wasm-pack` build?
- Priority: LSP IDE completeness (§3.1) vs. adoption templates/playground (§3.2/3.3) — which
  first if only one lands this cycle?
