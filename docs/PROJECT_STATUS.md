# Project Status & Session Handoff

_Last updated: 2026-07-02. This is the "pick up where we left off" doc — a snapshot of
what the project is, where everything lives, and what's next. For end-user usage see
[GUIDE.md](GUIDE.md); for the public overview see [../README.md](../README.md)._

---

## TL;DR

**redactor** is a local, privacy-first tool that removes secrets from text before you
paste it into an AI assistant. Runs 100% offline, zero runtime dependencies (Python
stdlib only), no networking code in the core. **Status: feature-complete (M0–M6),
116 tests passing, CI green on Python 3.11/3.12/3.13.**

- **Repo (master + backup):** https://github.com/its30NA/redactor (public)
- **Local working copy:** `/home/its30na/code/its30NA/redactor` (WSL, Ubuntu distro)
- **~2,170 LOC** across `src/redactor/`

---

## Where everything lives

| Location | What | Notes |
|---|---|---|
| GitHub `its30NA/redactor` | Everything, versioned | This is the durable backup — re-clone to restore |
| `/home/its30na/code/its30NA/redactor` | Code + `.venv` | Where `scrub` runs from |
| `~/.local/bin/scrub` | Symlink → `.venv/bin/scrub` | Global command in any WSL shell |
| Desktop `redactor.lnk` | Launcher shortcut (custom padlock icon) | → `C:\Users\skash\AppData\Local\redactor\{redactor.bat, redactor.ico}` |
| `docs/` | GUIDE.md, HARSH_TESTS.md, PROJECT_STATUS.md | |
| Personal Notion | Imported copy of GUIDE.md | User's own workspace |

**Desktop launcher depends on** the WSL repo staying at
`/home/its30na/code/its30NA/redactor`. If moved, update `AppData\Local\redactor\redactor.bat`.

---

## How to resume

```bash
cd ~/code/its30NA/redactor
source .venv/bin/activate      # or just use the global `scrub`

pytest                         # 116 tests
ruff check .                   # lint

scrub ui                       # web UI at http://localhost:8765
scrub file.log --preview       # CLI dry-run
```

Everything also works via the desktop **redactor** shortcut (double-click).

---

## Architecture (one screen)

```
text → [detectors] → [allowlist] → [resolver: merge overlaps] → [redactor] → safe text
                                                                      └→ [audit] (optional)
```

The `Match` dataclass (`models.py`) is the one contract every stage speaks. The raw
secret value lives only in memory and is provably absent from output and audit.

| Module (`src/redactor/`) | Responsibility |
|---|---|
| `models.py` | `Match` type (start, end, kind, label, value, confidence, detector) |
| `detectors/` | One class per secret family; `default_detectors()` / `build_detectors()` registry |
| `allowlist.py` | Drop user-exempted matches |
| `resolver.py` | **Merge** overlapping spans to union (never drop → never partial-leak) |
| `redaction.py` | Placeholder templating + stable per-value numbering (`#1`, `#2`) |
| `config.py` | TOML → `Config` (detectors, PII, custom rules, allowlist, LLM) |
| `pipeline.py` | Wires it together; `Pipeline.from_config()` is the assembly point |
| `cli.py` | argparse subcommands; `sanitize` is the default |
| `scanner.py` | Folder/file scan (read-only report or `--write`) |
| `githook.py` | `install-hook` + `check` (pre-commit blocking) |
| `clipboard.py` | Cross-platform clipboard read/write |
| `webui/` | stdlib http.server UI (`server.py` + `index.html`) |
| `llm/` + `detectors/llm.py` | Optional local (Ollama) pass, off by default |
| `audit.py` | Salted-fingerprint record of what was redacted (never the value) |

---

## Detectors (27 on by default, 33 total)

- **Structural (on, 26):** OpenAI, Anthropic, Hugging Face, GitHub, GitLab, AWS access +
  secret, Azure storage, Google API + OAuth, Slack token + webhook, Discord webhook,
  Stripe, SendGrid, Twilio, npm, PyPI, JWT, PEM/OpenSSH private keys, Bearer, Basic auth,
  cookies, session IDs, connection-string passwords.
- **Heuristic assignment (on, 1):** `SOMETHING_PASSWORD=`, `X_API_KEY=`, etc. with a
  placeholder guard (`changeme`, `${VAR}`, `true` are ignored).
- **High-entropy (off, opt-in):** `enabled_detectors = ["high_entropy_string"]`.
- **PII (off, opt-in, 5):** email, IPv4, US SSN, phone, credit card (Luhn) —
  `redact_pii = true` or the UI checkbox.
- **Custom rules:** user regex via `[[rules]]` in `redactor.toml`.
- **LLM:** optional local Ollama pass, off unless `[llm] enabled = true`.

---

## Milestone log (all shipped)

| # | What |
|---|---|
| M0 | Deterministic CLI core: pipeline, config, `scrub` CLI, stdlib-only |
| M1 | Full structural detector suite (themed modules) |
| M2 | Heuristic detection (assignment + opt-in entropy) + salted-fingerprint audit log |
| M3 | PII group + toggle, user-defined rules, `--preview` |
| M4 | Optional local-LLM pass (Ollama, verbatim-only, fails open) |
| M5 | Folder scan, git pre-commit hook, clipboard, GitHub Actions CI |
| — | **Fix:** resolver now merges overlaps (found a real password-leak bug on live data) |
| M6 | Local web UI (`scrub ui`) |
| — | Packaging: global `scrub`, Windows/WSL desktop launcher, custom icon, guide + harsh tests, README screenshot |

---

## Notable decisions & lessons

- **Deterministic-first, LLM optional.** ~95% of secrets have known shapes; the LLM only
  assists on the ambiguous remainder and never breaks the offline guarantee.
- **Resolver merges, never drops.** A greedy "keep highest-confidence" design leaked a
  password prefix when an email-shaped password partially overlapped. For a security tool,
  partial coverage is a bug → merge to the union.
- **Tiered detectors for trust.** Structural = on; heuristic entropy + PII = opt-in, so the
  default set almost never false-positives.
- **The thesis proved itself 3×** in one session: Cloudflare blocked a doc write, GitHub
  push-protection blocked a commit, and the resolver leak — all real systems reacting to
  secret-shaped strings, which is exactly what this tool prevents.

---

## Open threads / next steps

- [ ] **Architecture walkthrough** — user wants a guided read of the code (paused here).
  Suggested reading order: `models.py` → `detectors/base.py` → `detectors/ai_providers.py`
  → `pipeline.py` → `resolver.py` → `redaction.py`.
- [ ] **Delete the stray Notion page** — it was created in the shared "Notion von Erik"
  workspace (connector's only authorized workspace) and moved under **Software &
  Development** so it's visible/deletable. The real guide is already imported into the
  user's personal workspace + lives in `docs/GUIDE.md`.
- [ ] **Optional extras:** UI screenshot is done; could add a "Why I built this" section
  for portfolio; branch protection so the green CI badge carries weight; an extra backup
  (git bundle / second remote) if desired.
- [ ] **Detector hardening** against more real-world files as they come up.

---

## Environment notes

- WSL2 (Ubuntu), Windows host user `skash`; laptop dGPU is driven by the external HDMI
  monitor (unrelated to this project — a past debugging tangent).
- Git identity: Sina Kashani; commits authored solely by the user (no AI trailers).
- Ollama daemon may be running but is idle and unused unless the LLM detector is enabled.
