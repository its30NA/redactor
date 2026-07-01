<img src="assets/redactor.png" alt="redactor icon" width="96" align="left">

# redactor

**Local, privacy-first text sanitizer.** Redact secrets from logs, source, config, and
terminal output *before* you paste them into an external AI assistant (ChatGPT, Claude, …).

<br clear="left">

> 📖 [User guide](docs/GUIDE.md) · [harsh test cases](docs/HARSH_TESTS.md) ·
> [project status / handoff](docs/PROJECT_STATUS.md) — paste the harsh tests into
> `scrub ui` to see it handle deliberately confusing input.

## Screenshots

The local web UI (`scrub ui`) — paste on the left, safe-to-share text on the right,
highlighted placeholders, and a live per-type count:

![redactor web UI](assets/ui.png)

- **Runs entirely offline.** Zero runtime dependencies; the deterministic core is
  stdlib-only. Nothing is ever sent anywhere — there is no networking code to send it.
- **Context-preserving.** Secrets become descriptive placeholders, not black boxes, so
  the assistant still understands the surrounding text:

  ```
  OPENAI_API_KEY=[REDACTED: OpenAI API Key]
  Authorization: Bearer [REDACTED: Bearer Token]
  ```

- **Extensible by design.** Add a detector = add one small class.

> Status: **Milestone 6** — local web UI (`scrub ui`), on top of the full CLI, folder
> scan, git hook, and clipboard tooling. See the [roadmap](#roadmap).

## Install

```bash
cd redactor
python -m venv .venv && source .venv/bin/activate
pip install -e ".[dev]"
```

## Use

**Sanitize** a file or stream (the default command):

```bash
scrub secrets.env                 # sanitized text to stdout
cat app.log | scrub               # read from stdin
scrub app.log --summary           # + per-kind counts on stderr
scrub app.log --diff              # unified diff of what changed
scrub app.log --preview           # per-redaction report (line, label, context)
scrub app.log --audit audit.json  # write a redaction audit log (no raw values)
scrub app.log -c redactor.toml    # explicit config
```

Sanitized text goes to **stdout**; summaries/warnings go to **stderr**, so `scrub`
composes cleanly in pipes.

**Web UI** (M6) — the friendliest way in; paste-in, sanitized-out, one Copy button:

```bash
scrub ui                     # opens http://127.0.0.1:8765 in your browser
```

Runs a stdlib-only local server bound to loopback — nothing leaves your machine. Live
redaction as you type, a toggle for personal data, and highlighted placeholders.

> Want a global `scrub` command (no venv activation) or a **double-click desktop
> shortcut** on Windows/WSL? See [`packaging/`](packaging/README.md).

**Integrations** (M5):

```bash
scrub scan .                 # scan a tree; report findings, exit 1 if any (CI-friendly)
scrub scan . --write         # rewrite files in place, sanitized
scrub install-hook           # add a git pre-commit hook that blocks committing secrets
scrub clipboard              # sanitize the clipboard in place (WSL/macOS/Linux)
```

The pre-commit hook runs `scrub check` against staged files and blocks the commit if
anything sensitive is found (bypass with `git commit --no-verify`).

As a library:

```python
from redactor import Pipeline

result = Pipeline().sanitize("OPENAI_API_KEY=sk-...")
print(result.text)            # OPENAI_API_KEY=[REDACTED: OpenAI API Key]
print(result.redaction_count) # 1
```

## What it detects (M1)

High-precision, structural formats grouped by domain — 25 detectors in all:

| Domain | Detectors |
|---|---|
| **AI providers** | OpenAI, Anthropic, Hugging Face |
| **Version control** | GitHub (classic + fine-grained), GitLab |
| **Cloud** | AWS access key ID, AWS secret key *(contextual)*, Azure storage key, Google API key, Google OAuth token |
| **SaaS** | Slack token, Slack webhook, Discord webhook, Stripe, SendGrid, Twilio, npm, PyPI |
| **Crypto** | PEM/OpenSSH private keys, JWTs |
| **HTTP layer** | `Bearer` / `Basic` auth, cookies, session IDs |
| **Connection strings** | password inside `scheme://user:pass@host/db` (context preserved) |

*Contextual* detectors (marked above) only fire next to their tell-tale key name,
because the value alone (e.g. a bare 40-char AWS secret) is indistinguishable from
ordinary data.

### Heuristic detection (M2)

Two detectors cover secrets with no fixed vendor shape:

- **Assignment** *(on by default)* — a value assigned to a suspiciously-named variable
  (`MYAPP_DB_PASSWORD=…`, `SERVICE_API_KEY=…`). Fires only when the key name signals a
  secret and the value survives a placeholder guard (`changeme`, `${VAR}`, `true`, … are
  ignored). Labeled by keyword: `Password`, `API Key`, `Token`, …
- **High-entropy string** *(off by default)* — long, random-looking blobs with no
  recognizable prefix. This is the noisiest detector, so it's opt-in via config
  (`enabled_detectors = ["high_entropy_string"]`). Enable it when you value recall over
  precision.

### PII (M3, opt-in)

Email, IPv4, US SSN, phone numbers, and credit-card numbers (Luhn-validated). The whole
group ships **off** — PII is often legitimately part of the text you want the AI to reason
about — and is enabled with `redact_pii = true` in config, or per-detector via
`enabled_detectors`.

### User-defined rules (M3)

Any pattern the built-ins miss can be added from config, no code required:

```toml
[[rules]]
name = "acme_internal_token"
label = "ACME Internal Token"
pattern = "\\bACME-[A-Z0-9]{20}\\b"
group = 0   # optional: which capture group is the secret
```

Custom rules run through the same resolver and redactor as everything else.

### Optional local-LLM pass (M4, off by default)

A best-effort second sweep for ambiguous secrets with no fixed shape (a password in
prose, an internal hostname). It runs **entirely locally** via [Ollama](https://ollama.com)
— nothing leaves your machine — and is strictly additive to the deterministic layer:

- **Verbatim only** — the model's proposals are accepted only if they appear *literally*
  in the input, so it can't hallucinate a redaction into existence.
- **Fails open** — if the model isn't running or misbehaves, sanitization proceeds with
  the deterministic results, unharmed.

```bash
ollama pull qwen2.5:3b      # ~2 GB; llama3.2:3b also works
```
```toml
[llm]
enabled = true
model = "qwen2.5:3b"
```

The backend is a one-method protocol (`complete(prompt) -> str`), so swapping Ollama for
llama.cpp or a test double is trivial — the detector never knows the difference.

### Audit log

`--audit PATH` writes a JSON record of *what* was redacted — kind, label, offset, length,
and a **salted fingerprint** of each value — but never the raw secret. A fresh random salt
per run lets you spot duplicate values *within* a run (identical fingerprints) while making
the log useless as a hash oracle for confirming guessed secrets across runs.

## How it works

```
text ─▶ detectors ─▶ allowlist filter ─▶ resolve overlaps ─▶ redact ─▶ sanitized text
```

- **Detectors** (`detectors/`) each find one secret family and yield `Match` spans. A
  `Match` holds the raw value **in memory only** — it is never written out.
- **Allowlist** (`allowlist.py`) exempts known-safe look-alikes (doc placeholders,
  `example.com`).
- **Resolver** (`resolver.py`) greedily keeps the highest-confidence, non-overlapping
  set so replacements never collide.
- **Redactor** (`redaction.py`) splices in placeholders right-to-left and applies
  **stable numbering** — the same secret gets the same `#N`, distinct secrets differ —
  so relational meaning survives without revealing values.

## Configure

Drop a `redactor.toml` at or above your working directory. See
[`redactor.example.toml`](redactor.example.toml). Disable detectors, extend the
allowlist, or change the placeholder template. Config is optional.

## Add a detector

```python
# src/redactor/detectors/saas.py  (or the themed module that fits)
class LinearApiKeyDetector(RegexDetector):
    name = "linear_api_key"
    kind = "linear_api_key"
    label = "Linear API Key"
    pattern = re.compile(r"\blin_api_[A-Za-z0-9]{40,}\b")
```

Then list it in `_DEFAULT_DETECTOR_CLASSES` in `detectors/__init__.py`, and add a match
case + a false-positive case to `tests/test_detectors.py`. Done — nothing else changes.

## Test

```bash
pytest        # unit + golden + false-positive corpus
ruff check .  # lint
```

## Roadmap

- **M0 — Deterministic CLI core** ✅
- **M1 — Full structural suite** ✅ — 25 detectors across AI/VCS/cloud/SaaS/crypto/HTTP/connection strings
- **M2 — Heuristic detection + audit log** ✅ — assignment & entropy detectors, salted-fingerprint audit trail
- **M3 — Config maturity** ✅ — PII group + toggle, user-defined rule patterns, `--preview` report
- **M4 — Optional local-LLM pass** ✅ — Ollama-backed, verbatim-only, fails open
- **M5 — Integrations** ✅ — folder scan, git pre-commit hook, clipboard, CI exit codes
- **M6 — Local web UI** ✅ *(this release)* — `scrub ui`: stdlib-only loopback server, live redaction, PII toggle

Future ideas: IDE extension, richer PII models, per-project rule packs.

## License

MIT © Sina Kashani
