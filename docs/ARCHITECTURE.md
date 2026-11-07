# Architecture

Redactor is one detection engine and two front ends. The engine is a pure library. The desktop app and the
`scrub` command-line tool are thin shells around it, so both give the same answer for the same text.

```
            +----------------------------- Redactor.exe (Tauri 2) -----------------------------+
 WebView2   |  app/ui (Svelte 5)  --- IPC: 14 whitelisted commands, isolation, strict CSP -->   |
 (private,  |                                                  app/src-tauri (Rust)            |
 no network)|   shows text, highlights spans,                   +- session secrets (memory)    |
            |   never receives secret values                    +- settings.json (settings)    |
            |                                                   +- win/ clipboard, history,    |
            |                                                      session lock, WebView2 flags|
            +--------------------------------------------------------------------------------+
                                  |                                         |
                        crates/redactor-wire  <-------------- crates/scrub (CLI)
                                  |
                        crates/redactor-core  (the engine: no I/O, no unsafe)
```

## Repository layout

| Path | Role |
|---|---|
| `crates/redactor-core` | The engine. Text in, redacted text and value-free findings out. `#![forbid(unsafe_code)]`. |
| `crates/redactor-wire` | The JSON shape shared by the app and `scrub --json`: requests, responses, the UTF-16 offset mapping; and the text file decoder (UTF-8, UTF-16, BOMs) both use. No Tauri. |
| `crates/scrub` | The CLI: argument parsing, `redactor.toml`, file walking, git hook, audit log, clipboard helper. |
| `app/src-tauri` | The Windows shell, crate `redactor-app`: commands, session secrets, settings, wipe rules, hotkey, tray, Win32 code. |
| `app/ui` | The Svelte 5 + TypeScript front end, with CodeMirror 6 for both text panes. Fonts are bundled. |
| `app/isolation` | The page of Tauri's isolation pattern: an allowlist of the UI's IPC calls, with argument checks. |
| `corpus/` | Labeled test text (templates, with generated fake tokens) behind the accuracy gates. |
| `tools/` | Classifier training, release scripts, the Windows build loop, UI fixture generation, doc generation. |
| `docs/` | This documentation. |

## The engine (`redactor-core`)

`Engine::new(EngineConfig)` builds the engine once (compiles rules, sets up the matchers). It is immutable
and `Send + Sync`, so one engine serves many threads. `Engine::scan(text, &ScanRequest)` runs the pipeline
and returns a `ScanOutcome`: the redacted `output`, the `findings`, the `output_spans` (where the
placeholders sit in the output), `warnings` and a `verified` flag.

A `Finding` carries offsets, a kind, a label, a tier, a confidence, the detector id, a human reason, the
source and an opaque `ValueKey`. It never carries the secret value. A `ValueKey` is a keyed BLAKE3 hash of
the value under a random per-process key. It lets the UI say "keep this value" without ever holding it.

### The scan pipeline

| Step | Module | What happens |
|---|---|---|
| 1. Shadow text | `normalize.rs` | Remove invisible characters (zero-width, soft hyphen, variation selectors, fillers and similar), bidirectional controls and tag characters; apply NFKC so fullwidth forms fold. A byte map leads back to the original so every span grows to cover removed characters. Each class is counted for a warning. |
| 2. Detectors | `rules/`, `context/`, `classifier/`, `terms.rs` | All of these run on the shadow text and emit candidates `{range, kind, label, confidence, detector, reason, source}`. See below. |
| 3. Decode layer | `decode.rs` | Base64, percent-encoded, JSON-escaped and long hex runs that decode to mostly printable UTF-8 are decoded and scanned again, to a depth of two and within a byte budget. A hit inside covers the whole encoded run. |
| 4. Guards | `guard.rs` | Drop values that are templates or filler: `changeme`, `${VAR}`, `{{ x }}`, `process.env.X`, `xxxx`, already-redacted text and so on, plus allowlisted and kept values. |
| 5. Threshold | `engine.rs` | Keep candidates at or above the sensitivity threshold (Relaxed, Balanced, Paranoid). |
| 6. Propagate | `propagate.rs`, `occurrences.rs`, `encoded.rs` | Every redacted value of at least six characters is searched for elsewhere in the text, in linear time: an index over each value's first six bytes, no automaton over all values. Further occurrences become findings with the same label. Base64 (either alphabet), hex, percent-encoded and JSON-escaped runs are decoded into one buffer and searched too, up to two encodings deep, so an encoded copy is redacted as the value itself. |
| 7. Resolve | `resolve.rs` | Merge overlapping spans into their union. Nothing is dropped, because a dropped overlap would leak a prefix. The label comes from the best evidence among the members: a vendor pattern before the context engine before the classifier, then confidence. |
| 8. Redact | `redact.rs`, `placeholder.rs` | Give every occurrence of one value the label of its best-evidenced occurrence, so one value has one placeholder (`one_label_per_value` in `engine.rs`), and splice placeholders. The same value gets the same number, numbered per label in order of first appearance; the ` #N` appears only when a label has more than one distinct value. |
| 9. Verify | `engine.rs` | Scan the output again. Anything new outside a placeholder is redacted and the loop repeats up to a cap; if the cap is hit the outcome says `verified = false`. |

Scanning an already-redacted output gives it back unchanged with no findings.

### The detectors

- **Pattern catalog** (`rules/`): provider tokens, key material, password hashes, HTTP headers, URLs with
  credentials, connection strings, and personal data (only with `pii`). Almost every rule has a literal
  anchor such as `ghp_` or a header name. All anchors of all rules go into two Aho-Corasick automata, so
  the text is read once, and a rule's regex runs only in a small window around an anchor hit. Where a
  format has a checksum (GitHub tokens, credit cards, IBANs, BIP-39 phrases, Bitcoin WIF keys) it is
  checked in code. The `regex` crate has no lookaround, so word boundaries are also checked in code.
- **Context engine** (`context/`): finds secrets with no recognizable shape of their own by the name next
  to them. Key names are split into words (`dbPassword` becomes `db`, `password`) and matched as whole
  words, so `bypass` and `passport` never fire. It parses many syntaxes (`KEY=v`, JSON, YAML, XML,
  command-line flags, SQL, netrc) and prose in several languages, and judges whether the value is a
  literal or code.
- **Classifier** (`classifier/`): a logistic model over character-class, entropy and letter-trigram
  features, for random-looking tokens that have no key name. The weights and the trigram table are
  generated by `tools/classifier/train.py` and committed; no text is shipped with them.
- **Always-redact terms** (`terms.rs`): whole-word, case-insensitive matches for words the user lists.
- **Matcher** (`SecretMatcher`): a hook for known values. The app plugs its session secrets in here.
- **Custom rules** (`rules/custom.rs`): user regexes from the config file.

Every rule belongs to a group and has an id. [DETECTORS.md](DETECTORS.md) is generated from the catalog by
`scrub detectors --markdown`.

### Offsets

The engine works in **UTF-8 byte offsets** into the input, because that is what Rust slices with.
JavaScript strings, and therefore CodeMirror, count **UTF-16 code units**. Every offset that crosses the
boundary goes through `Utf16Map` (`redactor-core/src/utf16.rs`) inside `redactor-wire`: findings and output
spans are converted going out, and the manual ranges the UI sends are converted coming in. Emoji, CRLF line
ends and other astral characters before a secret therefore cannot shift a highlight. The same mapping
serves `scrub --json`, so the UI's test fixtures are exactly what the app sends.

## The app shell (`app/src-tauri`)

The platform-neutral modules (settings, session secrets, the engine bridge, hotkey rules, wipe rules, file
decoding, clipboard logic over a trait) build and test on every platform. Only the Tauri shell and the
Win32 code are Windows-only, so Linux CI needs no WebKit.

| Module | Role |
|---|---|
| `app.rs` | Builds the Tauri app: plugins, the main window, hotkey registration, idle timer, lock watcher, close-to-tray. |
| `commands.rs` | The 14 IPC commands. Heavy work runs on a blocking worker, never on the UI thread. |
| `bridge.rs` | Re-exports `redactor-wire`'s scan mapping. |
| `engines.rs` | Keeps one engine per `stripInvisible` value for the current always-redact words and placeholder style, rebuilds in the background when they change, and shares one per-process session key. |
| `session.rs` | Session secrets: boxed byte values, zeroed on drop, searched with `memchr::memmem` under a lock. There is no prebuilt automaton, whose internal copies of the values could not be wiped. |
| `settings.rs` | `%APPDATA%\Redactor\settings.json`: validated on the way in, repaired on the way out, written atomically. |
| `wipe.rs` | Tracks whether there is anything worth wiping and when the app was last used. |
| `clipscrub.rs` | Clipboard scrubbing over a `Clipboard` trait, so the rules are tested without a desktop. |
| `hotkey.rs` | Parses and validates the global hotkey. |
| `tray.rs` | The tray icon and menu. |
| `textfile.rs` | Decodes opened files with `redactor-wire`'s decoder: UTF-8 or UTF-16, binary refused, 20 MiB cap. |
| `browser_args.rs` | The WebView2 browser switches: wry's defaults, no non-proxied WebRTC UDP, no DNS prefetch, no background networking. |
| `win/` | The only place `unsafe` is allowed: the Win32 clipboard, WinRT clipboard history, session lock and sleep notifications, WebView2 hardening. Each block has a `// SAFETY:` comment. |

The window is built in code, not from `tauri.conf.json`: frameless with a custom title bar, WebView2 private
mode, browser extensions off, navigation limited to the app's own origin, new windows denied, developer
tools only in debug builds. `win/webview.rs` switches off autofill, password autosave, browser accelerator
keys, the default context menu and swipe navigation.

## The UI (`app/ui`)

Svelte 5 with runes, TypeScript, Vite and CodeMirror 6. All talk to the shell goes through one module,
`src/lib/api.ts`, which validates every response against a schema with an exact key set (a stray `value`
field in a finding would fail loudly). State lives in small rune stores (`workspace`, `session`,
`settings`, `remembered`). The editor panes, the findings rail, sheets and dialogs are components under
`src/components`. CodeMirror's styles are mounted as constructed stylesheets so that the content security
policy needs no `'unsafe-inline'`.

A mock backend (`src/lib/mock`, used by `npm run dev` and the Playwright tests) answers the same commands
from fixtures that `tools/gen-ui-fixtures.sh` generates by running the real engine through `scrub --json`.
The production bundle contains none of it; a test checks that.

## The IPC contract

JSON is camelCase and offsets are UTF-16 code units. The UI may call exactly these commands, which
`capabilities/main.json` grants and `tests/config.rs` checks:

| Command | Purpose |
|---|---|
| `scan(req)` | Scan text: sensitivity, PII, strip-invisible, kept value keys, manual ranges. Input cap 20 MiB. |
| `scrub_clipboard()` | Scrub the clipboard in place. The global hotkey calls the same code. |
| `read_clipboard()` | The clipboard text, or nothing when it is empty or marked private. |
| `copy_text(text)` | Write safe text to the clipboard. |
| `open_text_file()`, `save_text_file(text, name)` | The file dialogs, run in the shell. |
| `session_list/remember/add/forget/clear` | Session secrets. The UI sees `{ id, label, addedAt }`, never the value or its length. |
| `get_settings()`, `set_settings(s)` | Settings, validated in the shell. |
| `app_info()` | Versions and rule count. |

The shell sends four events: `clipboard-scrubbed`, `wipe` (reason: idle, lock, sleep or tray),
`session-changed` and `settings-changed`. Besides the commands, the window may only use minimize, maximize,
close and drag for the custom title bar, and listen to events. Every IPC message also passes through the
isolation page, which lets through only these calls with the argument shapes the UI sends
(`app/ui/tests/unit/isolation.test.ts` replays every UI call in Tauri's IPC shape). Errors are short
strings that never quote text.

## Security boundaries

- The UI never receives a secret value from Rust, and Rust never logs or stores scanned text.
- The window can reach only the app's own backend (CSP `connect-src ipc: http://ipc.localhost`).
- `unsafe` is forbidden in the engine, the wire crate and the CLI, and confined to `app/src-tauri/src/win/`.
- Dependencies are pinned (`=` for Tauri and its plugins, exact versions in `package.json`), lockfiles are
  committed, and CI runs `cargo deny` for licenses and advisories.

The full analysis is in [THREAT_MODEL.md](THREAT_MODEL.md).

## Build and release

- **CI** (`.github/workflows/ci.yml`): format, clippy with warnings denied, the Rust tests (Linux and
  Windows, including the corpus gates), `cargo deny`, UI typecheck, lint, unit tests and the Playwright
  suite against the mock backend. Workflows run with read-only permissions and actions are pinned by commit.
- **Release** (`.github/workflows/release.yml`): a `v*` tag on `main` builds everything on GitHub's runners
  and nowhere else, with the runner's checkout path remapped out of the binaries. It produces the NSIS
  installer, the portable zip, `scrub` for four targets, `SHA256SUMS`, and build provenance attestations,
  then opens a draft release whose notes come from `CHANGELOG.md`. Publishing the draft is manual.
- **Windows build loop** (`tools/win-build.sh`): for local development from WSL, copies the repo to a
  Windows folder and runs the Windows toolchain there.
