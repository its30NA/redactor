# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- The desktop app on macOS and Linux (X11 and Wayland), in preview: machine-tested, little hand use yet.
  Five new downloads, each in `SHA256SUMS` and with a build attestation like the Windows files:
  `Redactor_<version>_universal.dmg` (one app for Apple silicon and Intel, macOS 13 or later),
  `Redactor_<version>_amd64.deb`, `Redactor-<version>-1.x86_64.rpm`, `Redactor_<version>_x86_64.flatpak`
  and `Redactor_<version>_amd64.AppImage` (Linux x86_64; the `.deb`, `.rpm` and AppImage need glibc 2.35
  or newer). The
  [install guides](docs/install/linux.md) say which to pick, how to verify, install and remove each, and
  the [threat model](docs/THREAT_MODEL.md#macos-and-linux-preview-what-is-different) lists what differs
  from Windows.
- `Redactor --version` prints the version and exits, before any window opens. On Windows the app has no
  console, so the line shows only when the output is redirected to a file or a pipe.
- Inside the Flatpak, Settings shows the command to bind as
  `flatpak run io.github.its30na.redactor --scrub-clipboard`, the one that works from outside the sandbox.
- The window starts in the theme's background colour instead of flashing white before the page is drawn
  (dark for the dark theme, light for the light theme, and for the system theme the desktop's light or
  dark setting; dark where the desktop does not say). The page also starts in a chosen Dark or Light
  theme, so a theme that differs from the system's no longer shows the system's colours for a moment.
- `--scrub-clipboard`: starting the app with it scrubs the clipboard like the hotkey, through the running
  copy or in a new one. Where the desktop lets no app hold a key, Settings shows this command with the
  app's full path and a Copy button, to bind in the desktop's keyboard settings.
- Scrub notices in the window, next to the desktop notification: the result of every scrub, "Waiting for
  Redactor's window to scrub the clipboard…" where the clipboard needs the window in front, and "Not
  scrubbed" or "Clipboard not scrubbed" with a **Scrub now** button. A failed notice leaves after a later
  result or 10 minutes.
- The command palette has "Scrub the clipboard now", also over an open sheet. Esc on a focused notice
  dismisses only that notice.
- A hotkey that could not be set at start, or that the desktop dropped, shows as "Not active: pick another
  key." in Settings, and the window says so with an **Open Settings** button.

### Changed

- The desktop shell reaches the operating system through one platform layer.
- The hotkey scrub removes zero-width, bidi and tag characters from a copy with no secret in it, as
  `scrub clipboard` does, and says "Hidden characters removed" (with **Strip invisible characters** on,
  the default).
- "Scrub" is the verb in every app text ("Scrub now", "Not scrubbed"), and the window's notice and the
  desktop notification use the same words for the same result.
- A scan that could not be verified and removed nothing says "Check before pasting", not "Clipboard
  scrubbed".
- A file that cannot be opened says why, and the size cap reads 20 MiB everywhere.
- CI builds and tests the app shell on Linux and macOS too, and checks the dependency policy for both with
  the shell included. On Linux this allows the ISC licence and five advisories inside Tauri's GTK3 and D-Bus
  stack, each with its reason in `deny.toml`.
- CI builds every download on every pull request with the release's own scripts, and installs and starts
  each one on a clean system: the `.deb` on Ubuntu 22.04, 24.04 and Debian 12, the `.rpm` and the Flatpak on
  Fedora 45, the AppImage on Ubuntu 22.04 and Fedora 45, the disk image on Apple silicon and Intel Macs.
  Each check also looks for the license, the notices and the build machine's paths inside the package.

### Fixed

- Detection: a file path with a generated folder name (macOS temp folders under `/var/folders`, Xcode
  DerivedData, a timestamped release folder) is no longer a Random String in Balanced. A path is judged by
  the names in it, not as one run with its slashes; a random name inside a path and a secret with slashes
  in it are found as before, and Paranoid reports what it reported.
- The idle wipe counts time asleep, so text and remembered secrets do not outlive a night's sleep.
- A settings file that cannot be read is kept as `settings.json.bad` instead of being overwritten at the
  next save, and the window says so. Settings that cannot be saved say so once instead of failing
  silently.
- A key refused in Settings says why under the key until the next try, together with "Not active: pick
  another key." when no hotkey is set.
- On a Mac, the app's messages about the hotkey use the Mac's key names (Control+Option+R), like the
  keycaps.
- The welcome card and the shield promise wait until the app knows what the desktop can do.
- A notice meant to stay until dismissed is no longer pushed out by newer ones.

### Security

- The window loads `http://tauri.localhost` only without a port and never over https.
- Opening a pipe, a device or a file that keeps growing no longer reads without limit: only regular files
  are read, up to 20 MiB.
- On macOS and Linux the settings file is readable by you alone (0600, in a 0700 folder), and a crash
  writes no core file with the app's memory in it.
- The Flatpak asks only for the permissions the app uses: the window (Wayland, X11 only as a fallback),
  GPU drawing, the tray, notifications and the lock signals. No network, no file system access but the
  tray icon's own folder (files go through the desktop's file dialog), no sound. CI fails if the list
  changes.
- The AppImage's build tools are pinned by checksum, and it carries the license of every library it
  bundles and of the parts that start it (the AppImage runtime, AppRun and the GTK hook script).

### Known limits

On macOS and Linux:

- No screen-capture shield: macOS 15.4 and later ignore the window flag it would set, and Linux has none.
  Anyone who sees your shared screen sees the window.
- No clipboard history cleaning: the raw copy can stay in a clipboard manager, in Spotlight's clipboard
  history (macOS 26, off by default) and on your other Apple devices through Universal Clipboard.
- A copy you make in the very same instant as a scrub can be lost under the safe text of the earlier copy
  (macOS and Wayland; at most about 2 in 100 forced tries). It is never a secret that ends up on top.
- Some copies of clipboard text that the system or the window toolkit keeps cannot be wiped by Redactor.
- Wiping on a screen lock needs a lock the desktop announces: `swaylock` or `i3lock` started directly, and
  locking Windows while the app runs in WSLg, go unseen. Settings says when no lock signal is found;
  suspend and the idle timer still wipe.
- A crash reporter that ignores the core-file limit could still keep the app's memory; the stronger Linux
  setting is not used because it may break the GNOME and KDE hotkey.
- No automatic updates, no Linux build for ARM, and no package repository (apt, dnf, Flathub, Homebrew):
  download each new release.

macOS:

- Signed only ad hoc and not notarized: the first start needs **Open Anyway** in Privacy & Security.
- macOS gives no warning when another app already uses the hotkey; if it does nothing, pick another.
- If you choose **Don't Allow** at macOS's "paste from other apps" prompt, the hotkey cannot read the
  clipboard. Redactor should say "Clipboard not scrubbed"; it may say "Nothing to scrub" instead.
- The lock signal macOS sends is not documented by Apple.
- The menu-bar icon is the app icon in colour, and the Edit menu keeps Services, AutoFill and Writing
  Tools.
- If another user of the Mac holds the single-instance socket in `/tmp`, a second launch opens a second
  window instead of handing over.

Linux:

- On GNOME, Weston and WSLg the clipboard works only with Redactor's window in front: the hotkey brings
  the window forward first and gives up after 30 seconds. WSLg shows no notifications.
- Where the desktop has no shortcut service (sway, older GNOME and KDE, WSLg), you bind the command
  Settings shows in the desktop's own keyboard settings. GNOME 48+ and KDE Plasma 6 ask once to allow the
  key and may change it. A shortcut the desktop drops is reported once and not set again by itself.
- Text Redactor put on the clipboard is gone when it quits, unless a clipboard manager keeps it.
- Middle-click paste (the primary selection) is not scrubbed.
- A tray that appears after Redactor started is seen only at the next start; without a tray, closing the
  window quits.
- **Copy safe text** reports the clipboard as busy, instead of trying again, when another app copies in
  the same millisecond (X11).
- The AppImage's own libraries (WebKitGTK and about 170 others from Ubuntu 22.04) get security fixes only
  with a new Redactor release; the `.deb`, `.rpm` and Flatpak use your system's or Flathub's.
- The `.deb` and `.rpm` are not signed with a GPG key; the attestation proves where they came from.
- Flatpak: on desktops that hide the background clipboard from sandboxed apps, it works only with the
  window in front. On an X11 session the sandbox does not separate it from other apps' clipboard and
  keys.

### Not tested by hand

These were tested by machines only, or not at all. Tell us what you find.

- The macOS app: nobody has used it by hand yet, including the disk image and **Open Anyway**. Machines
  test it on macOS 15 and 26; macOS 13 and 14 are not tested. A real screen lock, sleep and switching
  users are tested only with posted signals, and file dialogs and notifications not at all.
- All four Linux downloads on a real desktop: the `.deb`, `.rpm`, AppImage and Flatpak are installed and
  started only in CI's clean containers.
- The Flatpak's tray, notifications, shortcut service and lock signals inside the sandbox.
- GNOME and KDE Plasma end to end, including their shortcut dialogs; Hyprland.
- On WSLg: the hotkey command with the window minimised, clicking another window right after it, and
  whether the scrubbed text stays on the clipboard after quitting.
- Handing the clipboard to a real clipboard manager on X11 (GNOME, KDE, Xfce) when Redactor quits.
- The first frame's colour on Windows and on a real Mac, also with the system theme on a light desktop.

## [2.1.1] - 2026-11-22

### Fixed

- Detection: a value that opens a quote and never closes it, at the end of a line with Windows line endings
  (`\r\n`), made the engine panic. The quoted value now ends before the `\r\n`.
  Found by the property tests; present since 2.0.0.

### Added

- A "Known misses" table in the threat model: shapes that 2.1.0 keeps on purpose or cannot detect, each
  measured at Balanced and Paranoid, with the workaround.
- A roadmap (`docs/ROADMAP.md`): the macOS and Linux app, fixes for the known misses, and an optional
  local-model judge under research.

### Security

- Dependencies: `url` 2.5.9 and `form_urlencoded` 1.2.3 (RUSTSEC-2026-0592: a stack overflow in
  `Url::origin()` on nested `blob:` URLs). The crate comes in through the Tauri shell; Redactor's own code
  does not call `Url::origin()`.

## [2.1.0] - 2026-11-11

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

[Unreleased]: https://github.com/its30NA/redactor/compare/v2.1.1...main
[2.1.1]: https://github.com/its30NA/redactor/compare/v2.1.0...v2.1.1
[2.1.0]: https://github.com/its30NA/redactor/compare/v2.0.0...v2.1.0
[2.0.0]: https://github.com/its30NA/redactor/releases/tag/v2.0.0
