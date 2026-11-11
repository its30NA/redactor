## What changes and why

## Checks

- [ ] `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings` pass
- [ ] `cargo test --workspace --locked` passes
- [ ] A detection change adds a corpus file (positive and, where it fits, negative) and a test
- [ ] Docs are updated when behavior changes (`docs/`, `CHANGELOG.md`)
- [ ] Every example secret is made up; no real key, password, token or personal data is in the diff
