# redactor

**Local, privacy-first text sanitizer.** Redact secrets from logs, source, config, and
terminal output *before* you paste them into an external AI assistant (ChatGPT, Claude, …).

- **Runs entirely offline.** Zero runtime dependencies; the deterministic core is
  stdlib-only. Nothing is ever sent anywhere — there is no networking code to send it.
- **Context-preserving.** Secrets become descriptive placeholders, not black boxes, so
  the assistant still understands the surrounding text:

  ```
  OPENAI_API_KEY=[REDACTED: OpenAI API Key]
  Authorization: Bearer [REDACTED: Bearer Token]
  ```

- **Extensible by design.** Add a detector = add one small class.

> Status: **Milestone 0** — deterministic CLI core. See the [roadmap](#roadmap).

## Install

```bash
cd redactor
python -m venv .venv && source .venv/bin/activate
pip install -e ".[dev]"
```

## Use

```bash
scrub secrets.env                 # sanitized text to stdout
cat app.log | scrub               # read from stdin
scrub app.log --summary           # + per-kind counts on stderr
scrub app.log --diff              # unified diff of what changed
scrub app.log -c redactor.toml    # explicit config
```

Sanitized text goes to **stdout**; summaries/warnings go to **stderr**, so `scrub`
composes cleanly in pipes.

As a library:

```python
from redactor import Pipeline

result = Pipeline().sanitize("OPENAI_API_KEY=sk-...")
print(result.text)            # OPENAI_API_KEY=[REDACTED: OpenAI API Key]
print(result.redaction_count) # 1
```

## What it detects (M0)

High-precision, structural formats plus one contextual case: OpenAI, Anthropic, and
GitHub tokens; AWS access key IDs; Slack tokens; JWTs; PEM/OpenSSH private-key blocks;
and `Authorization: Bearer` tokens. Heuristic detectors (assignment patterns,
high-entropy strings) land in a later milestone so this first suite stays trustworthy —
almost everything it flags is a real credential.

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
# src/redactor/detectors/builtins.py
class StripeKeyDetector(RegexDetector):
    name = "stripe_secret_key"
    kind = "stripe_secret_key"
    label = "Stripe Secret Key"
    pattern = re.compile(r"\bsk_live_[A-Za-z0-9]{24,}\b")
```

Then add it to `default_detectors()` in `detectors/__init__.py`. Add a match case and a
false-positive case to `tests/test_detectors.py`. Done.

## Test

```bash
pytest        # unit + golden + false-positive corpus
ruff check .  # lint
```

## Roadmap

- **M0 — Deterministic CLI core** ✅ *(this release)*
- **M1** — Full structural suite (all provider keys, connection strings, cloud creds, cookies, webhooks)
- **M2** — Heuristic detectors (assignment patterns, entropy), audit log
- **M3** — Config maturity: user rules, PII toggle, richer preview/diff UX
- **M4** — *Optional* local-LLM pass for ambiguous cases (off by default)
- **M5** — Integrations: clipboard watch, git pre-commit hook, folder scan, IDE

## License

MIT © Sina Kashani
