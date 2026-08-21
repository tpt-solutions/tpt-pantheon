# Changelog

All notable changes to the `tpt-telos-verifier` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-08-12

### Changed

- Improved disjunction handling and Z3 backend dispatch.

### Fixed

- **Soundness:** the Fourier-Motzkin solver now uses checked `i128` arithmetic
  throughout. On overflow, `unsat_checked` returns `None` ("bounds too large to
  decide") instead of a spurious answer, and `unsat` conservatively returns
  `false` rather than ever claiming a false contradiction.

### Added

- `examples/overflow.telos` fixture plus `unsat_checked_overflow_is_conservative`
  and `overflow_example_does_not_panic` tests locking in the overflow behavior.

## [0.1.1] - 2026-08-01

- Published to crates.io.

## [0.1.0] - 2026-07-16

- Initial release.
