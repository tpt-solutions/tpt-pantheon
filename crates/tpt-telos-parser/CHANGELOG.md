# Changelog

All notable changes to the `tpt-telos-parser` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-08-12

### Added

- Source spans (`span.rs`): line/col tracking threaded through the lexer and
  parser, powering caret/underline diagnostics in the CLI and LSP.
- Expanded lexer/parser/span integration test suites.

### Fixed

- Synced `grammar.ebnf`'s version comment and its `@state(persistent|ephemeral)`
  note (previously said "parsed only"; storage class is implemented in the
  router/codegen).

## [0.1.1] - 2026-08-01

- Published to crates.io.

## [0.1.0] - 2026-07-16

- Initial release.
