"""User-defined detectors built from config.

This is the escape hatch: any pattern the built-ins miss (an internal token format,
a company-specific identifier) can be added without touching code, straight from
``redactor.toml``::

    [[rules]]
    name = "acme_internal_token"
    label = "ACME Internal Token"
    pattern = "\\bACME-[A-Z0-9]{20}\\b"
    confidence = 0.95   # optional, default 0.9
    group = 0           # optional, which capture group is the secret

Custom rules are always active (a user who wrote one wants it), and being ordinary
``RegexDetector`` instances they flow through the same resolver and redactor as
everything else.
"""

from __future__ import annotations

import re

from redactor.detectors.base import RegexDetector


class CustomRegexDetector(RegexDetector):
    """A ``RegexDetector`` configured at runtime rather than at import time."""

    def __init__(
        self,
        name: str,
        label: str,
        pattern: str,
        confidence: float = 0.9,
        group: int | str = 0,
    ) -> None:
        self.name = name
        self.kind = name
        self.label = label
        self.pattern = re.compile(pattern)
        self.confidence = confidence
        self.group = group
