# Changelog

All notable changes to the `tpt-telos` crate (the `telos` binary) will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-08-12

### Added

- `telos fmt [--check|--stdout]` — reformat a `.telos` file canonically,
  reusing the LSP's formatter.
- `telos doctor [--json]` — checks for optional external tools (`go`,
  `gofmt`, Z3) that some commands shell out to.
- `telos completions <shell>` — shell-completion generation via
  `clap_complete` (bash/zsh/fish/powershell/elvish).
- `telos new <name>` — scaffolds a `<name>.telos` file plus README so a new
  user reaches `telos project --check` in one command.
- `--json` output for `telos eject`/`telos parse` (previously only on
  `verify`/`build`/`project`).
- Colorized, rustc-style diagnostics (caret/underline source spans) for CLI
  errors.
- `--watch` extended from `verify`-only to `build`/`project` too, with
  debouncing and directory-wide watching (previously a naive single-file
  mtime poll).
- `telos init --template <name>` with five starter templates (`simple`,
  `dual-backend`, `eject`, `real-time`, `python-ml`, `cross-module`) instead
  of one hardcoded Counter module.
- `verify` failure output now surfaces the FM solver's documented integer-
  incompleteness, hinting that some failures may be solver limitations
  rather than genuine spec violations.

### Fixed

- `eject --func <name>` now errors when the requested function doesn't exist,
  instead of silently ejecting a different `@eject` function.
- Pre-existing `: Int` return-type and `return`-in-`mutate state` bugs in the
  `simple`/`dual-backend`/`eject` `init` templates, so every template
  verifies cleanly.

## [0.1.1] - 2026-08-01

- Published to crates.io.

## [0.1.0] - 2026-07-16

- Initial release.
