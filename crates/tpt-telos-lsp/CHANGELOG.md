# Changelog

All notable changes to the `tpt-telos-lsp` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-08-12

### Added

- `textDocument/formatting` (reuses the existing AST pretty-printer).
- `textDocument/definition`, `textDocument/references`, and
  `textDocument/completion`, backed by a new workspace-wide symbol index
  (`build_index`) — built from the session's currently-open documents, not a
  full directory scan.
- `textDocument/inlayHint`, showing each module's routing target and marking
  `old(...)` pre-state references.

## [0.1.1] - 2026-08-01

- Published to crates.io.

## [0.1.0] - 2026-07-16

- Initial release.
