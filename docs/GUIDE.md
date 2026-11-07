# User guide

Redactor strips secrets out of text before you paste it into a chat assistant, a ticket or an e-mail. It
replaces each secret with a placeholder and works entirely on your machine.

```
OPENAI_API_KEY=<a long token>         ->  OPENAI_API_KEY=[REDACTED: OpenAI API Key]
curl -u deploy:<a password> https://ci  ->  curl -u deploy:[REDACTED: Password] https://ci
login failed for <the same password>   ->  login failed for [REDACTED: Password]
```

There are two ways to use it: the **desktop app** for Windows, and the **`scrub`** command line for
Windows, Linux and macOS. Both use the same engine. Download links and checksum steps are in the
[README](../README.md#download).

## The desktop app

### Paste and scrub

Open Redactor and paste with `Ctrl+V`, or press **Paste & scrub** in the bottom bar. You can also type or
paste straight into the Input pane, or open a text file with `Ctrl+O` (UTF-8 or UTF-16 text up to 20 MiB).

Three panes appear:

- **Input**: your text, with every secret highlighted.
- **Safe output**: the same text with placeholders. A green **Verified** badge means the output was
  scanned again and nothing that was found is still in it.
- **Findings**: one entry per distinct value, with the line it was first seen on.

Press **Copy safe text** (`Ctrl+Shift+C`) to put the output on the clipboard, or `Ctrl+S` to save it as a
file. Both only ever use the safe output.

Placeholders read `[REDACTED: Password]`. When a label has more than one distinct value, they are numbered
(`#1`, `#2`) and the same value always gets the same number. One value always carries one label: if it
is a password in a URL and again in a log line, both places read `[REDACTED: Password in URL]`. Copies of
a found value in base64, hex, percent-encoding or JSON escapes get the same placeholder. In Settings, **Placeholders** can switch to
**Generic** if you do not want the labels to show what kind of secret was there.

### Sensitivity

The segmented control in the bottom bar (or `Ctrl+1`, `Ctrl+2`, `Ctrl+3`) sets how eager detection is.

| Level | Behavior |
|---|---|
| **Relaxed** | Only confident findings. Fewest false alarms, most likely to miss something unusual. |
| **Balanced** | The default. |
| **Paranoid** | Flags anything plausible, including random-looking strings with no key name and long hex runs. Expect some false positives. Use it for text you cannot afford to get wrong. |

The **PII** switch adds personal data: e-mail addresses, phone numbers, IBANs, card numbers, IP addresses
and US social security numbers. It is off by default because that data is often meant to be in the text.

### Findings: Keep, Redact and Remember

Detection is heuristic, so you stay in charge. Each entry in the findings list shows what was found, why
(for example "GitHub token format, checksum not valid"), and a confidence. They are grouped as **Certain**,
**Likely** and **Possible**. Click an entry to jump to it in the text.

- **Keep** says "this is not a secret". The value stays in the output, everywhere it occurs. The button
  turns into **Redact** so you can change your mind.
- **Redact** (on a selection): select any text in the Input pane and a small bar appears with **Redact**
  and **Remember**. Redact adds your selection to the findings. Use it for what the engine missed.
- **Remember** keeps the value for this session, so it is redacted wherever it appears from now on,
  including inside encoded text, until you forget it, quit, lock Windows or wipe. Use it for a password
  that has no recognizable shape.

### The Remembered sheet

Open it with the bookmark button in the title bar. It has two lists.

**This session** holds the secrets you asked Redactor to remember. It shows each one by label and time,
never the value and not even its length. They live in memory only and are never written to disk. **Forget**
removes one, **Forget all** removes all of them, and **Add a secret** lets you type one into a masked
field (at least 6 characters) with a label of your choice. They are also forgotten when you quit, when
Windows locks, on sleep, and after the idle timeout.

**Always redact** is a list of plain words for things that are not passwords: your name, your company,
internal host names. Whole words, any case, at least 3 characters, up to 500. It is stored on this PC in
plain text, and the sheet says so. If you type something that looks like a password, Redactor suggests
"This session" instead. Put passwords in "This session", never in this list.

### Scrub the clipboard from anywhere: `Ctrl+Alt+R`

Press `Ctrl+Alt+R` in any app, without opening Redactor. It scrubs whatever text is on the clipboard in
place and shows a Windows notification:

- "2 secrets removed, safe to paste", or
- "Nothing sensitive found", or
- "Left alone: a password manager marked that copy private", or
- "Left alone: the clipboard text is larger than 20 MiB".

Copies that a password manager marked private are never read or changed. If something else is copied
while the scan runs, the new copy is left alone and the scrub reports an error; press the hotkey again. When something was removed, the
raw copy is also deleted from the Win+V clipboard history, on a best-effort basis. The hotkey uses your
current sensitivity and PII settings. You can change it in Settings (use at least two modifier keys; a few
combinations are reserved). If another program already holds the key, Redactor keeps the old one and says
so.

### Screen-share shield

On by default. Screenshots and screen sharing see a blank window instead of your text. It asks Windows to
exclude the window from capture, which is best effort: some capture tools can bypass it. Switch it in
Settings (**Screen-capture shield**) or from the tray. The title bar shows "Shield on" while it is active.

### Auto-wipe: idle, lock and sleep

Redactor clears its text panes and forgets every remembered secret:

- after the idle timeout: **Wipe when idle**, 5, 10 or 30 minutes, or off (default 10);
- when Windows locks or goes to sleep: **Wipe when Windows locks** (on by default);
- when you quit, or choose **Wipe now** in the tray.

The **Wipe** button (`Ctrl+Shift+X`) clears both panes and the undo history so that undo cannot bring the
text back. It does not touch the remembered list: use **Forget all** for that.

### The tray

Redactor lives in the notification area. A left click opens the window. The menu has **Open Redactor**,
**Scrub clipboard**, **Screen-share shield** (a check mark), **Wipe now** and **Quit**. When "Keep running in
the tray" is on (default), closing the window hides it, and the hotkey keeps working. Only one copy runs:
starting it again brings the first one forward.

### Keyboard

| Keys | Action |
|---|---|
| `Ctrl+V` | Paste and scrub |
| `Ctrl+Shift+C` | Copy safe text |
| `Ctrl+O` / `Ctrl+S` | Open a text file / save a safe copy |
| `Ctrl+Shift+X` | Wipe both panes |
| `Ctrl+1` / `Ctrl+2` / `Ctrl+3` | Relaxed / Balanced / Paranoid |
| `Ctrl+K` | Command palette |
| `Esc` | Close the sheet or dialog |
| `Ctrl+Alt+R` | Scrub the clipboard (global, remappable) |

### Settings

Stored in `%APPDATA%\Redactor\settings.json` and never contain any scanned text. Settings are: sensitivity,
personal data, strip invisible characters, placeholder style, screen-capture shield, wipe when idle, wipe
when Windows locks, the clipboard hotkey, keep running in the tray, theme (follow Windows, dark or light)
and motion. **Strip invisible characters** removes zero-width, bidirectional and tag characters from the
output; they can hide text from you, so leave it on unless you need them.

## The `scrub` command line

`scrub` reads a file or standard input and writes the redacted text to standard output. Summaries and
notices go to standard error, so pipes stay clean.

### Redact text

```bash
scrub app.log                  # a file
cat app.log | scrub            # standard input (or: scrub -)
scrub app.log > safe.log
```

Options for this mode:

| Option | Meaning |
|---|---|
| `--sensitivity relaxed\|balanced\|paranoid` | How eager detection is. Overrides the config file. |
| `--pii` | Also redact personal data. |
| `--no-strip` | Keep invisible, bidi and tag characters in the output. |
| `--summary` | Print counts per label to stderr. |
| `--preview` | Print each redaction (line, label, masked context) to stderr. |
| `--diff` | Print a unified diff instead of the text. **It shows the original lines, secrets included. Do not share it.** |
| `--audit PATH` | Write a JSON audit log: time, source, counts, lines, labels, detectors. Never values. `-` writes to stderr. |
| `--json` | Print the result as JSON (the same shape the desktop app uses): output, findings, spans, warnings. |
| `-c, --config PATH` | Use this config file. |

```console
$ scrub app.log --summary
DB_PASSWORD=[REDACTED: Password #1]
host=db.example.test
login failed for [REDACTED: Password #1]
$ curl -u deploy:[REDACTED: Password #2] https://ci.example.test/api
Mail anna@example.test, IP 203.0.113.9
Redacted 3 item(s):
  - Password: 3
```

Add `--pii` and the address and the IP go too. This mode exits 0 whether or not it found anything.

The input may be UTF-8 or UTF-16 (with a byte-order mark, as Windows PowerShell 5.1 writes with `>`). The
redacted text comes out in the same encoding; `--json` and `--diff` are always UTF-8. Binary input is
refused with exit code 2 rather than passed through. Text in an older 8-bit encoding is read with
replacement characters, and a notice says so.

### `scrub scan`: find secrets in files

```bash
scrub scan                       # the current folder
scrub scan src config/app.env   # files and folders
scrub scan . --no-recursive     # do not descend into folders
scrub scan . --write            # rewrite files in place, sanitized
```

By default it only reports, one line per file with the labels, and exits **1** if anything was found, so it
works in CI. `--write` is the only option that changes files: it replaces the secrets in place, so use it in
a git repository where you can review the diff. `scan` skips `.git`, `node_modules`, virtual environments
and similar folders, and symbolic links it meets while walking.

Files are read as UTF-8 or UTF-16 (with a byte-order mark, or plainly UTF-16 text without one). Every file
that is not scanned gets a `skipped` line on stderr with the reason: binary, larger than 5 MiB, or a
byte-order mark the content does not follow. The last one, and a file that cannot be read at all, make the
exit code 2.

`--write` keeps each file's encoding, byte-order mark and permissions. It writes a temporary file next to
the original and renames it over, so a file is never left half-written. Right before, it checks that the
path is still the regular file it read: if it was replaced meanwhile (by a symbolic link, say), or is a
symbolic link named on the command line, the file is left unchanged and the run ends with exit code 2.
Files in an older 8-bit encoding are reported but not rewritten.

### `scrub check` and the pre-commit hook

`scrub check` scans what is staged in git: the content the commit would record, not the working tree. It
exits 1 if it finds anything.

- Every added, modified, renamed or copied file is checked in full. Rename detection is off for the
  check, so `git mv app.env.example app.env` followed by filling in a password is checked as the new file
  it is.
- Each file is read by its object id, never by its name, so file names with spaces, line breaks,
  non-ASCII characters or a `:` are checked correctly.
- Deleted files are ignored. Submodules are skipped, with a `skipped` line: their own repositories have
  their own hooks.
- A staged symbolic link is checked as what git stores for it, the path it points to. The file behind
  the link is not part of the commit and is not read.
- UTF-16 files are decoded and checked. Binary files are skipped with a `skipped` line.
- A staged text file the check cannot read (larger than 5 MiB, a byte-order mark the content does not
  follow, an unresolved conflict) blocks the commit with exit code 2 and a message naming it. The check
  never passes text it did not read.
- If the config file in use lowers detection (an `allowlist`, `disabled` rules or
  `sensitivity = "relaxed"`), `check` prints a warning naming the file and those settings.

```bash
scrub install-hook          # writes .git/hooks/pre-commit
scrub install-hook --force  # overwrite an existing hook
```

The hook runs `scrub check` before every commit and stops the commit if a secret is staged. `scrub` must
be on your `PATH`. In an emergency, `git commit --no-verify` skips it.

### `scrub clipboard`

```bash
scrub clipboard
```

Windows only. Cleans the clipboard text in place and prints what it did. Copies marked private by a
password manager are left alone. One PowerShell helper reads the clipboard, waits for the redacted text,
reads the clipboard again and writes only if it still holds exactly what was read. If you copied something
else in the meantime (a fresh password-manager copy, say), nothing is written and the command exits 2;
run it again. Unlike the desktop app, it does not remove the raw copy from Win+V history.

### `scrub detectors` and `scrub version`

```bash
scrub detectors             # a table of every rule
scrub detectors --markdown  # the same as Markdown (this is how docs/DETECTORS.md is made)
scrub version               # scrub version, engine version, rule count
```

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. |
| 1 | `scan` or `check` found secrets, or `install-hook` found an existing hook. |
| 2 | Usage, input or environment error (unreadable file, bad or refused config, not a git repository, a staged file `check` could not read, a clipboard that changed during `scrub clipboard`). |
| 101 | An internal error (a bug). `scrub` prints one line with the source location; the details are withheld, because they could quote your text. |

## The config file

`scrub` looks for `redactor.toml` or `.redactor.toml` in the current folder and then in each folder above
it, up to the root of the git repository (the folder holding `.git`) or your home folder, whichever comes
first. It never reads a config file from above those. `-c PATH` names a file directly. Every key is
optional. [`examples/redactor.example.toml`](../examples/redactor.example.toml) is a commented starting
point. (The desktop app uses its own settings, not this file.)

| Key | Meaning |
|---|---|
| `sensitivity` | `"relaxed"`, `"balanced"` (default) or `"paranoid"`. |
| `pii` (or `redact_pii`) | `true` to redact personal data. Default `false`. |
| `strip_invisible` | Remove zero-width, bidi and tag characters from the output. Default `true`. |
| `terms` | A list of words that are always redacted, whole words, any case. |
| `placeholder` | `"labeled"` (default) or `"generic"`. |
| `template` (or `placeholder_template`) | The placeholder text, with `{label}` and `{suffix}` (the ` #N` number). Default `"[REDACTED: {label}{suffix}]"`. |
| `disabled` (or `disabled_detectors`) | Rule ids to turn off. `scrub detectors` lists them. |
| `enabled` (or `enabled_detectors`) | Rule ids that ship switched off and should run. An entry in `disabled` still wins. |
| `allowlist` | Regexes tested against each detected value; matches are left alone. Either `allowlist = [...]` or a table `[allowlist]` with `patterns = [...]`. |
| `[[rules]]` (or `custom_rules`) | Your own detectors: `name` (or `id`), `label`, `pattern` (a regex), `confidence` (default 0.9) and `group` (which capture group is the secret; default 0, the whole match). |

```toml
sensitivity = "balanced"
terms = ["Contoso"]
disabled = ["bearer_token"]

[allowlist]
patterns = ["example\\.com"]

[[rules]]
name = "acme_internal_token"
label = "ACME Internal Token"
pattern = "\\bACME-[A-Z0-9]{20}\\b"
confidence = 0.95
```

An `[llm]` section from version 1 is still accepted and ignored with a one-line notice, because version 2
has no language-model pass. Unknown keys are ignored with a notice too.

### Config files are never silent

A config file can switch detection off, so `scrub` treats one with care:

- **It always says which file it uses.** Whenever a config file is loaded, the first line on stderr is
  `scrub: using config: PATH`.
- **It refuses files someone else could have planted** (Linux and macOS): a config file that every user
  may write to, or that belongs to another user (root excepted), stops the run with exit code 2. Fix it
  with `chmod o-w FILE`, or move it.
- **It refuses catch-all allowlists.** An `allowlist` pattern that matches the empty string (`.*`, `a*`,
  `|x`), or that matches a large share of random secret-shaped values made up at run time (`\w`, `\d`,
  `[a-z]`, a single letter), stops the run with exit code 2. Narrow patterns keep working: literal
  placeholder values, `example\.com`, a fixture prefix such as `^test_`, or a 40-hex-digit commit id.
- **`scrub check` warns about weaker settings.** Before the pre-commit check runs, it names the config file
  and lists what in it lowers detection: allowlist patterns (counted), disabled rules, and
  `sensitivity = "relaxed"`.

## Troubleshooting

- **Missed secret.** Select it in the Input pane and choose Redact, or Remember it. Try Paranoid
  for a one-off. Report patterns that are worth adding as an issue, with a made-up example.
- **Something harmless was redacted.** Press Keep on its finding. For the command line, add an
  `allowlist` regex or disable the rule.
- **The hotkey does nothing.** Another program may hold `Ctrl+Alt+R`. Pick another in Settings.
- **SmartScreen warns on the installer.** The binaries are not code-signed. See the
  [threat model](THREAT_MODEL.md#the-unsigned-binary-and-smartscreen) and verify the download.
- **`scrub scan` flags my test files.** If they contain fake secrets on purpose, allowlist the values or
  keep the tests generating them at run time.

More hard cases to try are in [HARSH_TESTS.md](HARSH_TESTS.md).
