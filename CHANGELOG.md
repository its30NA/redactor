# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- `scrub.exe` carries a Windows version resource: product name, version, description and copyright.
- A code signing policy and privacy statement in the README.
- README sections for installing and uninstalling `scrub`, a longer verification recipe, and an "at a glance"
  line. Issue forms for a missed secret, a false alarm and a bug, and a pull request template.
- Detection: a key after a comma or parenthesis takes a literal value (lists in prose, struct dumps,
  `helm --set`, mount options).
- Detection: Markdown emphasis, inline HTML tags, table cells and secret-named table or CSV columns.
- Detection: name/value siblings (Kubernetes env, ECS, Azure, HAR, Postman, .NET settings) and nested `value`
  fields.
- Detection: long or digit-bearing key names (`OAUTH2_PROXY_CLIENT_SECRET`, `PG15_PASSWORD`) are keys.
- Detection: word-like passwords (`Temp1234`) on config lines and in JSON are redacted.
- Detection: more prose connectors in English, German, Italian, French and Persian, and the value before
  "is the password".
- Detection: secrets in `define`/`getenv`/`setdefault` calls, setters, fallbacks, Terraform plans, PHP dumps and
  SQL `INSERT`.
- Detection: "user / password" after Credentials/Zugangsdaten/Identifiants, User/Pass or Login:, and "u: x p: v".
- Detection: `(user: x, pw: Temp1234)` in prose is redacted.
- Detection: password arguments of about 20 more tools, secrets piped into `sudo -S`, `--password-stdin` and
  `chpasswd`, `bindpw`/`rootpw` and Cisco secret lines.
- Detection: URL passwords containing `/`, `?` or `#`, token-only userinfo, Oracle thin JDBC logins, Zapier
  hooks, any `*_token` query parameter and webhook URL keys.
- Detection: words like `id`, `file`, `provider` or `input` before a key's head no longer void it
  (`APPLE_ID_PASSWORD`, `INPUT_PASSWORD`); Vault `secret_id`.
- Detection: more key words (`dbpass`, `psw`, `psk`, `authtok`, `passkey`, Schlüssel, clé, jeton, کلید, raw
  `Authorization` values).
- Detection: all-digit passwords on config lines and in JSON, passcode/OTP/CVV codes, short and letterless
  config passwords, logfmt words.
- Detection: config values with operator-like characters, YAML tag/alias marks and `${VAR:-default}` defaults;
  passphrases with common words.
- Detection: tokens right after `\n`/`\t` escapes or ANSI colour codes, and escaped JSON inside JSON strings.
- Detection: XML namespace prefixes, attributes, CDATA, and `<property name=…>`/`<entry key=…>` element text.
- Detection: argv arrays, arrays of secrets, values on the next line, YAML block scalars, triple quotes,
  Kubernetes Secret data, decoded kubectl output, `user:password` and `sasl_passwd` lines.
- Detection: Databricks, Slack client, ElevenLabs and HubSpot tokens; TOTP seeds near a QR code; license keys.
- Detection: connection-string passwords with quotes, JWE, wrapped JWTs, dotenv passphrases with options.
- Detection: `'$ummer2024'` in single quotes and `Config.2024!x` are redacted.
- Detection: both passphrases of `ssh-keygen -P … -N …`.

### Changed

- The README and the threat model say that the installer downloads Microsoft's WebView2 runtime when a PC
  does not have it.
- The app's window may call 13 commands, not 14: the unused `scrub_clipboard` command is gone. The hotkey
  and the tray menu are unchanged.
- The hotkey and tray scrub say "Check before pasting", not "safe to paste", when the output could not be
  verified. The app strips the same invisible characters from remembered secrets as the engine does from text.
- Values with digits mixed into the letters (`$ummer2024x`, `Tr0ub4dor-here`, `config.Zq81mXv0pLr3`,
  `%Tgb6yhn7%`) are redacted even when they look like a variable, template or config path.
- Text framed like the placeholder template is redacted unless its label is one the engine emits (text that
  only looks like a placeholder, such as `[REDACTED-…`, is redacted). `scrub check` warns about a custom
  `template`.
- `scrub` treats a file as binary when at least 5 % of its first 8 KiB are control bytes, not at the first NUL.
  `scan --write` never rewrites a file that contains NUL bytes.
- `scrub` refuses allowlist patterns that match every token that starts with one provider's prefix, `scrub check`
  prints every setting that lowers detection on each run, and a config that leaves `check` unable to detect
  common secrets makes it exit 2.
- `scrub scan` and `scrub check` run git at the repository's top level with output-changing settings off,
  replace refs included.
- `scrub` prints every engine warning, naming the input. An encoded part left undecoded, or output that could
  not be verified clean, exits 2 when nothing was found (`scan` and `check` keep exit 1 when they found secrets).
- Releases are built only after CI is green for the tagged commit and `cargo deny` passes. The release job
  runs the Windows tests, the smoke test redacts a made-up secret, and the cargo and rustup folders are
  remapped out of the binaries.
- Detection: `"auth": "required"` and other scheme names, SSH public keys, Kubernetes pod names and
  secret-store references (`vault:path`, a Secrets Manager ARN) are no longer flagged. Pagination tokens
  (`next_page_token`) are no longer secrets, and `Passwort: siehe E-Mail` is no longer redacted.

### Fixed

- Help-text metavariables (`-p [DATABASE]`, `-pPASSWORD`, `STRING`) are no longer redacted as passwords.
- JS/PHP/Java statements joined by `;` are no longer read as connection strings.
- `x=1;token=…` and `a&&pass==b` in code are no longer query parameters.
- `grep -n` and compiler output, IPv6 addresses and `passwd` lines are no longer `.pgpass` passwords.
- Readable query values (paths, promo codes, locales) and Windows SIDs are no longer keys or session ids.
- The npm registry rule no longer flags `self._password = ...` in code.
- `Password=${VAR};` is read as a template, not a password.
- An escape JSON does not know (`\x`) no longer stops later `\u` escapes from being decoded.
- The app's quick toggles can no longer leave the screen shield off while the window says it is on.

### Security

- The search for copies of found values is linear in time and memory, also when values nest or overlap
  (649 KB: 27 s to 0.07 s; 98 KB: 1.33 GB to 21 MB).
- Base64 wrapped across lines (`base64 -w`, MIME, PEM, YAML block scalars; LF or CRLF) is decoded whole: a
  secret split by a line break is found, and so are copies of found values.
- Every Unicode default-ignorable character (and the interlinear annotation controls) is removed before
  detection; U+FE0F stays only after an emoji.
- Remembered secrets and always-redact terms are also found in their base64, hex, percent-encoded and
  JSON-escaped copies. Always-redact terms are found there even when the text has no plain copy, also glued
  to other characters.
- Base64 glued to a URL path (`/cb/<base64>`, `<base64>/next`) is decoded and scanned.
- A value glued right after a placeholder (`secret = [REDACTED: Password #1]Juniper-Coast-55`) is redacted.
- At the verify limit the last round's findings are still redacted, and the output is marked as not verified.

## [2.0.0] - 2026-11-07

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

[Unreleased]: https://github.com/its30NA/redactor/compare/v2.0.0...main
[2.0.0]: https://github.com/its30NA/redactor/releases/tag/v2.0.0
