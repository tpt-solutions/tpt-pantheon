# Changelog

All notable changes to the `tpt-telos-codegen` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-08-12

### Added

- Promoted the hardcoded `"generated_rust"` literal in `rust_cargo_toml()` to a
  `pub const RUST_CRATE_NAME`, mirroring the existing `pub const GO_PACKAGE`.

### Fixed

- FFI codegen (`ffi.rs`) now rejects non-integer types (floats/strings/bools/
  arrays/nested structs) crossing an FFI-routed boundary with a clear error,
  instead of silently coercing every field to `int64_t`/`i64`.

## [0.1.1] - 2026-08-01

- Published to crates.io.

## [0.1.0] - 2026-07-16

- Initial release.
