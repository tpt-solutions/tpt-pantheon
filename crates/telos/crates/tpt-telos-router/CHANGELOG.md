# Changelog

All notable changes to the `tpt-telos-router` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-08-12

### Added

- `RoutingDiagnostic` now also flags `real_time`/`zero_allocation` modules
  routed to Python (interpreter + GC), previously only checked for Go-target
  conflicts.
- `route_checked` warns on unrecognized `@state(...)` values (e.g. a typo like
  `@state(persistant)`) instead of silently falling back to `Ephemeral`.
- `route_checked` emits `UnrecognizedBoundaryFlag` for unknown `@boundary(...)`
  flags (e.g. `cp_bound`) instead of silently ignoring them.

## [0.1.1] - 2026-08-01

- Published to crates.io.

## [0.1.0] - 2026-07-16

- Initial release.
