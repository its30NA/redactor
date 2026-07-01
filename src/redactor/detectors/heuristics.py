"""Heuristic detectors — for secrets that have no fixed vendor shape.

Structural detectors recognize *known formats*. But plenty of secrets are just
"a value assigned to a suspiciously-named variable" (``DB_PASSWORD=...``) or "a long
random-looking blob" with no recognizable prefix. These two detectors cover that gap.

They are inherently lower-precision than structural detectors, so:

* :class:`AssignmentSecretDetector` fires only when the *key name* signals a secret
  and the value survives a placeholder guard — it ships enabled but at low confidence.
* :class:`HighEntropyStringDetector` is the noisiest of all, so it ships **disabled**
  (``default_enabled = False``) and must be turned on explicitly in config.
"""

from __future__ import annotations

import math
import re
from collections import Counter
from collections.abc import Iterator

from redactor.detectors.base import Detector
from redactor.models import Match

# Keyword in a variable name -> the human label to use for its redacted value.
# Ordered: the first keyword found in the key name wins, so more specific terms
# ("private_key") should precede generic ones ("key"/"secret").
_KEYWORD_LABELS: tuple[tuple[str, str], ...] = (
    ("private_key", "Private Key"),
    ("access_key", "Access Key"),
    ("secret_key", "Secret Key"),
    ("api_key", "API Key"),
    ("apikey", "API Key"),
    ("password", "Password"),
    ("passwd", "Password"),
    ("pwd", "Password"),
    ("token", "Token"),
    ("secret", "Secret"),
    ("credential", "Credential"),
)

# Values that match a secret-ish key but are obviously *not* secrets. Kept
# deliberately conservative — when in doubt we redact.
_PLACEHOLDER_VALUE = re.compile(
    r"""(?ix)
    ^(?:
        true|false|null|none|nil|yes|no|on|off|enabled|disabled  # config flags
        | changeme|example|placeholder|redacted|todo|xxx+|\*+     # obvious stand-ins
        | \[.*\] | <.*> | \$\{.*\} | \$\(.*\)                     # templated values
    )$
    """
)


class AssignmentSecretDetector(Detector):
    name = "assignment_secret"
    default_enabled = True

    # Key must contain a secret-ish keyword; value captured in group "value".
    # VERBOSE so the pattern can be laid out readably; the literal whitespace here
    # is therefore ignored (it is not matched against the input).
    _pattern = re.compile(
        r"""
        (?P<key>[A-Za-z0-9_.\-]*
            (?:password|passwd|pwd|secret|token|api[_-]?key|access[_-]?key
               |private[_-]?key|credential)
            [A-Za-z0-9_.\-]*)
        \s* [:=] \s*
        (?P<q>["']?)                 # optional opening quote
        (?P<value>[^\s"'\#]{6,})     # the value (>= 6 chars, no whitespace/comment)
        (?P=q)                       # matching closing quote
        """,
        re.IGNORECASE | re.MULTILINE | re.VERBOSE,
    )

    def detect(self, text: str) -> Iterator[Match]:
        for m in self._pattern.finditer(text):
            value = m.group("value")
            if _PLACEHOLDER_VALUE.match(value):
                continue
            start, end = m.span("value")
            yield Match(
                start=start,
                end=end,
                kind="assignment_secret",
                label=self._label_for(m.group("key")),
                value=value,
                confidence=0.6,
                detector=self.name,
            )

    @staticmethod
    def _label_for(key: str) -> str:
        lowered = key.lower()
        for keyword, label in _KEYWORD_LABELS:
            if keyword in lowered:
                return label
        return "Secret"


class HighEntropyStringDetector(Detector):
    """Flag long, high-entropy tokens that look like credentials.

    Off by default: without the guardrails a structural detector gives, this is the
    detector most likely to redact something innocuous (a hash, a base64 blob). Enable
    it when you value recall over precision.
    """

    name = "high_entropy_string"
    default_enabled = False

    _token = re.compile(r"[A-Za-z0-9+/=_\-]{20,}")

    def __init__(self, min_length: int = 24, min_entropy: float = 4.0) -> None:
        self.min_length = min_length
        self.min_entropy = min_entropy

    def detect(self, text: str) -> Iterator[Match]:
        for m in self._token.finditer(text):
            token = m.group(0)
            if len(token) < self.min_length:
                continue
            if shannon_entropy(token) < self.min_entropy:
                continue
            yield Match(
                start=m.start(),
                end=m.end(),
                kind="high_entropy_string",
                label="High-Entropy Secret",
                value=token,
                confidence=0.5,
                detector=self.name,
            )


def shannon_entropy(s: str) -> float:
    """Shannon entropy in bits per character. 0 for empty input."""
    if not s:
        return 0.0
    counts = Counter(s)
    length = len(s)
    return -sum((c / length) * math.log2(c / length) for c in counts.values())
