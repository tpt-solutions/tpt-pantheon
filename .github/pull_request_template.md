## Summary

<!-- What does this PR do? One or two sentences. -->

## Type of change

- [ ] Bug fix
- [ ] New feature
- [ ] Documentation / tooling
- [ ] Refactor (no behaviour change)

## Testing done

<!-- Describe how you tested this. New tests added? Existing tests updated? -->

## Checklist

- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` is clean
- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo llvm-cov --workspace --fail-under-lines 75` passes (coverage not lowered)
- [ ] New or updated fixtures added to `examples/` and wired into integration tests (if applicable)
