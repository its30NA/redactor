# Threat model

Redactor exists so that a person can paste text into somewhere they do not fully control (a chat
assistant, a ticket, a forum) without pasting the passwords and keys inside it. This page says what it
protects, from whom, and where the protection stops.

## Assets

1. **The secrets in the text you paste.** Passwords, tokens, keys. This is what Redactor is for.
2. **The text itself** while it is in the app: input, output and the findings list.
3. **Secrets you ask Redactor to remember for the session.** They are held in memory so that they are
   redacted wherever they show up.
4. **Your settings**, including the always-redact word list (names, company names, internal host names).
5. **The clipboard**, which carries the raw text before and the safe text after.

## Who we defend against

| Attacker | Goal |
|---|---|
| **The recipient of the output** (an AI service, a support agent, a public thread) | Read a secret that survived redaction. |
| **Crafted input** (text from a web page, a log or a file you did not write) | Make Redactor miss a secret, hang, crash, or run something. |
| **Someone who gets at the machine later** (a stolen disk, a backup, a forensic look) | Find secrets in files the app left behind. |
| **Someone who sees the screen** (screen sharing, a screenshot tool, a recording) | Read the text in the window. |
| **A tampered download** | Make you run a build that is not the one published here. |

## What is protected, and how

**Leaking to the recipient.** The output is built from the redacted text only and is scanned again before it
is shown; anything that still matches is redacted too. A value found once is redacted at every position,
including its base64 (either alphabet), hex, percent-encoded and JSON-escaped copies, up to two
encodings deep, and it keeps one placeholder everywhere. Overlapping findings are merged, never dropped,
so no fragment of a secret is left behind. Findings carry offsets and labels, never values, so a value cannot be echoed back through
the findings list, the audit log or the JSON output (`crates/redactor-core/tests/api.rs`).

**Crafted input.**
- *Invisible and look-alike characters.* Every Unicode default-ignorable character (zero-width
  characters, variation selectors, fillers and the like), the interlinear annotation controls,
  bidirectional controls and tag characters are removed and Unicode is normalized before detection, so
  splitting a token with them does not hide it, and the output is stripped of them by default. They are also reported as a warning.
- *Encodings.* Base64, percent-encoding, JSON escapes and hex runs are decoded and rescanned, within a
  depth and byte budget, so a bomb of nested encodings costs bounded work and raises a warning. Copies of
  a value Redactor has already found are searched for in their encoded forms as well (see above).
- *Denial of service.* All matching uses the `regex` crate, which has no backtracking, and the rest of
  the engine (the search for copies of found values, the decode layers, the verify pass) is linear in
  time and memory too, also when found values nest or overlap. A release test,
  `adversarial_shapes_scale_linearly` (`crates/redactor-core/tests/perf.rs`), scans long lines,
  unterminated blocks, many distinct values, one value repeated, long base64, deep JSON and a mix at
  1 MiB and 8 MiB. It fails if the peak heap of a scan passes a bound set by the input size and the
  number of findings, if the 8 MiB scan holds much more than ten times the heap of the 1 MiB scan, if it
  takes more than 20 times as long, or if it runs slower than 1 MiB/s; the slowest shape ran at about
  3.9 MiB/s when last measured. The app refuses input over 20 MiB rather than scanning it (clipboard text
  over 20 MiB is measured in place and never copied out), and `scrub scan` skips files over 5 MiB and
  binary files, each with a line on stderr. A file counts as binary when at least 5 % of its first 8 KiB
  are control bytes, so a text file with a stray NUL is still scanned. `scrub check` never passes a staged
  text file it could not read: it blocks the commit instead.
- *Files in other encodings.* UTF-16 text (what Windows PowerShell 5.1 writes), with or without a
  byte-order mark, is decoded and scanned by `scrub` and the app, not skipped as binary.
- *Hidden changes in a commit.* `scrub check` lists staged files without rename detection and reads each
  one by its blob id, so a renamed-and-edited file or an odd file name (`0:other.env`) cannot slip past.
  Its git calls run at the top level of the repository with output-changing settings off (including
  replace refs). Content added with `git commit --amend` that is already in HEAD is not checked again.
- *A planted config file.* `scrub` searches for `redactor.toml` only up to the repository root or the
  home folder, names every config file it loads, refuses files writable by every user or owned by
  another user, and refuses catch-all allowlist patterns, including a pattern that matches every token that
  starts with one provider's prefix. `scrub check` prints every allowlist entry, disabled rule and custom
  placeholder template on each run, and exits 2 if the settings leave it unable to detect a set of
  common secrets at all.
- *UI injection.* The window has a content security policy of `default-src 'none'` with only
  `'self'` for scripts, styles, images and fonts, so no inline script, no `eval`, no remote content.
  The prototype is frozen, navigation is limited to the app's own origin and new windows are denied
  (`tauri.conf.json`, `app/src-tauri/src/app.rs`, `app/src-tauri/tests/config.rs`). IPC goes through
  the isolation pattern, whose hook (`app/isolation/index.js`) lets through only the UI's own calls,
  with their argument names, types and sizes checked; anything else is turned into a call that fails.
  WebRTC, which the CSP does not govern, is removed from the page. On Windows, WebView2 runs with
  non-proxied WebRTC UDP, DNS prefetching and background networking off
  (`app/src-tauri/src/browser_args.rs`); on Linux, WebKitGTK's WebRTC, media stream and DNS prefetching
  are off (`app/src-tauri/src/platform/linux/webview.rs`).
  Text from the input is shown as text in the editor, never as markup.
- *Memory safety.* Rust, with `unsafe` forbidden in the engine, the wire crate and the CLI.

**Traces on disk.**
- The app has no logging code and installs a panic hook that prints nothing, because a panic message could
  quote text. Release builds abort on panic. `scrub`'s panic hook prints one fixed line with the source
  location, never the message, and exits with code 101.
- Scanned text and remembered secrets are never written to disk. The only file the app writes is
  `settings.json` (on Windows `%APPDATA%\Redactor\settings.json`; for macOS and Linux see
  [below](#macos-and-linux-preview-what-is-different)), and it holds settings and the always-redact
  words. Those words are stored as plain text so that they can be shown and edited, and the app says so
  and warns when an entry looks like a password. A settings file the app cannot read is moved aside to
  `settings.json.bad` next to it, so the next save does not overwrite the list in it; delete that file
  when you no longer need it.
- The web view keeps no browsing data: WebView2's private mode on Windows, a non-persistent data store
  on macOS, an ephemeral one on Linux (with the files listed below). The UI uses no local storage.
- The app never keeps anything derived from your passwords: no hashes, no fingerprints, no encrypted
  lists. Anything like that on disk or in memory would let another program on the machine test password
  guesses against it. The one keyed hash in play, the finding's `ValueKey`, uses a random key that exists
  only in the process and is never persisted, so it is useless anywhere else.
- `scrub` writes a file only when you ask: `--write`, or `--audit PATH`. The audit records the time, the
  source name, counts, line numbers, labels and detector ids, never values, lengths, offsets or hashes.

**Screen capture (best effort, Windows only).** By default the window asks Windows to exclude it from
screenshots and screen sharing. This works with the capture methods Windows controls, and some tools or
drivers can bypass it. Treat it as a seatbelt, not a wall. It can be switched off in Settings or in the
tray. macOS and Linux have no such shield.

**Clipboard history (best effort, Windows only).** After the hotkey cleans the clipboard in place, the
app asks Windows to delete the raw item from the Win+V history. If history is off, the call fails, or the
cloud clipboard has already synced the raw copy, that copy may survive. Copies that a password manager
marked as private (`ExcludeClipboardContentFromMonitorProcessing`) are never read and never rewritten. A
copy made while a scan runs is never overwritten: the app compares the clipboard's sequence number after
opening the clipboard for writing, in the same session that writes, and `scrub clipboard` re-reads and
compares the text in the same helper process that writes it. The `scrub clipboard` command does not
touch Win+V history at all.

**Time limits.** Remembered secrets are zeroed, and both text panes are cleared, when you quit, lock
the screen, put the computer to sleep, or leave the app idle past the timeout (10 minutes by default,
selectable). On Linux a lock counts only when the desktop announces it (see below). "Idle" means no
call from the window to the app's backend: a scan, a copy, a settings change. Reading the output or the
findings without doing any of those does not count as activity, so the panes are also wiped after the
timeout while you are reading. The timer runs only while the app holds text or remembered secrets.

## What is not protected

- **A program running as you can read what is in the window.** This is the main limit. Any process of the
  same user can read the app's memory, take a screenshot with an API that ignores the capture setting, log
  your keystrokes, or read the clipboard at the moment you paste. Redactor keeps nothing beyond the text
  you pasted, and wipes it quickly, so it adds nothing for such a program to find, but it cannot stop one
  that is already there.
- **An attacker at your unlocked session, or a compromised operating system.** Out of scope.
- **Secrets the detectors do not recognize.** Detection is heuristic. A secret in an unusual format with
  no telling name near it can pass. That is why every finding is shown with its reason, why Paranoid mode
  exists, why you can select any text and redact it, and why you should read the output before you send
  it. Redactor reduces the risk. It does not replace looking.
- **Short values glued to other text.** A found value of 6 to 11 characters is redacted elsewhere only
  where it stands alone: a copy with a letter or digit right before or after it (`hostKx9mPq2vLz`,
  `Kx9mPq2vLz1`) is left. Values of 12 or more characters are found anywhere.
- **Remembered secrets inside short hex.** A remembered secret is not searched for in hex that is shorter
  than 32 digits or that does not decode to text. Its base64 copies are found.
- **Memory on crafted input.** Memory grows by about 1 KB per finding (8 MiB of `password=` lines, 349,464
  findings: 335 MiB peak, 585 MiB with `--json`), so a crafted 20 MiB paste can use close to a gigabyte
  for that scan.
- **Other formats of a clipboard copy.** The hotkey reads and scans only the plain text of a copy. When
  it removes something, it replaces the whole copy with the safe plain text. When the verdict is clean,
  it leaves the copy as it is, so its other formats (HTML, RTF and the like) were never looked at: a
  secret that is only there, in hidden markup say, can still be pasted into an app that takes that
  format. This holds on every OS.
- **Crash dumps on Windows.** Nothing in the app turns off Windows Error Reporting or the WebView2
  crash reporter. If the app or its web view crashes, the dump can contain the text that was in memory
  (the window's text, remembered secrets), and Windows may keep it on disk. Redactor does not write dumps
  itself and cannot delete the ones Windows writes. For macOS and Linux see below.
- **`--amend`.** `scrub check` does not re-check content that is already in HEAD when you amend a commit.
- **Values after a placeholder.** A value written right after a placeholder with no gap
  (`[REDACTED: Password #1]Juniper-Coast-55`) is redacted. One that follows it after punctuation or a space
  (`[REDACTED: Password #1]!abc`, `[REDACTED: Password #1] Juniper-Coast-55`) is judged like any other text,
  and may be kept.
- **Not every copy of your text is zeroed.** What is zeroed, on the Rust side: remembered secrets (the
  stored values, and the copies made while normalizing and comparing them), the text of a scan request
  when the request is dropped, clipboard text read for a scrub, the text given to Copy, Save and Remember,
  and the bytes of a file you open. What is not zeroed:
  - inside the engine: the shadow text it scans, decoded layers (base64, percent-encoding and the like),
    the decoded copies and the index built from found values to find their other occurrences, and the
    output strings, including the response sent back to the window;
  - Tauri's IPC buffers, including the payloads the isolation pattern encrypts and the shell decrypts;
  - JavaScript strings: the text you paste lives as strings in the web view's process, which the garbage
    collector frees when it likes. The wipe replaces the editor contents and drops the history, which is
    as far as a web view allows.
  These copies are freed normally and can stay in memory until it is reused.
- **Remembered secrets and the swap file.** Memory holding a remembered secret can be paged to disk by
  the operating system (the page file on Windows, swap on macOS and Linux). Zeroing on drop limits the
  time a value sits in memory, not what the operating system does with it meanwhile.
- **Other software on the path.** Redactor cannot see what your browser, terminal or chat client does with
  text before or after. The WebView2 runtime is a Microsoft component with its own update mechanism; the
  installer downloads it from Microsoft when a PC does not have it. The
  app's content security policy keeps the app's own page from using the network, but does not govern the
  runtime's updates. On macOS the web view is the system's WebKit, on Linux the distribution's
  WebKitGTK; each is updated with the system.
- **Metadata.** Placeholders keep structure: labels, line breaks and which values are equal (`#1` twice).
  That is deliberate, so the output still reads well, but it reveals that two places share a value.
- **The always-redact list.** It is plain text on disk. Do not put passwords in it. Use "Remember" for those.
- **The CLI's `--diff` output** shows the original lines, secrets included. Do not share it.

### Known misses (2.1.0)

These shapes pass on purpose or for lack of a cheap signal. Each was measured with `scrub` 2.1.0; the
values are made up. For any of them, add the value as an always-redact term, select it and redact it, or
read the output before you send it. Fixes are tracked in the [roadmap](ROADMAP.md).

| Input | Balanced | Paranoid | Why it passes |
|---|---|---|---|
| `PRIVATE_NOTE="Juniper-Coast-55"` | kept | kept | `NOTE` is not a secret name; flagging every password-shaped value under any key would hit ordinary config. |
| `deploy key note: kT9#mQ2$vL` | kept | kept | Too short for the randomness check, and no key name it belongs to. |
| `#Harbor91` on its own line | kept | kept | A line that starts with `#` is read as a dotenv comment. |
| `commit 3f786850e387550fdab836ed7d6dc8f5c6e1c8a2` | kept | kept | 40 hex digits after a word like `commit` read as a git commit id. Without such a word, Paranoid redacts the hex; Balanced keeps it unless a key name comes first. |
| `Passwort: siehe E-Mail` | kept | kept | A standalone `see`, `siehe`, `voir`, `ver` or `vedi` after a password key marks a pointer, not a value. A real password that is one of these words followed by text leaks; `siehe1234` is still redacted. |
| `mysql -u root -pROOTPASS` | kept | kept | An all-capital, letters-only value after `-p` reads as a placeholder from usage text (`-pPASSWORD`). |
| `DB_PASSWORD='$DB_PASS'` | kept | kept | A `$` plus an all-capital name reads as a variable reference, also in single quotes (Docker Compose expands it there). Mixed case such as `'$ummer2024'` is redacted. |

## macOS and Linux (preview): what is different

The macOS and Linux apps are previews ([macOS](install/macos.md), [Linux](install/linux.md)). Everything
above holds for them too, except what this section says. The macOS app is tested by machines on macOS 15
and 26 and has not yet been used by hand. The Linux app is tested by machines on X11 and sway, and by hand
on WSLg; GNOME and KDE Plasma are not tested end to end. Where a line below says "not tried on a real
Mac", it is what the code does, not what anyone has seen.

**Settings.** The file is `~/Library/Application Support/Redactor/settings.json` on macOS and
`$XDG_CONFIG_HOME/redactor/settings.json` (or `~/.config/redactor/settings.json`) on Linux. A variable
that is unset, empty or not an absolute path means no file at all: the app runs on defaults and never
writes into the folder it was started from. The app's own folder is created readable by you alone
(0700), and a folder that already let others in is tightened; the file and `settings.json.bad` are 0600.
A shared parent such as `~/.config` is never changed. The rights are set when the app saves, so a file
left by an older build stays as it was until the next settings change.

**No screen-capture shield.** On macOS, since 15.4, screen capture ignores the window flag the shield
would set; Linux has no such flag. The setting is off and greyed out, and Settings says why. Anyone who
sees your shared screen or a recording sees the window.

**No clipboard history cleaning.** Neither OS lets an app delete one item from a clipboard history, so
the scrub result never says the history was cleaned. On macOS the raw copy can stay in a clipboard tool,
in Spotlight's clipboard history (macOS 26, off by default) and on your other Apple devices through
Universal Clipboard. On Linux a clipboard manager (Klipper, GNOME extensions, `cliphist` and others)
keeps its own history, often on disk. All of these are out of Redactor's reach. Copy the raw text only
where you need it, or turn the history off.

**The primary selection.** On X11 and Wayland, selecting text also puts it in the primary selection,
which a middle click pastes. Redactor reads and cleans only the clipboard. Text you selected stays
pasteable with a middle click, raw.

**Desktops where the clipboard needs the window in front** (GNOME, Weston, WSLg). The hotkey or
`--scrub-clipboard` brings the window forward and cleans when it is in front. Until then the raw text
is on the clipboard and can be pasted. The wait ends after 30 seconds: the clipboard is left raw, a
desktop notification says "Not cleaned", and the window shows it with **Clean now** (at once, and again
when you next bring it forward within 10 minutes). WSLg shows no notifications. The window's toolkit
(GTK) keeps its own copy of the text it read, which Redactor cannot wipe, and reads a copy over 20 MiB
in full before the size check refuses it.

**When the clean text is pasteable.** On Wayland and on X11 the text Redactor puts on the clipboard lives
in Redactor: after you quit it is gone, unless a clipboard manager took it over (on X11 it is offered to
one). On macOS it stays after quit. The write is not atomic on Wayland or macOS: a copy you make in the
very same instant can be lost under the safe text of the earlier copy. It is never a secret that ends up
on top. Forced 2000 times in tests, this happened in up to 2.25 % of rounds on a local sway and at most
9 times on CI's macOS runners.

**Core dumps.** The app sets its core-file limit to zero first thing at start, on Linux and macOS, so a
crash does not put its memory (clipboard text, both panes, remembered secrets) in a core file, and
systemd-coredump does not store one. Processes the app starts itself inherit the limit, which on Linux
includes the web view's. Not covered: a crash reporter that does not honour the limit, the system's own
crash reports on macOS, and the macOS web view's processes, which the system starts rather than
Redactor, so the limit may not reach them (not checked).

**Wipe on lock on Linux.** The panes and remembered secrets are wiped on these signals: logind's
`PrepareForSleep` (going to sleep), the session's `Lock` and its `LockedHint` turning true, and
`ActiveChanged` turning true on `org.freedesktop.ScreenSaver`, `org.gnome.ScreenSaver`,
`org.cinnamon.ScreenSaver`, `org.mate.ScreenSaver` or `org.xfce.ScreenSaver`. If a bus restarts, the app
listens again; a bus that is not there when the app starts is not watched at all. A locker that
announces nothing does not trigger the wipe: `swaylock` or `i3lock` started directly, and locking
Windows while the app runs in WSLg. The idle timer is then the only backstop. On sway, lock through
logind instead, so the lock is announced: bind your lock key to `loginctl lock-session` and let swayidle
start the locker (`swayidle -w lock 'swaylock -f'`).

**Wipe on lock on macOS.** The screen-locked notification (which Apple does not document), displays
going to sleep, the Mac going to sleep and switching to another user all wipe. Tested with posted
notifications only, not with a real lock.

**The web view's files on Linux.** The web view keeps no page data, but WebKitGTK still creates
`~/.local/share/io.github.its30na.redactor` with `hsts-storage.sqlite`, files named `salt` and empty
cache folders. The Linux smoke tests check after each run that the app left nothing else in its XDG
folders and that no password is in any of them. Delete the folder when you remove the app.

**macOS: pasteboard privacy.** Newer macOS versions can ask whether Redactor may paste from other apps.
If you choose "Don't Allow" and macOS still lists the text on the clipboard but hands over none of it,
Redactor says "Clipboard not scrubbed" and leaves the clipboard as it was; it does not read that as an
empty clipboard. If macOS hides even the list of formats, the app sees no text and says "Nothing to
scrub", although the raw text is still there. Which of the two a refusal gives has not been tried on a
real Mac.

**macOS: the single-instance socket.** To hand a second launch to the running app, the single-instance
plugin uses a socket at `/tmp/io_github_its30na_redactor_si.sock`, in a folder every user of the Mac
shares. When something at that path belongs to another user, Redactor does not use it: it starts without
the single-instance check, says so once on stderr, and a second launch opens a second window. Another
user therefore cannot stop Redactor from starting. Not tried on a real Mac. On Linux the plugin uses your
own session bus instead.

**The previews are not release downloads.** The macOS app is a CI build, kept for 7 days, signed only ad
hoc (not by an Apple developer account, not notarized); the Linux app is built from source. They come
with no `SHA256SUMS` and no build attestation, so the defence against a tampered download described
[below](#the-unsigned-binary-and-smartscreen) does not cover them yet. Download the build only from this
repository's own Actions page, or build it yourself.

**Advisory exceptions.** The Linux build pulls in Tauri's GTK3 and D-Bus stack, which has five open
advisories that `cargo deny` is told to accept (`deny.toml`). Each is re-checked at every Tauri update and
before every release.

| Advisory | Crate | Why it is accepted |
|---|---|---|
| RUSTSEC-2024-0429 | glib 0.18 | `VariantStrIter` is unsound (fixed in 0.20). Tauri's GTK3 stack pins 0.18, and Redactor never calls it. |
| RUSTSEC-2024-0370 | proc-macro-error | Unmaintained. A build-time macro of glib 0.18; no code of it runs in the app. |
| RUSTSEC-2026-0370 | zvariant | Unmaintained (merged into zbus 6). zbus 5 still uses it, and the Tauri notification and single-instance plugins need zbus 5; Redactor's own D-Bus code (lock watcher, tray check, hotkey portal) uses the same zbus 5. |
| RUSTSEC-2026-0371 | zbus_names | Unmaintained (merged into zbus 6), same reason. |
| RUSTSEC-2026-0372 | zvariant_derive | Unmaintained (merged into zbus 6), same reason. |

## The unsigned binary and SmartScreen

The Windows installer and the executable are **not code-signed yet**. Free code signing through
[SignPath Foundation](https://signpath.org/) is planned (see the [code signing policy](../README.md#code-signing-policy));
until it is in place, Windows SmartScreen warns on first run ("Windows protected your PC"), and the
executable shows no verified publisher. A signature would show who published a file; it would not prove the
code has no bugs.

What replaces the signature here:

- Releases are built only by GitHub Actions from a tagged commit on `main`, never uploaded from a
  developer's machine, and the runner's checkout path is remapped out of the binaries.
- Each release has `SHA256SUMS` and build provenance attestations, so you can check that a file is
  byte-for-byte what the workflow produced (`gh attestation verify`, see the [README](../README.md#verify-the-download)).
- The source is public, dependencies are pinned and checked with `cargo deny`, and you can build it yourself.

A SmartScreen prompt is not proof of a problem, and a missing prompt is not proof of safety. Verify the
checksum or the attestation before you click "Run anyway". The attestation proves where the file came
from, not that the code has no bugs.

## Reporting a problem

Use GitHub's private vulnerability reporting. See [SECURITY.md](../SECURITY.md).
