# Changelog

All notable changes to the `tpt-telos-agent` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-08-12

### Added

- `StaticAgent` now synthesizes a first-attempt body for pure inequality
  `ensures` clauses (e.g. `ensures balance >= 0`), instead of producing an
  empty body.

### Fixed

- `problem_for` returns `Result` instead of panicking on an internal
  re-extraction failure during the verify/rewrite loop.

## [0.1.1] - 2026-08-01

- Published to crates.io.

## [0.1.0] - 2026-07-16

- Initial release.
