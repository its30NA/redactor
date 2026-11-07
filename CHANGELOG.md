# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- `scrub.exe` carries a Windows version resource: product name, version, description and copyright.
- A code signing policy and privacy statement in the README.

### Changed

- The README and the threat model say that the installer downloads Microsoft's WebView2 runtime when a PC
  does not have it.

## [2.0.0] - 2026-11-06

Version 2 is a rewrite. Version 1 was a Python command-line tool with a browser interface on a local
server. Version 2 is a Windows desktop app and a command-line tool on one new detection engine written in
Rust.

### Added

- A Windows desktop app (Tauri 2) with its own window, a tray icon and a per-user installer that needs no
  administrator rights. There is no browser, no server and no open port.
- A review interface: every finding shows what it is, why it was flagged and a confidence, grouped as
  Certain, Likely and Possible. You can Keep a finding, redact any selection, or Remember a value.
- Remembered secrets for the current session, held in memory only and wiped on quit, lock, sleep and idle.
  A separate "Always redact" word list for names and host names.
- A global hotkey (`Ctrl+Alt+R`) that cleans the clipboard in place, leaves copies marked private by a
  password manager alone, and removes the raw copy from Windows clipboard history on a best-effort basis.
- A screen-share shield (the window is excluded from capture, best effort), and auto-wipe after idle time, on
  lock and on sleep.
- A new detection engine, `redactor-core`, with 170+ rules: provider tokens for AI, code-hosting, cloud and
  SaaS services; key material; password hashes; HTTP and URL credentials; connection strings.
- A context engine that reads key names as whole words (so `bypass` never matches `pass`) and handles JSON,
  YAML, XML, INI, shell, PowerShell, SQL, netrc and prose in English, German, French, Spanish and Persian.
- A small trained classifier for random-looking tokens that have no key name.
- A decode layer: secrets inside base64, percent-encoding, JSON escapes and hex are found.
- Propagation: once a value is found, every other occurrence is redacted, with the same placeholder number.
- Shadow-text normalization: zero-width, bidirectional and tag characters and fullwidth forms no longer hide
  a secret, and are stripped from the output.
- A verify step that scans the output again before it is shown.
- Sensitivity levels (Relaxed, Balanced, Paranoid) and a labeled or generic placeholder style.
- `scrub` options `--json`, `--sensitivity`, `--pii` and `--no-strip`, and the config keys `sensitivity`,
  `strip_invisible`, `terms` and `placeholder`.
- `scrub` builds for Windows, Linux and macOS.
- Releases built only by GitHub Actions, with `SHA256SUMS` and build provenance attestations.
- Documentation: user guide, architecture, threat model, detector catalog and accuracy report.

### Changed

- The engine is Rust. The CLI keeps its commands, flags and exit codes: `scrub [FILE]`, `scan`, `check`,
  `install-hook`, `clipboard`, `--summary`, `--preview`, `--diff`, `--audit`. Existing `redactor.toml` files keep
  working.
- `scrub check` now reads the staged blobs, so it checks what will be committed rather than the working
  tree.
- The audit log records counts, labels, line numbers and detector ids only. Version 1 stored a salted
  fingerprint of each value; version 2 stores nothing derived from a secret.
- Overlapping findings are merged into their union and never dropped, which closes a way for a prefix of a
  secret to survive.
- Detection was rebuilt after a probe in which version 1 missed 17 of 22 realistic fake-secret formats,
  among them JSON `"password"` values, YAML values with spaces, German "Passwort ist ...", `curl -u`,
  `mysql -p`, XML `<password>`, netrc, PGP private key blocks and secrets split by zero-width characters.
  It also left the second occurrence of a detected password in clear text.

### Removed

- The Python implementation and the local web interface (`scrub ui`).
- The optional local language-model pass (the `[llm]` config section is read and ignored with a notice).
  Version 2 contains no network code at all.

### Security

- The app's window may only call its own 14 commands. The content security policy allows no remote content,
  no inline script and no `eval`. IPC uses the isolation pattern.
- No logging, no stored text and no stored secrets. The only file is a settings file. A panic never prints
  its message: the app prints nothing, `scrub` one line with the source location (exit code 101).
- The isolation hook lets through only the UI's own IPC calls, with argument checks. WebRTC is removed from
  the page, and WebView2 runs without non-proxied WebRTC UDP, DNS prefetching or background networking.
- `scrub check` lists staged files without rename detection and reads them by blob id, so renamed, copied
  and oddly named files are checked; it blocks the commit when it cannot read a staged text file.
- `scrub` and the app decode UTF-16 text files instead of skipping them as binary; `scrub` reports every
  file it skips and writes files back atomically, in their own encoding.
- `scrub` names every config file it loads, stops searching at the repository root or the home folder,
  refuses config files other users could have planted and catch-all allowlists, and `scrub check` warns
  about settings that lower detection.
- Clipboard scrubs never overwrite a copy made during the scan, and clipboard text over 20 MiB is refused
  before it is copied.
- The engine, the wire crate and `scrub` forbid `unsafe` code.
