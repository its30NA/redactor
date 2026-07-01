"""Allowlist — values that must never be redacted.

Real text is full of things that *look* like secrets but are safe and useful to
keep: documentation placeholders (``sk-xxxxxxxx``), example hosts (``example.com``),
well-known test keys. The allowlist lets a user (or a project config) exempt them so
the tool doesn't mangle legible context. Entries are treated as regular expressions
and tested against each match's raw value.
"""

from __future__ import annotations

import re
from collections.abc import Iterable

from redactor.models import Match


class Allowlist:
    """A set of regex patterns; a match is exempt if its value matches any of them."""

    def __init__(self, patterns: Iterable[str] = ()) -> None:
        self._patterns: list[re.Pattern[str]] = [re.compile(p) for p in patterns]

    def allows(self, match: Match) -> bool:
        """True if ``match`` should be left untouched."""
        return any(p.search(match.value) for p in self._patterns)

    def filter(self, matches: Iterable[Match]) -> list[Match]:
        """Drop every allowlisted match."""
        return [m for m in matches if not self.allows(m)]
