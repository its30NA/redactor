# Project status

**Status:** feature-complete (milestones M0 to M6). 116 tests pass; CI runs
lint and tests on Python 3.11, 3.12 and 3.13.

redactor is a local tool that removes secrets from text before it is shared
with an external AI assistant. It runs offline and uses only the Python
standard library at runtime. For usage see [GUIDE.md](GUIDE.md); for an
overview see the [README](../README.md).

## Changelog

| Milestone | Changes |
|---|---|
| M0 | Deterministic CLI core: pipeline, TOML config, `scrub` command, stdlib only |
| M1 | Structural detectors for API keys, tokens, private keys, connection strings |
| M2 | Heuristic assignment detection, opt-in high-entropy detector, salted-fingerprint audit log |
| M3 | Opt-in PII detectors, user-defined regex rules, `--preview` |
| M4 | Optional local LLM pass (Ollama), off by default, fails open |
| M5 | Folder scan, git pre-commit hook, clipboard support, GitHub Actions CI |
| Fix | Overlapping matches are now merged to their union instead of dropped, which closes a partial-leak case |
| M6 | Local web UI (`scrub ui`) |
| Packaging | Windows launcher and icon, user guide, harsh test cases, README screenshot |

## Design notes

- **Deterministic first, LLM optional.** Most secrets have known shapes and
  are caught by pattern detectors. The optional LLM pass only adds matches
  and never breaks the offline default.
- **Overlaps are merged, never dropped.** Keeping only the highest-confidence
  match can leave part of a secret in the output. Merging overlapping spans
  avoids that.
- **Tiered detectors.** Structural detectors are on by default; entropy and
  PII detectors are opt-in to keep false positives low.

## Possible next steps

- Harden detectors against more real-world files.
- Branch protection on the default branch.
