"""LLM-backed detector for ambiguous secrets the regexes miss.

Role in the pipeline: a *span proposer of last resort*. The model reads the text and
returns verbatim substrings it judges sensitive; we locate each one and emit it as an
ordinary ``Match``, so it merges with deterministic matches through the same resolver.

Two hard rules keep this safe:

* **Verbatim only.** We ignore anything the model returns that is not an exact
  substring of the input — the model cannot invent, paraphrase, or hallucinate a
  redaction target into existence.
* **Fail open.** Any backend or parsing failure yields *no* matches (optionally routed
  to ``on_error``) rather than raising, so a missing model never blocks sanitization.
"""

from __future__ import annotations

import json
import re
from collections.abc import Callable, Iterator

from redactor.detectors.base import Detector
from redactor.llm.backend import LLMBackend, LLMUnavailable
from redactor.models import Match

_PROMPT = """\
You are a secret-scanning assistant. Read the TEXT and list every substring that is \
sensitive: credentials, API keys, tokens, passwords, private keys, connection strings, \
or personal data.

Return ONLY a JSON array. Each element is an object with:
  "text": the sensitive substring, copied EXACTLY and VERBATIM from the input
  "type": a short human label (e.g. "API Key", "Password", "Email Address")

Do not invent values. Copy them character-for-character. If nothing is sensitive, \
return [].

TEXT:
```
{text}
```
"""


class LLMSecretDetector(Detector):
    name = "llm_secret"
    default_enabled = False
    category = "llm"

    def __init__(
        self,
        backend: LLMBackend,
        confidence: float = 0.7,
        min_length: int = 4,
        on_error: Callable[[Exception], None] | None = None,
    ) -> None:
        self.backend = backend
        self.confidence = confidence
        self.min_length = min_length
        self.on_error = on_error

    def detect(self, text: str) -> Iterator[Match]:
        if not text.strip():
            return
        try:
            raw = self.backend.complete(_PROMPT.format(text=text))
        except LLMUnavailable as exc:  # fail open — deterministic layer still applies
            if self.on_error:
                self.on_error(exc)
            return

        for item in _parse_items(raw):
            value = str(item.get("text", "")).strip()
            if len(value) < self.min_length or value not in text:
                continue  # verbatim-only: ignore anything not literally in the input
            label = str(item.get("type") or "Sensitive Value").strip() or "Sensitive Value"
            for m in re.finditer(re.escape(value), text):
                yield Match(
                    start=m.start(),
                    end=m.end(),
                    kind="llm_secret",
                    label=label,
                    value=value,
                    confidence=self.confidence,
                    detector=self.name,
                )


def _parse_items(raw: str) -> list[dict]:
    """Best-effort extraction of a JSON array of objects from a model reply."""
    for candidate in (raw, _extract_array(raw)):
        if candidate is None:
            continue
        try:
            parsed = json.loads(candidate)
        except json.JSONDecodeError:
            continue
        if isinstance(parsed, list):
            return [x for x in parsed if isinstance(x, dict)]
    return []


def _extract_array(raw: str) -> str | None:
    start = raw.find("[")
    end = raw.rfind("]")
    if start != -1 and end > start:
        return raw[start : end + 1]
    return None
