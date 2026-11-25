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

`npm run dev` serves the UI against the mock backend. Query switches pick what the mock pretends, such as
`?os=linux&hotkey=command&focus=1`, `?clipboard=hidden` or `?nolock`; the full list is at the top of
`app/ui/src/main.ts`.

The Tauri shell builds on Windows, macOS and Linux (Linux needs the WebKitGTK development packages). To build
and test the Windows side from WSL on a Windows machine, run `tools/win-build.sh`: `check` runs clippy and the
tests that leave the desktop alone, `test` also runs the clipboard tests in
`app/src-tauri/tests/win_clipboard.rs`, which need a real desktop session and are ignored by default (they
save and restore your clipboard text).

### Build the app

The app crate embeds the built UI, so build the UI first, then the app with the Tauri CLI:

```bash
cd app/ui && npm ci && npm run build
cd ../src-tauri && ../ui/node_modules/.bin/tauri build --no-bundle -- --locked   # target/release/Redactor
```

A plain `cargo build --release` gives a development binary (`redactor-app`) that loads the dev server at
localhost:5173 and shows a white window without it. On Linux, install the packages CI uses first:

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
```

### Linux desktop tests

The tests that need an X server or a D-Bus bus (`app/src-tauri/tests/x11_clipboard.rs` and the other
files listed in the script) are ignored by default and refuse to run unless `REDACTOR_TEST_DISPLAY` is set.
Do not set it by hand: on a real desktop they would take over your clipboard. Run

```bash
sudo apt-get install -y xvfb xclip xdotool xbindkeys x11-utils x11-xserver-utils openbox wmctrl python3-dbusmock dbus
tools/ci/x11-tests.sh
```

The script starts its own Xvfb (a display number from 99 up), a private session bus and a private bus with
a logind mock in place of the system bus, runs the tests one at a time and fails unless every test binary
is green. It works under WSL too, where it never touches the Windows clipboard. Tests wait with a deadline
and are never retried: a test that fails once in ten runs is a bug to find.

`tools/ci/x11-tests.sh --run <command...>` runs any command in that same environment, for example the app
itself. `tools/ci/linux-smoke.sh <app binary>` starts the built app there and checks it end to end (S1-S9:
the hotkey scrubs the clipboard, clean text is left alone, lock and sleep wipe, the window closes or hides
to the tray as the desktop allows, and at the end the XDG folders hold only the files the app may leave and
none of the passwords it was given); `--self-test` checks the script's own set-up without an app. With
`SMOKE_WEBDRIVER=0` (as CI runs it) S2, S5 and S6 are SKIP, so 6 pass and 3 skip.

The Linux test and smoke scripts unmount anything still mounted in their temporary folder when they exit
(the document portal's `$XDG_RUNTIME_DIR/doc`, for example) before they delete it. A clean-up problem only
prints a warning; it never changes the script's exit status.

### Wayland desktop tests

The Wayland tests (`app/src-tauri/tests/wayland_*.rs`) need a compositor and a D-Bus bus. They are ignored
by default and refuse to run unless `REDACTOR_TEST_WAYLAND` names the socket of the compositor the script
started. **Do not set it by hand and never run the tests on your real desktop:** they take over the
clipboard. Under WSL the inherited `WAYLAND_DISPLAY` is WSLg, which is your Windows clipboard.

```bash
sudo apt-get install -y sway grim wl-clipboard python3-gi gir1.2-glib-2.0 dbus jq webkit2gtk-driver
tools/ci/wayland-tests.sh
```

- The script starts headless sway (`tools/ci/sway.conf`: no Xwayland, no bar), a private session bus with
  no services on it, and its own `XDG_RUNTIME_DIR`, so nothing reaches the desktop you started it from. It
  runs the test binaries one at a time and fails unless every one reports `0 failed` and at least one
  passed; with `WAYLAND_TESTS_EXPECT=<n>` (as CI sets it) it also fails unless exactly n tests passed, so a
  test that went missing is noticed. Tests wait with a deadline and are never retried.
- `python3-gi` is needed for `/usr/bin/python3` (the system one): the portal mock
  (`tools/ci/mock_portal.py`) uses it. A pyenv or conda `python3` has no `gi`; the scripts call
  `/usr/bin/python3` on purpose.
- The portal tests run in two passes: `no_portal_means_command_mode` alone first (no portal on the bus, must
  report exactly 1 passed), then all tests with the mock running.
- Lines the tests print as `OBSERVED ...` (what a compositor does where there is no pass or fail, such as
  the measured write race) are collected at the end of the run and in the GitHub job summary. Read them
  when a number looks different from last time.
- `tools/ci/wayland-tests.sh --run <command...>` runs a command in that same environment, from the
  repository root (with the mock portal, unless `MOCK_PORTAL=none`): to start the built app on headless sway
  by hand. `grim` takes screenshots of that compositor.

`tools/ci/wayland-smoke.sh <app binary>` checks the built app end to end on headless sway (W1-W8: a native
Wayland window, `--scrub-clipboard` scrubs a copy and leaves a password manager's copy alone, the clipboard
that works only while the window is in front, Settings without a portal, quit, and what the runs left in
the XDG folders). It runs itself inside
`wayland-tests.sh --run` with no portal, and first builds two development helpers in
`app/src-tauri/examples` (they are not part of the app): `wayland_offer`, a copy marked as a password
manager's, and `wayland_keyboard`, a keyboard for the seat, because headless sway has none and without one
no window gets focus. `REDACTOR_WAYLAND_FOCUSED=1` makes the app use the window clipboard even where the
compositor offers data-control; it is a test switch and can only take a capability away. The Settings check
(W6) goes through WebDriver, which cannot open a session in the app's incognito web context, so W6 fails
with WebDriver on. CI runs the smoke with `SMOKE_WEBDRIVER=0`, where W6 is reported as SKIP (not tested at
L3) and the rest must pass. With WebDriver on, the smoke also needs `webkit2gtk-driver` and `tauri-driver`
(`cargo install tauri-driver --locked --version 2.1.0`). Screenshots and logs go to `smoke-out/`.

KDE Plasma (KWin, with the data-control protocol `ext_data_control_v1` that sway does not offer) is tested
in a Fedora 42 container (needs Docker): `REDACTOR_WAYLAND_COMPOSITOR=kwin` makes `wayland-tests.sh` start
`kwin_wayland --virtual` instead of sway and run `wayland_clipboard` only (no mock portal, no focused-window
tests). The container has no cargo, so the test binaries are built on the host first:

```bash
REDACTOR_WAYLAND_COMPOSITOR=kwin tools/ci/wayland-tests.sh --build target/wayland-tests
tools/ci/desktop-container.sh env REDACTOR_WAYLAND_COMPOSITOR=kwin WAYLAND_TEST_BINARIES=target/wayland-tests \
  tools/ci/wayland-tests.sh
```

CI sets `WAYLAND_TESTS_EXPECT` to 42 on sway (19 in `wayland_clipboard`, 12 in `wayland_focused`, 10 in
`wayland_portal` and 1 in `wayland_webview`; `no_portal_means_command_mode` runs in its own pass) and to 19
on KWin, the number of tests that must pass. Change a count there when you add or remove a test;
`cargo test -p redactor-app --test <binary> -- --list --ignored` lists them.

Two things the machines cannot test: the real GNOME and KDE shortcut dialogs, and the window clipboard with
a real desktop's focus rules. Those are tested by hand, on a real desktop of that kind. A WSLg round (the
window-clipboard path and the command, on Weston) is run by hand before a release.

### macOS desktop tests

The macOS tests run on GitHub's macOS runners (macOS 26 on Apple silicon and macOS 15 on Intel, job
`app-macos-desktop`). `app/src-tauri/tests/macos_clipboard.rs` runs the clipboard contract and the macOS
race tests on the real pasteboard; `tests/macos_desktop.rs` checks the hardened web view, the lock and sleep
signals and the global hotkey. They are ignored by default and refuse to run unless
`REDACTOR_TEST_PASTEBOARD` (and `REDACTOR_TEST_DESKTOP`) is set. On your own Mac they take over the clipboard,
press hotkeys and post "screen locked" to the apps of the session, so the script runs only on a CI runner or
when told so:

```bash
tools/ci/macos-tests.sh --i-know-this-takes-my-clipboard
tools/ci/macos-smoke.sh <Redactor.app> --i-know-this-takes-my-desktop
```

`macos-smoke.sh` starts the built app with a throwaway home and drives it with posted keys and clicks
(small Swift helpers in `tools/ci/macos/`, compiled on the run): the hotkey scrubs the clipboard, Cmd+V in
the window, the lock wipes, the window comes back from the menu bar, nothing is left on disk. Lines starting
with `OBSERVED` say what this macOS did where that is not a pass or a fail; read them in the job summary.
The UI's Playwright suite also runs in WebKit on the macOS runners (`PW_WEBKIT=1 npx playwright test
--project=webkit`; it runs on Linux too once Playwright's WebKit is installed).

### Package checks (L4)

Every download is built on every pull request and checked the way a user gets it: CI's `bundle-linux`,
`bundle-macos` and `bundle-flatpak` jobs run the same scripts as the release, and the `l4-*` jobs install
each file on a clean system and start it. To build the Linux bundles yourself (Ubuntu 22.04 or newer, with
the packages from [Build the app](#build-the-app) plus `file`; the `.rpm` check also needs `rpm2cpio` and
`cpio`):

```bash
cd app/ui && npm ci && npm run build && cd ../..
RUSTFLAGS="$(tools/release/remap-path-flags.sh "$PWD")" tools/release/bundle-linux.sh dist
```

`dist/` then holds the AppImage, the `.deb` and the `.rpm`, and the script has already run the contents
check on them. The checks can also be run one by one:

```bash
tools/ci/l4/contents.sh dist/Redactor_<version>_amd64.deb --expect-version <version>
tools/ci/l4/selftest.sh       # proves start.sh cannot reach your desktop, clipboard or home
tools/ci/l4/start.sh <version> dist/Redactor_<version>_amd64.AppImage --appimage-extract-and-run
```

`contents.sh` looks inside a package: LICENSE and the notices, the binary, the desktop file, no path of the
build machine in the binary, the version, and a license for every library the AppImage or Flatpak bundles
(for the AppImage also its runtime, AppRun and GTK hook, `packaging/appimage/APPIMAGE-PARTS.txt`).
`start.sh` runs `--version` with no display, then opens the window, checks that a second launch with
`--scrub-clipboard` hands over to the first, and that quitting leaves nothing behind. It always starts its
own Xvfb from `:90`, its own D-Bus session and a temporary home and XDG folders, and unsets
`WAYLAND_DISPLAY`, so it is safe to run on a desktop or in WSLg.

CI-only: `tools/ci/l4/install.sh` (it installs into a clean container as root: `.deb` on Ubuntu 22.04,
24.04 and Debian 12, `.rpm` on Fedora 45, the AppImage on Ubuntu 22.04 and Fedora 45, the Flatpak on
Fedora 45), the Flatpak build (`tools/release/bundle-flatpak.sh`, in Flathub's GNOME 51 build image) and
the Mac disk image (`tools/release/bundle-macos.sh` and `tools/ci/l4/dmg.sh`, on the macOS runners). On a
Mac, `dmg.sh` runs the app's smoke test only with `--i-know-this-takes-my-desktop`.

### Run before each release

These tests run on no CI machine: hosted runners have no reliable desktop clipboard, and timing gates on
shared runners flake. Run them by hand before each release tag.

| What | How |
|---|---|
| The Windows clipboard tests (`app/src-tauri/tests/win_clipboard.rs`, with the clipboard history check) | `tools/win-build.sh test` from WSL on a Windows machine. It saves and restores your clipboard text. |
| The engine's performance tests (`crates/redactor-core/tests/perf.rs`, including `adversarial_shapes_scale_linearly`) | `cargo test -p redactor-core --release --test perf -- --ignored --nocapture --test-threads=1` |
| Wayland smoke W6 (Settings shows the command to bind where there is no shortcut portal) | WebDriver cannot open a session in the app's private web context, so CI skips it. Check it by hand on a desktop without a portal: open Settings and check that it shows the command to bind. |

### Regenerating generated files

| File | How |
|---|---|
| `docs/DETECTORS.md` | `tools/gen-docs.sh` |
| `docs/ACCURACY.md` | written by the corpus gates: `cargo test -p redactor-core --test corpus` |
| `app/ui/src/lib/mock/fixtures/*.json` | `tools/gen-ui-fixtures.sh` (real engine output) |
| `assets/*.png`, `assets/demo.gif` | `app/ui/scripts/readme-assets.ts`, see the header of that file |
| classifier weights | `python3 tools/classifier/train.py` (see `tools/classifier/README.md`) |
| `packaging/flatpak/cargo-sources.json`, `packaging/flatpak/node-sources.json` | `tools/gen-flatpak-sources.sh`, after any change to `Cargo.lock` or `app/ui/package-lock.json` (CI fails when they are stale) |

Do not edit generated files by hand.

## How to release

A release is a tag on `main`. GitHub Actions builds every file; nothing is uploaded from a developer's
machine.

1. **Versions.** Set the new version in `Cargo.toml` (`[workspace.package]`), `app/src-tauri/tauri.conf.json`
   and `app/ui/package.json`, run `cargo build` and `npm install --package-lock-only` in `app/ui` so both
   lockfiles carry it, and add a `<release version="…" date="…"/>` line at the top of `<releases>` in
   `packaging/flatpak/io.github.its30na.redactor.metainfo.xml`. A pre-release tag such as `v2.2.0-rc.1`
   needs that exact string in all of them. The Flatpak builds with its own pinned Rust and Node
   archives (the `rust` and `node` modules of `packaging/flatpak/io.github.its30na.redactor.json`),
   while the other builds take the newest release of the Rust channel in `rust-toolchain.toml` and of
   Node 22: when a newer one is out, put its URL and sha256 into the manifest.
   `tools/ci/l4/flatpak-manifest-check.py` fails when the Rust release differs from the local `rustc`
   or the Node major version from the workflows'; a newer Node 22 release it does not notice.
2. **Changelog.** Rename `## [Unreleased]` in `CHANGELOG.md` to `## [<version>] - <date>`, with its
   **Known limits** and **Not tested by hand** lists brought up to date. The section becomes the release
   notes.
3. **Check.** `tools/release/check-version.sh v<version>` and `tools/release/changelog-section.sh
   v<version>` pass, the [hand tests](#run-before-each-release) are done, and CI is green on the commit.
4. **Tag** the merged commit on `main` as `v<version>` and push the tag to GitHub.
5. **The release workflow** (`.github/workflows/release.yml`) checks the tag again (on `main`, versions,
   changelog, CI green, the dependency policy for every platform), then builds the Windows installer and
   portable zip, the universal Mac disk image, the Linux AppImage, `.deb`, `.rpm` and Flatpak, and `scrub`
   for four targets. The AppImage and the disk image are started once on the runner. It writes
   `SHA256SUMS`, attests every file and opens a **draft** release.
6. **Verify every file of the draft** before publishing. Download them all, then:

   ```bash
   sha256sum --check SHA256SUMS                       # every line OK
   for f in *; do gh attestation verify "$f" --repo its30NA/redactor; done
   ```

   and look for build paths in each binary (unpack the archives and packages first; the AppImage with
   `--appimage-extract`, the `.deb` with `dpkg-deb -x`, the disk image by mounting it):

   ```bash
   strings -a <binary> | grep -E '/home/runner/|/Users/runner/|[A-Z]:\\a\\|/\.cargo/registry/src/|/\.rustup/toolchains/'
   ```

   which must print nothing.
7. **Publish** the draft on GitHub.

## Making a change

- Keep changes small and focused, and add a test with them. A new detector needs a positive test (a
  generated token it must catch) and a negative one (a look-alike it must leave alone).
- `redactor-core`, `redactor-wire` and `scrub` forbid `unsafe` code. In the app, `unsafe` is allowed only
  under `app/src-tauri/src/platform/windows/` and `platform/macos/`, and in one library-loading call in
  `platform/linux/tray.rs`, with a `// SAFETY:` comment on each block.
- Code that talks to the operating system goes into `app/src-tauri/src/platform/<os>/`, behind the traits in
  `platform/mod.rs`. The shell (`app.rs`, `commands.rs`, `state.rs`, `tray.rs`) never uses `cfg(windows)` or
  an OS API; `tests/platform_boundary.rs` fails if it does.
- A new clipboard implementation passes the shared contract, `app/src-tauri/tests/support/clipboard_contract.rs`,
  on its own OS: add a runner like `tests/win_clipboard.rs`, `tests/x11_clipboard.rs` or
  `tests/macos_clipboard.rs` that supplies the clipboard and an `OtherApp`.
- Rules that differ by OS (hotkey names, the settings folder, shortcuts) take an `Os` value instead of
  `cfg`, so their tests run for every OS on any machine.
- Detection uses the `regex` crate only (linear time). There is no lookaround; check boundaries in code.
- Nothing may log, print or store scanned text, and a finding must never carry a secret value.
- Changes to the IPC contract touch the Rust side (`redactor-wire`, `commands.rs`, `capabilities/main.json`),
  the UI (`src/lib/types.ts`, `schema.ts`, the mock and its fixtures), the isolation page's allowlist
  (`app/isolation/index.js`, which also lists the event names) and the tests that pin each shape, together.
- Pin dependencies: Tauri and its plugins with `=`, npm packages by exact version, and commit lockfiles.
- Commit messages are short and in the imperative ("Add the Telegram bot token rule").
