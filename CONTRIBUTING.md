# Contributing

Thanks for looking. Bug reports, missed-secret reports (with made-up examples) and pull requests are
welcome. For a security problem, follow [SECURITY.md](SECURITY.md) instead of opening an issue.

## The one rule: no real secrets, ever

Tests, the corpus, fixtures, docs and screenshots must never contain a real secret, a real token that has
merely been revoked, or real personal data. This repository is public and scanned by secret scanners.

- Realistic vendor tokens are **generated when the test runs**, from templates and a seeded random number
  generator. See `crates/redactor-core/tests/support/gen.rs` and `crates/scrub/tests/support/gen.rs`. A
  token never appears as one contiguous literal in a source file.
- Corpus files hold generator markers instead of tokens. Plain made-up values (a password like
  `Orchid-Lantern-42!` in an obviously fake config) are fine.
- Use reserved example names for hosts and addresses (`example.test`, `192.0.2.0/24`, `203.0.113.0/24`).
- The UI's demo text is assembled at run time for the same reason (`app/ui/src/lib/mock/demo.ts`), and a
  unit test (`no_literal_tokens`) fails if a vendor-shaped token appears in a UI source file.
- Screenshots come from the mock UI and its demo data, never from a real desktop.

Before you commit, run `scrub check` or `scrub scan .` on your changes. If the pre-commit hook is
installed (`scrub install-hook`), it does this for you.

## Build and test

You need Rust 1.99 (`rust-toolchain.toml` selects it) and Node 22 for the UI.

```bash
# Rust: engine, wire, CLI and the platform-neutral part of the app
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked          # includes the corpus accuracy gates
cargo deny check                          # licenses and advisories (cargo install cargo-deny)

# UI
cd app/ui
npm ci
npm run check                             # svelte-check, warnings are errors
npm run lint
npm test                                  # unit tests
npx playwright install chromium           # once
npm run test:e2e                          # UI tests against the mock backend, with the app's CSP
```

The Tauri shell compiles on Windows only (the platform-neutral modules test everywhere). To build and test
it from WSL on a Windows machine, run `tools/win-build.sh`. The clipboard tests in
`app/src-tauri/tests/win_clipboard.rs` need a real desktop session and are ignored by default.

### Regenerating generated files

| File | How |
|---|---|
| `docs/DETECTORS.md` | `tools/gen-docs.sh` |
| `docs/ACCURACY.md` | written by the corpus gates: `cargo test -p redactor-core --test corpus` |
| `app/ui/src/lib/mock/fixtures/*.json` | `tools/gen-ui-fixtures.sh` (real engine output) |
| `assets/*.png`, `assets/demo.gif` | `app/ui/scripts/readme-assets.ts`, see the header of that file |
| classifier weights | `python3 tools/classifier/train.py` (see `tools/classifier/README.md`) |

Do not edit generated files by hand.

## Making a change

- Keep changes small and focused, and add a test with them. A new detector needs a positive test (a
  generated token it must catch) and a negative one (a look-alike it must leave alone).
- `redactor-core`, `redactor-wire` and `scrub` forbid `unsafe` code. In the app, `unsafe` is allowed only
  under `app/src-tauri/src/platform/windows/`, with a `// SAFETY:` comment on each block.
- Code that talks to the operating system goes into `app/src-tauri/src/platform/<os>/`, behind the traits in
  `platform/mod.rs`. The shell (`app.rs`, `commands.rs`, `state.rs`, `tray.rs`) never uses `cfg(windows)` or
  an OS API; `tests/platform_boundary.rs` fails if it does.
- Detection uses the `regex` crate only (linear time). There is no lookaround; check boundaries in code.
- Nothing may log, print or store scanned text, and a finding must never carry a secret value.
- Changes to the IPC contract touch the Rust side (`redactor-wire`, `commands.rs`, `capabilities/main.json`)
  and the UI (`src/lib/types.ts`, `schema.ts`) together.
- Pin dependencies: Tauri and its plugins with `=`, npm packages by exact version, and commit lockfiles.
- Commit messages are short and in the imperative ("Add the Telegram bot token rule").
