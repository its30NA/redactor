# Architecture

Redactor is one detection engine and two front ends. The engine is a pure library. The desktop app and the
`scrub` command-line tool are thin shells around it, so both give the same answer for the same text.

```
            +----------------------------- Redactor.exe (Tauri 2) -----------------------------+
 WebView2   |  app/ui (Svelte 5)  --- IPC: 14 whitelisted commands, isolation, strict CSP -->   |
 (private,  |                                                  app/src-tauri (Rust)            |
 no network)|   shows text, highlights spans,                   +- session secrets (memory)    |
            |   never receives secret values                    +- settings.json (settings)    |
            |                                                   +- platform/windows/ clipboard,|
            |                                                     history, lock, WebView2 flags|
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
| 1. Shadow text | `normalize.rs` | Remove invisible characters (every Unicode default-ignorable character, such as zero-width characters, the soft hyphen, variation selectors and fillers, plus the interlinear annotation controls), bidirectional controls and tag characters; U+FE0F is kept only after an emoji or before a keycap mark; apply NFKC so fullwidth forms fold. A byte map leads back to the original so every span grows to cover removed characters. Each class is counted for a warning. |
| 2. Detectors | `rules/`, `context/`, `classifier/`, `terms.rs` | All of these run on the shadow text and emit candidates `{range, kind, label, confidence, detector, reason, source}`. See below. |
| 3. Decode layer | `decode.rs` | Base64, percent-encoded, JSON-escaped and long hex runs that decode to mostly printable UTF-8 are decoded and scanned again, to a depth of two and within a byte budget. A hit inside covers the whole encoded run. Base64 wrapped across lines (same width, same indent, LF or CRLF) is decoded as one body. A base64 run glued to a URL path (`/cb/<base64>`) is tried at four alignments in both alphabets. An escape JSON does not know (`\x`, `\q`) is copied through and also read as a boundary, so later `\u` escapes are still decoded. |
| 4. Guards | `guard.rs` | Drop values that are templates or filler: `changeme`, `${VAR}`, `{{ x }}`, `process.env.X`, `xxxx`, already-redacted text and so on, plus allowlisted and kept values. The variable, template, filler and dotted-config-path shortcuts apply only when no digit is mixed into the letters, so `$ummer2024x` and `config.Zq81mXv0pLr3` are still redacted; all-caps names with trailing digits (`API_KEY_V2`) still count as variables. "Already redacted" means a placeholder whose label the engine emits, so text that only looks like one (`[REDACTED-…`) is redacted. |
| 5. Threshold | `engine.rs` | Keep candidates at or above the sensitivity threshold (Relaxed, Balanced, Paranoid). |
| 6. Propagate | `propagate.rs`, `occurrences.rs`, `encoded.rs` | Every redacted value of at least six characters is searched for elsewhere in the text, in linear time and memory, also when values nest or overlap: each position reports the longest value plus a few nested shorter ones, and past a work budget an Aho-Corasick pass takes the longest match per position. Further occurrences become findings with the same label. A short value (6 to 11 characters) is found only where it stands alone; values of 12 or more characters are found anywhere. Always-redact terms and the values of the matcher are searched in their encoded copies too, and terms seed the encoded-copy search even when the text has no plain copy. Base64 (either alphabet), hex, percent-encoded and JSON-escaped runs are decoded into one buffer and searched too, up to two encodings deep, so an encoded copy is redacted as the value itself. |
| 7. Resolve | `resolve.rs` | Merge overlapping spans into their union. Nothing is dropped, because a dropped overlap would leak a prefix. The label comes from the best evidence among the members: a vendor pattern before the context engine before the classifier, then confidence. |
| 8. Redact | `redact.rs`, `placeholder.rs` | Give every occurrence of one value the label of its best-evidenced occurrence, so one value has one placeholder (`one_label_per_value` in `engine.rs`), and splice placeholders. The same value gets the same number, numbered per label in order of first appearance; the ` #N` appears only when a label has more than one distinct value. |
| 9. Verify | `engine.rs` | Scan the output again. Anything new outside a placeholder is redacted and the loop repeats up to a cap. At the cap the last round is still applied, and the outcome says `verified = false` with a warning. |

Scanning an already-redacted output gives it back unchanged with no findings.

`drop_identifiers` (`context/mod.rs`) is linear in the size of the input. It measured 0.27, 1.09 and 2.54 s for 1, 4 and 8 MiB of `password = <identifier>` lines, and is left as it is.

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
decoding, clipboard logic over a trait) need no desktop. The shell (`app.rs`, `commands.rs`, `state.rs`,
`tray.rs`) never names an operating system: it reaches the OS only through one `Platform` value built at
start-up (`platform::native()`), and `tests/platform_boundary.rs` keeps it that way. The shell compiles and
is tested on Windows, macOS and Linux CI (Linux needs the WebKitGTK packages). The app starts on Windows, on
macOS 13 and later, and on Linux X11 and Wayland sessions.

| Module | Role |
|---|---|
| `app.rs` | Builds the Tauri app: plugins, the main window, hotkey registration, idle timer, lock watcher, close-to-tray. |
| `commands.rs` | The 14 IPC commands. Heavy work runs on a blocking worker, never on the UI thread. |
| `bridge.rs` | Re-exports `redactor-wire`'s scan mapping. |
| `engines.rs` | Keeps one engine per `stripInvisible` value for the current always-redact words and placeholder style, rebuilds in the background when they change, and shares one per-process session key. |
| `session.rs` | Session secrets: boxed byte values, zeroed on drop, searched with `memchr::memmem` under a lock. There is no prebuilt automaton, whose internal copies of the values could not be wiped. |
| `os.rs` | `Os` (`Windows`, `Mac`, `Linux`): a plain value the OS-dependent rules take (hotkey names and lists, the settings folder, the UI's shortcuts), so every OS's rules are tested on any OS. The platform layer says which one is running. |
| `settings.rs` | `settings.json` in the OS's config folder: `%APPDATA%\Redactor` on Windows, `~/Library/Application Support/Redactor` on macOS, `$XDG_CONFIG_HOME/redactor` (or `~/.config/redactor`) on Linux; an unset, empty or relative variable means defaults, never a file in the working folder. Validated on the way in, repaired on the way out, written atomically. On macOS and Linux the app's own folder is 0700 and the file 0600; an existing folder or file is tightened at the next save, a shared parent such as `~/.config` never. A file that is too large or is not settings JSON is moved to `settings.json.bad` beside it (0600, replacing an older one) and the app starts on defaults; the window says so (`settingsUnreadable`). Settings that cannot be saved still apply until the app closes; the first failure in a run says so once, as a desktop notification and in the window (`settingsNotSaved`). |
| `wipe.rs` | Tracks whether there is anything worth wiping and when the app was last used. Idle time counts on two clocks, the longer of the monotonic and the wall clock, so time asleep counts as idle (the monotonic clock stands still in a suspend on Linux and macOS). A wall clock set back never delays the wipe. The shell checks every 15 s. |
| `clipscrub.rs` | Clipboard scrubbing over a `Clipboard` trait, so the rules are tested without a desktop. It writes back whenever the scan changed the text: secrets removed, or (with strip invisible on, the default) only zero-width, bidi or tag characters removed (status `stripped`). A scan that could not be verified and removed nothing leaves the clipboard as it was. The words of every result are one table, `ClipboardScrub::words()`, which the UI's toasts share (`tests/fixtures/scrub_notices.json`). |
| `hotkey.rs` | Parses and validates the global hotkey for an `Os`: `Cmd` on macOS, `Super` on Linux, the Windows key unreadable on Windows. The rules and their messages are pinned by one case table (`tests/fixtures/hotkey_cases.json`) that the Rust tests and the UI's tests both read. |
| `tray.rs` | The tray icon and menu. |
| `textfile.rs` | Decodes opened files with `redactor-wire`'s decoder: UTF-8 or UTF-16, binary refused, 20 MiB cap. Only regular files are read, and the read stops at the cap, so a folder, a pipe, a device or a file that grows while it is read is refused ("The file could not be read.", "The file is larger than 20 MiB."). |
| `browser_args.rs` | The WebView2 browser switches: wry's defaults, no non-proxied WebRTC UDP, no DNS prefetch, no background networking. |
| `platform/` | The platform layer: traits `SystemClipboard` (on top of `clipscrub::Clipboard`, which also covers clipboard history), `LockWatcher`, `WebviewHardening` and `GlobalHotkey`, a `tray` flag, a `shield` flag, and `native()`. Four small hooks let the shell treat each OS its own way without naming it: `WebviewHardening::prepare` (the last step before the main window is built, for settings an OS only takes before the web view exists), `reopen` (this run-loop event is the OS asking for the window back), `tray_click_opens_menu` (a left click on the tray icon opens the menu, or the window) and `tray_notice` (the sentence shown once when closing hides the window). `hotkey_plugin.rs` implements the hotkey with Tauri's global-shortcut plugin. |
| `platform/windows/` | The Windows implementation: the Win32 clipboard, WinRT clipboard history, session lock and sleep notifications, WebView2 hardening. One of the places `unsafe` is allowed (with `platform/macos/` and one call in `platform/linux/tray.rs`); each block has a `// SAFETY:` comment. |
| `platform/macos/` | macOS. `clipboard.rs` is the general pasteboard (below). `lock.rs` wipes on `com.apple.screenIsLocked` (the only direct lock signal, undocumented, observed with "deliver immediately" so a background app still gets it), on displays going to sleep, on the Mac going to sleep and on the session being switched away. `webview.rs` builds the web view from our own configuration (below). `mod.rs` keeps the system's Dictation and Emoji items out of the Edit menu, answers the Dock click and a second launch with the window (`reopen`), and opens the menu on a click of the menu-bar icon. There is no screen-capture shield: since macOS 15.4 screen capture ignores the window flag it would set. The single-instance plugin's socket is `/tmp/io_github_its30na_redactor_si.sock`, in a folder all users share: when something at that path belongs to another user (or cannot be checked), the plugin is not registered, the app says so once on stderr and starts without the check, so another user cannot stop it from starting. |
| `platform/linux/` | Linux. `session.rs` reads the session from `DISPLAY` and `WAYLAND_DISPLAY` (`WAYLAND_DISPLAY` wins). `x11/` is the app's own X11 clipboard client (below). `wayland/` holds the two Wayland clipboards, `portal.rs` the hotkey through the desktop's shortcuts portal and `command_hotkey.rs` the hotkey where there is no portal (below). `lock.rs` wipes on lock and sleep: logind's `PrepareForSleep` and `Lock`, the session's `LockedHint` (GNOME and KDE lock without the `Lock` signal), and `ActiveChanged` of five screen-saver interfaces (`org.freedesktop`, `org.gnome`, `org.cinnamon`, `org.mate` and `org.xfce` `.ScreenSaver`); a missing bus is not an error. A watched bus that goes away (a system or session bus restart) is subscribed to again after 1 s, with the pause doubling up to 30 s while it stays away and resetting after a working subscription; a bus missing at start is given up on. Each bus reports once whether it found a lock source (this process's logind session, or a screen-saver name with an owner); `app_info` waits up to 1 s for that and sends `lockWatched`, and where neither bus has one Settings notes that a screen lock does not wipe here. A locker that sends no signal (`swaylock` or `i3lock` started directly, WSLg) goes unseen; suspend and the idle timer still wipe. `tray.rs` says whether the desktop shows a tray (a StatusNotifier host is registered and the indicator library loads; the one `unsafe` call on Linux), because the tray library aborts where there is none; without a tray, closing the window quits. The probe has 2 s in all, connecting included (`tray::PROBE`), at every start, also each `--scrub-clipboard` second launch: a watcher that answers late means no tray for that run, never a stalled start. `webview.rs` hardens WebKitGTK: no WebRTC, media stream, page cache, WebGL, WebAudio, swipe navigation or default context menu. GDK is pinned to the session's backend (X11 or Wayland), so the window lives on the same server as the clipboard and the hotkey. The screen-capture shield does not exist on Linux (`Platform.shield` is false; the setting is forced off and the UI says why). |
| `platform/unsupported.rs` | The placeholder for an OS that is none of the three: `native()` returns the reason and the binary points to the `scrub` CLI. No CI job builds it. |

Every clipboard implementation passes one contract, `tests/support/clipboard_contract.rs` (text round trip, a copy marked private is left alone, an empty clipboard or an image reads as empty, oversized text never reaches the window, a stale sequence blocks the write, scrub in place rewrites only when there are findings). An in-memory fake runs it on every OS (`tests/fake_clipboard_contract.rs`); each real clipboard runs it on its own desktop (`tests/win_clipboard.rs` on Windows, with the clipboard history check on top; `tests/x11_clipboard.rs` on a throwaway X server, against `xclip` and against helper processes that hint, lie about their size, stay silent or exit half way; `tests/macos_clipboard.rs` on GitHub's macOS runners, against `pbcopy`/`pbpaste` and a second process that copies during our reads and writes).

The text a clipboard hands over is judged whole. Windows reads text up to the first NUL (`CF_UNICODETEXT`).
On Linux (X11, Wayland) and macOS a paste gets every byte, so trailing NULs are dropped and every other NUL
is shown and scanned as U+FFFD (`clipscrub::text_from_utf8`, `own_text`): no text hides behind a NUL, and a
rewrite holds none. On macOS a copy that lists text but hands over none of it (a refused paste, a lazy
provider that gives nothing) is an error, "Clipboard not scrubbed", not an empty clipboard.

### Start-up answers

`main` first sets the core-file limit to zero on Linux and macOS (`platform::no_core_dumps`), so a crash
writes no core file and systemd-coredump stores none; processes the app starts inherit the limit. It is no
guarantee against every crash reporter, and the process is not marked non-dumpable (`PR_SET_DUMPABLE`), which
may stop the GNOME and KDE portals from recognising the app; that waits for a hand test there.

`platform::native()` returns the `Platform` or the reason there is none, and `main` prints that reason and
exits with code 2. There is one sentence per case, each pointing to the `scrub` CLI: no display at all, an X
server that cannot be reached or has no XFIXES extension, and a Wayland compositor that cannot be reached.

### The X11 clipboard (`platform/linux/x11/`)

On X11 there is no clipboard store: the text stays in the application that copied it, and whoever pastes
asks that application for it. Redactor speaks this protocol itself (`x11rb`), on its own connection, because
the rules of the contract above need control a plugin does not give.

- **Two kinds of threads, one connection.** A worker thread is the only one that waits for events. It counts
  every change of the `CLIPBOARD` owner (XFIXES reports a new owner, an owner whose window is destroyed and an
  owner that exits), answers applications that paste our text (`serve.rs`), and hands the answers of the one
  clipboard operation in progress to the thread that asked for it.
- **The sequence number is ours.** X11 has none, so it is the count of owner changes. Reading it sends a
  "ping" first (an empty write to a property of our own window). The server reports events in the order
  things happened, so when the ping comes back every earlier owner change has been counted.
- **Compare-and-set under a server grab.** `write_if_unchanged` grabs the server, pings, compares the count
  with the caller's number and only then takes the selection. While the grab lasts the server handles no
  other client, so no copy can land between the comparison and the write. Two tests put another
  application's copy on its way before and during the write and require that copy to survive.
- **The other side is not trusted** (`read.rs`). A copy whose `x-kde-passwordManagerHint` says `secret`, or
  cannot be read, is left alone. The size of the text is asked for before a byte is copied; text over 20 MiB
  is refused, also when the owner announced less and sends more. Every answer has one second, a whole read
  ten. A transfer in pieces that turns out too large is not abandoned half way (an owner that serves one
  transfer at a time would answer nobody afterwards): its pieces are deleted on the server, unread.
- **Serving.** `TARGETS`, `MULTIPLE` (how clipboard managers save), `TIMESTAMP` and the two UTF-8 text
  targets. Text up to 256 KiB is answered in one property, larger text in pieces from a snapshot taken at
  request time; at most 8 such transfers run at once, and one whose requestor goes away or stops asking is
  dropped. When another application copies, our text is dropped.
- **On exit** the clipboard would be empty. If the desktop runs a clipboard manager, it is asked to take the
  text over (`SAVE_TARGETS`) and gets one second to do so.
- X11 has no clipboard history to clean; a clipboard manager's own history is out of reach.

### The macOS clipboard (`platform/macos/clipboard.rs`)

macOS keeps copied text in a pasteboard server, so unlike X11 nothing has to be served. The pasteboard has
no "open" session and no compare-and-set. Its change count moves by one each time an app takes it over, so
the count is the sequence number. Every call fetches the pasteboard itself, may come from any thread, and
holds one process-wide lock.

- **Reading.** A copy marked `org.nspasteboard.ConcealedType` (password managers) or
  `org.nspasteboard.TransientType` (an app that will put the old content back in a moment) is left alone.
  Text is read as UTF-8, or as UTF-16 when that is all the copy offers; more than 20 MiB is refused before it
  is decoded. A copy that lists a text type but gives no data for it is an error, not an empty clipboard.
  The count is read before and after: if it moved, a newer copy (maybe a password manager's) landed while
  the types were checked, and the read is thrown away and started again.
- **The guarded write.** `write_if_unchanged` compares the count with the caller's number, takes the
  pasteboard over, and requires the count that step returns to be exactly one higher; after the text is
  written the count must still be that. If any check fails, another app copied in between: the write
  reports an error, and if AppKit put our text on top of that newer copy anyway, our text is cleared away
  again. The other copy is then lost (the clipboard is empty), never replaced by ours after an error.
- **What the checks cannot see.** macOS hands a copy to the pasteboard server a moment after the call
  returns, in every app. So a copy another app makes within about a millisecond of ours can still end up
  under our text while every check passed: the clipboard then holds the safe text of the earlier copy, and
  the newer copy is gone. It is never a secret that ends up on top. CI forces this race 2000 times per
  runner; the rate measured is at most 9 in 2000, and the test fails above 2 % or on any case after an error.
- macOS has no clipboard history an app can reach (`historyCleaned` is always false), and quitting leaves
  the text in the pasteboard, so there is nothing to hand over on exit.

### The Wayland platform (`platform/linux/`)

A session with `WAYLAND_DISPLAY` set is a Wayland session, whether or not `DISPLAY` is also set. The window is
a **native Wayland window**, not an XWayland one: `gdk::set_allowed_backends("wayland")` runs before Tauri
opens the display, so the window lives on the compositor the clipboard talks to. `wayland_platform()` picks
two things, and each can fall back on its own:

| Part | Chosen when |
|---|---|
| Clipboard | `DataControlClipboard` when the compositor offers a data-control protocol (and connecting to it works), else `FocusedClipboard`. |
| Hotkey | `PortalHotkey` when the session bus has the GlobalShortcuts portal (version 1 or later), else `CommandHotkey`. Asked at the hotkey's first use, within 5 s, so a second start never waits for the session bus. |
| Lock, tray, webview | As on X11. The shield does not exist (`shield: false`). |

If the compositor cannot be reached at all, start-up fails with a sentence pointing to the `scrub` CLI.
`app_info` tells the UI which hotkey mode applies and whether the clipboard needs focus (`hotkey`: `keys`,
`portal` or `command`; `clipboardNeedsFocus`), and Settings words the difference (texts pinned by the UI's unit
tests).

#### The data-control clipboard (`wayland/datacontrol.rs`)

Wayland has no clipboard store either: text stays in the app that copied it. Normal apps may read the
clipboard only while focused. Compositors that offer a data-control protocol let a background client do it,
and Redactor uses that one.

- **Two protocols, one trait.** `ext_data_control_manager_v1` when offered (Plasma 6), else
  `zwlr_data_control_manager_v1` (sway, wlroots, KWin 5.27). One internal trait covers both, so the read and
  write code exists once.
- **One worker thread owns the connection.** Only it waits for events. It counts every `selection` event (the
  sequence number is ours, as on X11), keeps the current offer and its MIME types, and answers applications
  that paste our text. Other threads send it a request and wait for the answer.
- **The other side is not trusted.** A copy that carries the password-manager hint (`x-kde-passwordManagerHint`
  = `secret`), or has no text type, is left alone. The read stops at 20 MiB (counted as bytes received, not as
  announced) and at its deadlines: 1 s between two pieces, 10 s for the whole read, 1 s for the hint. An owner
  that never writes ends the read with an error; one that closes the pipe half way gives the text so far. Our
  own text is served to at most 8 pastes at once, each with 10 s, so pastes that nobody reads cannot pile up
  threads. Received bytes are held in zeroized buffers.
- **Write is compare-and-set without a lock.** `write_if_unchanged` asks the compositor for a `sync` round
  trip, compares the count at its answer with the caller's number, and sets the selection right there on the
  worker. Wayland has no server grab, so a copy that reaches the compositor in the moment between the answer
  and our `set_selection` loses to our write. This is a known limit; `write_race_is_bounded` tries it 2000
  times, prints an `OBSERVED` line and fails above 10 % (local sway runs: up to 2.25 %; on CI's sway and
  KWin the race was not reached).
- **On exit** the selection we set is dropped with our connection. No protocol hands it to a clipboard
  manager. A desktop with one may already hold a copy; sway has none, and the text is gone.
  `quit_drops_our_selection_on_sway` records what happens. <!-- PENDING hand test: whether WSLg keeps text
  Redactor put on the clipboard after it quits (hand test H); WSLg uses the focused-window clipboard below,
  which has no `keep_after_exit` either. -->

#### The focused-window clipboard (`wayland/focused.rs`)

For compositors without data-control (GNOME, Weston, WSLg). GTK, on the main thread, owns the clipboard
access, because a Wayland client can only read the selection while its window has the keyboard focus.

- **Every read and every write refuses while the window is not active.** This is the point of the design: a
  copy made elsewhere after the window lost focus must never be overwritten by a scrub of an older offer.
  Each operation first catches up with the compositor (a round trip, then every pending GDK event), then
  checks the window, in one main-thread step. Pinned by `inactive_window_refuses_read_and_write`.
- The sequence number is the count of GTK `owner-change` signals.
- Text moves as bytes, as the sender offered it, so CRLF stays CRLF.
- GTK keeps its own copy of text it read, which Redactor cannot zeroize, and it reads an oversized copy fully
  before the 20 MiB check can refuse it. Both are known limits. GTK's waits have no deadline of their own: a
  sender that never answers keeps one open until the next copy ends it, and until then every operation fails
  at once.
- **The pending scrub** (`pending.rs`). A press (hotkey, `--scrub-clipboard`, the tray, the palette or
  **Scrub now**) while the window is not focused shows the window, asks for focus, sends the notification
  "Bring Redactor to the front to scrub the clipboard", tells the window (`clipboard-scrub-waiting`) and
  arms a *pending scrub*. It runs on the next focus-in within 30 s, and only once. A second press re-arms
  the same one, not a second scrub; a press while the window is focused scrubs at once and clears any
  pending one. If focus does not come (the desktop may refuse to move a window forward), clicking the
  window starts it. The 30 s count on two clocks (`When`, the longer of the monotonic and the wall clock), so
  a press from before a suspend is old after it. After 30 s the pending scrub expires, and the desktop
  notification "Not scrubbed. Redactor's window did not come to the front in time." is sent; the window
  shows "Not scrubbed" with a **Scrub now** button, at expiry and again at the next focus within 10 min
  (`UNSEEN_FOR`, once). Pinned by `pending_scrub_runs_once_on_focus`, `pending_scrub_lapses_after_30_s`
  and `second_press_rearms_not_doubles`. <!-- PENDING hand test: WSLg, window minimised (hand test D/D2).
  WSLg has no notification service, so an expired scrub shows only in the window, once it is back. Two
  minimised runs on an older build left the clipboard raw; the cause is probably the 30 s wait but is not
  proven. -->
  <!-- PENDING hand test: click race (hand test G): who wins when the user clicks another window right after
  the command. The code never runs a scrub on a stale clipboard or twice for one press. -->
- **`REDACTOR_WAYLAND_FOCUSED=1`** makes `wayland_platform()` choose this clipboard even where data-control
  exists. It can only take a capability away, so it is safe in shipped code. CI needs it: the focused path is
  smoked on sway with it, because headless Weston gives no window focus.
- On WSLg (Weston) the window counts as focused while a Windows app is in front, so a press scrubs at once
  and never arms a pending scrub; the window does not come forward and no desktop notification shows, only
  the window's own toast (hand test C).

#### The hotkey on Wayland

An app cannot grab a key by itself on Wayland, so there are two ways:

- **`portal.rs` (`PortalHotkey`).** Speaks the `org.freedesktop.portal.GlobalShortcuts` interface (KDE 6,
  GNOME 48 and later) with Redactor's own `zbus` code, on a connection of its own. The Settings key goes as
  `preferred_trigger`, which is a wish: the desktop may ask the user, change it or refuse it. A refused
  bind is `HotkeyTaken`. Changing the key in Settings closes the portal session and binds again. Method
  calls give up after 10 s, a request after 30 s, a bind after 120 s (a person may be reading the dialog).
  The app starts when there is no portal, when the portal lacks the interface, or when the user says no in
  the desktop's dialog; then Settings says what to do and the command path still works
  (`no_portal_means_command_mode`, `refused_bind_is_taken`, `bind_cancelled_mid_wait_times_out`). The bind
  starts on a thread of its own, never on the main thread. The shortcut's description is "Scrub the
  clipboard". CI talks to `tools/ci/mock_portal.py`; the real GNOME and KDE portals are not run in CI (no
  full session), so the real dialog is by hand.
  - *Sender check.* Portal signals count only from the current owner of `org.freedesktop.portal.Desktop`,
    asked with `GetNameOwner` at start and kept current from `NameOwnerChanged`, which counts only from
    `org.freedesktop.DBus`. Another program on the session bus cannot fake a press, a bind or a loss
    (`forged_portal_signals_are_dropped`).
  - *Loss.* When the desktop closes the shortcut session, xdg-desktop-portal restarts, or the app's
    connection to the portal breaks, a bound key is lost. Each loss is told once: the desktop notification
    "The hotkey stopped working: the desktop closed its shortcut. Set it again in Settings.", the window's
    notice "Hotkey stopped working" (`app-status`, `hotkeyLost`), and Settings shows the key as not active.
    The app does not bind again by itself; setting the key again in Settings opens a fresh session. A key
    the app unbound itself, or one never bound, is never reported. Known limit: after the portal's own bus
    connection broke, a new bind cannot succeed until the app restarts.
- **`command_hotkey.rs` (`CommandHotkey`).** No key at all: the user binds the command Settings shows in the
  desktop's keyboard settings. Settings builds it from `app_info.executable` (`cli::this_program`: `$APPIMAGE`
  when it is an absolute path, else the running executable) plus `--scrub-clipboard`, with the path in POSIX
  single quotes when it holds a space or a shell character, and has a Copy button. That command is a second
  launch of the app. The single-instance plugin forwards its arguments to the running copy, which starts a
  scrub as if the hotkey was pressed, and the second process exits with code 0 at once. The flag must be an
  exact argument (`--scrub-clipboard=1` does nothing).
- **Cold start.** With no copy running, the flag starts the app. Where the clipboard works in the background
  (Windows, macOS, X11, Wayland data-control), the scrub runs at once during set-up, before the window is
  up; its result waits for the page's first `app_info` and is shown then (latest wins, once). Only where the
  clipboard needs the window does the press wait for the page's first `app_info` (10 s at most,
  `START_SCRUB_WAIT`), and then wait for focus like any press; if that timer cannot start, it runs at once.

```text
desktop key --> `<program> --scrub-clipboard` --> single-instance --> running copy
                                                                     |-- data-control: scrub now
                                                                     `-- focused: show, focus, pending scrub (30 s)
```

#### Wayland tests

`tests/support/clipboard_contract.rs` runs against both Wayland clipboards (`wayland_clipboard.rs` on sway
and, in the Plasma container, KWin; `wayland_focused.rs` with a real GTK window on sway with the test
switch above), against `wl-clipboard` and helper processes that hint, lie about size, stay silent or close
early. `wayland_portal.rs` runs the portal conversation against the mock. `wayland_webview.rs` checks the
same hardening as on X11 on a native Wayland window. All are ignored by default and run by
`tools/ci/wayland-tests.sh` (see [CONTRIBUTING](../CONTRIBUTING.md#wayland-desktop-tests)); CI counts 42 tests
on sway (19, 12, 10 and 1 per binary; `no_portal_means_command_mode` runs apart, on a bus without a portal,
and is not counted) and 19 on KWin. `tools/ci/wayland-smoke.sh` runs the built app on sway end to end (W1-W8).
The real GNOME and KDE dialogs, and the focused-window path with a real desktop's focus rules, are tested by
hand (a WSLg round on Weston).

The window is built in code, not from `tauri.conf.json`: a custom title bar, private web data,
browser extensions off, navigation limited to the app's own origin, new windows denied, developer tools
only in debug builds. The origin is `tauri://localhost` (and `isolation://` for the isolation frame) on macOS
and Linux, and `http://tauri.localhost` (and `http://isolation.localhost`) on Windows, only there and only
without a port; https is refused everywhere, since `useHttpsScheme` is off. Debug builds also allow the dev
server, `http://localhost:5173` (`allowed_origin` in `app.rs`). The page blocks the right-click menu itself
(`lib/harden.ts`) on every OS.

- **Windows:** frameless, WebView2 private mode. `platform/windows/webview.rs` switches off autofill,
  password autosave, browser accelerator keys, the default context menu and swipe navigation.
- **macOS:** the system's traffic-light buttons over our title bar (the UI leaves room for them and hides
  its own caption buttons). Some settings only exist before a `WKWebView` does, so `platform/macos/webview.rs`
  hands Tauri our own configuration: a data store that keeps nothing on disk, Writing Tools and inline
  predictions off (where the macOS version has them). It also switches off pop-up windows,
  the fraudulent-site lookup, element full screen, back-forward gestures, magnification and link previews.
- **Linux:** see `platform/linux/webview.rs` above.

## The UI (`app/ui`)

Svelte 5 with runes, TypeScript, Vite and CodeMirror 6. All talk to the shell goes through one module,
`src/lib/api.ts`, which validates every response against a schema with an exact key set (a stray `value`
field in a finding would fail loudly). State lives in small rune stores (`workspace`, `session`,
`settings`, `remembered`). The editor panes, the findings rail, sheets and dialogs are components under
`src/components`. CodeMirror's styles are mounted as constructed stylesheets so that the content security
policy needs no `'unsafe-inline'`.

A mock backend (`src/lib/mock`, used by `npm run dev` and the Playwright tests) answers the same commands
from fixtures that `tools/gen-ui-fixtures.sh` generates by running the real engine through `scrub --json`.
Query switches pick what the mock pretends (`?os=linux&hotkey=command&focus=1`, `?clipboard=hidden`, `?scrub`,
`?exe=`, `?hotkeyrefused`, `?settingsbad`, `?savefail`, `?nolock` and more); the full list is at the top of
`src/main.ts`. The production bundle contains none of it; a test checks that.

## The IPC contract

JSON is camelCase and offsets are UTF-16 code units. The UI may call exactly these commands, which
`capabilities/main.json` grants and `tests/config.rs` checks:

| Command | Purpose |
|---|---|
| `scan(req)` | Scan text: sensitivity, PII, strip-invisible, kept value keys, manual ranges. Input cap 20 MiB. |
| `read_clipboard()` | The clipboard text, or nothing when it is empty or marked private. |
| `copy_text(text)` | Write safe text to the clipboard. |
| `open_text_file()`, `save_text_file(text, name)` | The file dialogs, run in the shell. |
| `session_list/remember/add/forget/clear` | Session secrets. The UI sees `{ id, label, addedAt }`, never the value or its length. |
| `get_settings()`, `set_settings(s)` | Settings, validated in the shell. |
| `app_info()` | Versions, rule count, and what the desktop allows: tray, shield, how the hotkey works (`hotkey`), whether the clipboard needs the window in front (`clipboardNeedsFocus`), whether the hotkey works now (`hotkeyActive`), whether a screen lock can be heard (`lockWatched`, Linux; left out when not known within 1 s, always true on Windows and macOS) and the program a desktop shortcut runs (`executable`; left out when the path is not Unicode). The page's first call also tells the shell that it listens: scrub results and notices held until then are sent, and a held `--scrub-clipboard` start on a needs-focus desktop goes ahead. |
| `scrub_clipboard()` | Scrub the clipboard in place, the same path as the hotkey (the **Scrub now** button on a failed or expired scrub, and the palette's "Scrub the clipboard now"). Takes nothing, returns no text; the result comes as the `clipboard-scrubbed` event. |

The global hotkey and the tray menu scrub the clipboard in place in the shell (`clipscrub.rs`); the window
can only ask for the same scrub through `scrub_clipboard`. Every press gets a number (`press`). The shell
sends seven events:

| Event | Payload |
|---|---|
| `clipboard-scrubbed` | The result plus `press`, the latest press this scrub covers (one scrubbed without waiting, or one whose wait for the window ended in this scrub; never a press still waiting). `status` is `cleaned`, `unverified` (rewritten, but the engine could not verify the output: "Check before pasting"), `stripped` (no secret, but hidden characters removed and written back), `clean`, `empty`, `protected`, `tooLarge` or `error`; also `removed`, `byLabel` and `historyCleaned`. Before the page listens, the latest result is held for its first `app_info`. |
| `clipboard-scrub-waiting` | `{ press }`: that press waits for the window to come to the front. |
| `clipboard-scrub-expired` | `{ press }`: that press waited in vain and did not run. Sent when the wait runs out and again at the next focus within 10 min, so a minimised window still shows it. |
| `app-status` | `{ hotkeyActive, notice }`: the hotkey's state now, and `notice` (`hotkeyNotActive`, `hotkeyLost`, `settingsNotSaved`, `settingsUnreadable`, or null). A notice from before the page listens waits for its first `app_info`, in order. |
| `wipe` | The reason: idle, lock, sleep or tray. |
| `session-changed` | The new list of session secrets, `{ id, label, addedAt }` each. |
| `settings-changed` | The settings now in force. |

The hotkey is not active when it could not be registered at start, when the last key asked for in Settings
was refused and the old one could not come back, or when the portal dropped it. Settings then shows "Not
active: pick another key." under the key; at start and after a loss the window also shows a notice with
**Open Settings** ("Hotkey not active", "Hotkey stopped working").

Besides the commands, the window may only use minimize, maximize, close and drag for the custom title bar,
and listen to the events above. Every IPC message also passes through the isolation page, which lets
through only these calls with the argument shapes the UI sends, and only these event names (`APP_EVENTS`
in `app/isolation/index.js`; `app/ui/tests/unit/isolation.test.ts` replays every UI call in Tauri's IPC shape
and fails to typecheck when an event is missing from its list). Errors are short strings that never quote
text.

## Security boundaries

- The UI never receives a secret value from Rust, and Rust never logs or stores scanned text.
- The window can reach only the app's own backend (CSP `connect-src ipc: http://ipc.localhost`).
- `unsafe` is forbidden in the engine, the wire crate and the CLI, and confined to
  `app/src-tauri/src/platform/windows/`, `platform/macos/` and one library-loading call in
  `platform/linux/tray.rs`.
- Dependencies are pinned (`=` for Tauri and its plugins, exact versions in `package.json`), lockfiles are
  committed, and CI runs `cargo deny` for licenses and advisories.

The full analysis is in [THREAT_MODEL.md](THREAT_MODEL.md).

## Build and release

- **CI** (`.github/workflows/ci.yml`): format, clippy with warnings denied, the Rust tests (Linux, Windows
  and macOS, including the corpus gates), `cargo deny`, UI typecheck, lint, unit tests and the Playwright
  suite against the mock backend. Desktop jobs run the real app: on X11 under Xvfb, on Wayland (headless
  sway: the contract tests and the built app's smoke run; headless KWin in a Plasma 6 container: the
  data-control tests), and on macOS 26 (Apple silicon) and macOS 15 (Intel), where the pasteboard tests, the desktop checks, the built app's
  smoke run and the Playwright suite in WebKit run. Workflows run with read-only permissions and actions are
  pinned by commit. A few tests run on no CI machine (the Windows clipboard tests, the engine's performance
  tests, Wayland smoke W6); [CONTRIBUTING](../CONTRIBUTING.md#run-before-each-release) lists them.
- **Release** (`.github/workflows/release.yml`): a `v*` tag on `main` builds everything on GitHub's runners
  and nowhere else, with the runner's checkout path remapped out of the binaries. It produces the NSIS
  installer, the portable zip, `scrub` for four targets, `SHA256SUMS`, and build provenance attestations,
  then opens a draft release whose notes come from `CHANGELOG.md`. Publishing the draft is manual.
- **Windows build loop** (`tools/win-build.sh`): for local development from WSL, copies the repo to a
  Windows folder and runs the Windows toolchain there.
