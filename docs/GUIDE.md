# redactor — User Guide & Architecture

> **redactor** is a local, privacy-first tool that removes secrets from text before you
> paste it into an AI assistant (ChatGPT, Claude, …). It runs **100% on your machine** —
> nothing is ever sent anywhere. Repo: https://github.com/its30NA/redactor

## What it does, in plain terms

You paste in logs, config, code, or terminal output. It finds anything sensitive — API
keys, passwords, tokens, private keys, database URLs, and optionally personal data — and
replaces each with a **descriptive placeholder** like `[REDACTED: OpenAI API Key]`. The
result keeps enough context for the AI to understand your problem, but reveals none of
your actual secrets.

```
OPENAI_API_KEY=sk-abc...          →  OPENAI_API_KEY=[REDACTED: OpenAI API Key]
Authorization: Bearer eyJ...      →  Authorization: Bearer [REDACTED: JSON Web Token (JWT)]
DATABASE_URL=postgres://u:PASS@h  →  postgres://u:[REDACTED: Connection String Password]@h
```

## Three ways to use it

**1. The desktop app (easiest).** Double-click the **Redactor** icon on your Windows
desktop. It starts the local server and opens the UI at `http://localhost:8765`. Paste on
the left, sanitized text on the right, click **Copy**. A checkbox also redacts personal
info. Close the small console window to stop it.

**2. The web UI via command:** `scrub ui` (flags: `--port`, `--no-browser`, `-c`).

**3. The command line** (for pipes & automation):

```bash
scrub file.log                 # sanitized text to the screen
cat app.log | scrub            # read from a pipe
scrub file.log --preview       # show what WOULD change (line, label, context)
scrub file.log --summary       # add a per-type count
scrub file.log --diff          # before/after diff
scrub file.log --audit log.json# audit record (no raw secrets)
```

## Command reference

| Command | What it does |
|---|---|
| `scrub FILE` | Sanitize a file (or stdin), print the safe version. **Never edits the file.** |
| `scrub ui` | Launch the local web UI. |
| `scrub scan PATH` | Scan files/folders and **report** findings. Read-only. Exits non-zero if any found. |
| `scrub scan PATH --write` | Rewrite files in place, sanitized. The **only** command that edits files. |
| `scrub check` | Scan git-staged files (used by the pre-commit hook). |
| `scrub install-hook` | Install a git pre-commit hook that blocks commits with secrets. |
| `scrub clipboard` | Sanitize the clipboard in place. |

### Common flags

| Flag | Works with | Meaning |
|---|---|---|
| `-c, --config PATH` | most | Use a specific `redactor.toml`. |
| `--summary` | sanitize | Per-type redaction counts. |
| `--preview` | sanitize | Per-redaction report: line, label, masked context. |
| `--diff` | sanitize | Unified before/after diff. |
| `--audit PATH` | sanitize | JSON audit log (`-` = screen). No raw secrets. |
| `--write` | scan | Rewrite files in place (destructive; use in a git repo). |
| `--no-recursive` | scan | Don't descend into subfolders. |
| `--port`, `--host`, `--no-browser` | ui | Server options. |

## What it detects

**Always on (high-precision):** OpenAI, Anthropic, Hugging Face keys; GitHub & GitLab
tokens; AWS access + secret keys; Azure storage key; Google API key & OAuth token; Slack
token & webhook; Discord webhook; Stripe; SendGrid; Twilio; npm; PyPI; JWTs; PEM/OpenSSH
private keys; Bearer/Basic auth; cookies; session IDs; connection-string passwords.

**Heuristic (on):** any value assigned to a suspiciously-named variable (`DB_PASSWORD=`,
`SERVICE_API_KEY=`), even with no vendor prefix.

**Off by default (opt-in):** high-entropy strings (`enabled_detectors = ["high_entropy_string"]`)
and personal info — email, IPv4, phone, US SSN, credit cards (`redact_pii = true`).

## Configuration (`redactor.toml`)

Optional — works with zero setup. Drop it in your project or any parent folder:

```toml
disabled_detectors = ["bearer_token"]
enabled_detectors = ["high_entropy_string"]
redact_pii = true
placeholder_template = "[REDACTED: {label}{suffix}]"

[allowlist]
patterns = ["example\\.com", "sk-xxxx"]

[[rules]]
name = "acme_token"
label = "ACME Internal Token"
pattern = "ACME-[A-Z0-9]{20}"

[llm]
enabled = false
model = "qwen2.5:3b"
```

## How it works (simple terms)

Text flows through five small, independent stages:

1. **Detectors** — each recognizes one kind of secret and reports where it is.
2. **Allowlist** — drops anything you've marked safe.
3. **Resolver** — when two detectors overlap, it **merges** them so no part of a secret is left half-exposed.
4. **Redactor** — swaps each secret for a placeholder; identical secrets get the same number (`#1`, `#2`).
5. **Audit** (optional) — records *what* was removed via a salted fingerprint, never the value.

The real value lives only in memory and is provably absent from the output and audit log.
There is **no networking code** in the core — it literally cannot phone home.

## Privacy guarantees

- Fully offline; zero runtime dependencies (Python standard library only).
- The web server binds to `127.0.0.1` (loopback) — only your machine can reach it.
- The optional AI pass uses a **local** model (Ollama) and is off by default.

## Harsh tests

Copy-paste-ready cases live in [`HARSH_TESTS.md`](HARSH_TESTS.md) — glued-together
secrets, false-positive bait, the email-shaped-password regression, JWT-in-Bearer,
decoys, inline PEM, PII, and stable numbering.

## Troubleshooting

- **Port already in use:** `scrub ui --port 8790`.
- **Browser didn't open (WSL):** open `http://localhost:8765` manually.
- **`scrub` not found:** the global command is a symlink at `~/.local/bin/scrub`; ensure that folder is on your PATH.
- **Scan flagged my test files:** expected (they contain fake secrets); allowlist or ignore them.
