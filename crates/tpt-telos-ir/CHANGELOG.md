# Changelog

All notable changes to the `tpt-telos-ir` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-08-12

### Fixed

- **Soundness:** `if`/`else` contract lowering (`to_constraints_dnf`) no longer
  accepts the `else` branch unconditionally — it is now guarded by `!cond`, so a
  function that always executes `else` can no longer verify when `cond` was true
  and `then` should have applied.
- **Soundness:** `match` contract lowering now ties each arm's DNF branch to a
  premise that the scrutinee actually matches that arm's pattern, instead of
  treating every arm as independently true.
- **Soundness:** fixed a max-corner bug in `linearize_bounded`'s nonlinear
  bounding, which previously substituted the same corner value regardless of
  whether the surrounding relation needed an upper or lower bound (could prove
  false facts or reject true ones depending on direction).
- `assign_constraint` now accepts a bare local-variable assignment target
  (`Stmt::Assign` to a plain `Expr::Var`), so a scalar `ensures out == a + b`
  bound by `out = a + b;` lowers into a proper equality constraint instead of
  being silently dropped.

## [0.1.1] - 2026-08-01

- Published to crates.io.

## [0.1.0] - 2026-07-16

- Initial release.
