# Contributing to tpt-telos

## Issues only

tpt-telos is developed as a maintainer-driven project. **We accept contributions
through GitHub Issues only** — please do not open pull requests with code changes.

- **Bug reports:** open an issue with a minimal `.telos` reproduction, the command
  you ran (e.g. `telos verify examples/foo.telos`), expected vs. actual output, and
  your platform/toolchain versions.
- **Feature requests / proposals:** open an issue describing the use case and the
  behaviour you'd like. Large refactors should be discussed in an issue before any
  work begins.
- **Questions:** open an issue tagged as a question; the maintainers will respond there.

Good issues include enough detail for a maintainer to reproduce and verify a fix
against the existing `examples/*.telos` fixtures and integration tests.

## For maintainers (internal)

The build/test/coverage gates are documented in `AGENTS.md` and `CLAUDE.md`:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo llvm-cov --workspace --fail-under-lines 75
```

Do not lower the 75% coverage floor.
