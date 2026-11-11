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
  WebRTC, which the CSP does not govern, is removed from the page, and WebView2 runs with non-proxied
  WebRTC UDP, DNS prefetching and background networking off (`app/src-tauri/src/browser_args.rs`).
  Text from the input is shown as text in the editor, never as markup.
- *Memory safety.* Rust, with `unsafe` forbidden in the engine, the wire crate and the CLI.

**Traces on disk.**
- The app has no logging code and installs a panic hook that prints nothing, because a panic message could
  quote text. Release builds abort on panic. `scrub`'s panic hook prints one fixed line with the source
  location, never the message, and exits with code 101.
- Scanned text and remembered secrets are never written to disk. The only file is
  `%APPDATA%\Redactor\settings.json`, and it holds settings and the always-redact words. Those words are
  stored as plain text so that they can be shown and edited, and the app says so and warns when an entry
  looks like a password.
- The window runs in WebView2's private mode, and the UI uses no local storage.
- The app never keeps anything derived from your passwords: no hashes, no fingerprints, no encrypted
  lists. Anything like that on disk or in memory would let another program on the machine test password
  guesses against it. The one keyed hash in play, the finding's `ValueKey`, uses a random key that exists
  only in the process and is never persisted, so it is useless anywhere else.
- `scrub` writes a file only when you ask: `--write`, or `--audit PATH`. The audit records the time, the
  source name, counts, line numbers, labels and detector ids, never values, lengths, offsets or hashes.

**Screen capture (best effort).** By default the window asks Windows to exclude it from screenshots and
screen sharing. This works with the capture methods Windows controls, and some tools or drivers can
bypass it. Treat it as a seatbelt, not a wall. It can be switched off in Settings or in the tray.

**Clipboard history (best effort).** After the hotkey cleans the clipboard in place, the app asks Windows
to delete the raw item from the Win+V history. If history is off, the call fails, or the cloud clipboard has
already synced the raw copy, that copy may survive. Copies that a password manager marked as private
(`ExcludeClipboardContentFromMonitorProcessing`) are never read and never rewritten. A copy made while a
scan runs is never overwritten: the app compares the clipboard's sequence number after opening the clipboard
for writing, in the same session that writes, and `scrub clipboard` re-reads and compares the text in the
same helper process that writes it. The `scrub clipboard` command does not touch Win+V history at all.

**Time limits.** Remembered secrets are zeroed, and both text panes are cleared, when you quit, lock
Windows, put the PC to sleep, or leave the app idle past the timeout (10 minutes by default, selectable).
"Idle" means no call from the window to the app's backend: a scan, a copy, a settings change. Reading the
output or the findings without doing any of those does not count as activity, so the panes are also wiped
after the timeout while you are reading. The timer runs only while the app holds text or remembered
secrets.

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
- **Crash dumps.** Nothing in the app turns off Windows Error Reporting or the WebView2 crash reporter.
  If the app or its web view crashes, the dump can contain the text that was in memory (the window's
  text, remembered secrets), and Windows may keep it on disk. Redactor does not write dumps itself and
  cannot delete the ones Windows writes.
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
  - JavaScript strings: the text you paste lives as strings in the WebView2 process, which the garbage
    collector frees when it likes. The wipe replaces the editor contents and drops the history, which is
    as far as a web view allows.
  These copies are freed normally and can stay in memory until it is reused.
- **Remembered secrets and the swap file.** Memory holding a remembered secret can be paged to disk by
  Windows. Zeroing on drop limits the time a value sits in memory, not what the operating system does
  with it meanwhile.
- **Other software on the path.** Redactor cannot see what your browser, terminal or chat client does with
  text before or after. The WebView2 runtime is a Microsoft component with its own update mechanism; the
  installer downloads it from Microsoft when a PC does not have it. The
  app's content security policy keeps the app's own page from using the network, but does not govern the
  runtime's updates.
- **Metadata.** Placeholders keep structure: labels, line breaks and which values are equal (`#1` twice).
  That is deliberate, so the output still reads well, but it reveals that two places share a value.
- **The always-redact list.** It is plain text on disk. Do not put passwords in it. Use "Remember" for those.
- **The CLI's `--diff` output** shows the original lines, secrets included. Do not share it.

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
