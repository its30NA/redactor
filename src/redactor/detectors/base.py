"""Detector interface and a reusable regex-backed base class.

A *detector* is the unit of extensibility in this project. To add coverage for a
new secret family you write one small class and register it — nothing else in the
pipeline changes. Most detectors are pure pattern matches, so ``RegexDetector``
does the heavy lifting; only genuinely custom logic needs to subclass ``Detector``
directly.
"""

from __future__ import annotations

import re
from abc import ABC, abstractmethod
from collections.abc import Iterator

from redactor.models import Match


class Detector(ABC):
    """Base class for all detectors.

    Subclasses set ``name`` and implement :meth:`detect`. Detectors must be
    pure functions of their input text — no I/O, no shared state — which is what
    makes them cheap to test and safe to run in any order.

    ``default_enabled`` controls whether a detector ships on out of the box. High-
    precision structural detectors are ``True``; noisier heuristic ones (entropy)
    default to ``False`` and must be opted into via config.
    """

    name: str = "detector"
    default_enabled: bool = True
    # Coarse grouping used by config toggles. "secret" detectors are the core set;
    # "pii" detectors are gated behind an explicit opt-in (see Config.redact_pii).
    category: str = "secret"

    @abstractmethod
    def detect(self, text: str) -> Iterator[Match]:
        """Yield every sensitive span found in ``text``."""
        raise NotImplementedError


class RegexDetector(Detector):
    """A detector whose matches come from a single compiled regular expression.

    Attributes:
        kind: Canonical id assigned to every match (see :class:`~redactor.models.Match`).
        label: Human label used to build the placeholder.
        pattern: Compiled regex scanned with :meth:`re.Pattern.finditer`.
        confidence: Confidence assigned to every match.
        group: Which capture group holds the *secret* span. Defaults to ``0``
            (the whole match). Use a named or numbered group when the secret is a
            substring of a larger, context-carrying pattern — e.g. capturing only
            the token in ``Authorization: Bearer <token>``.
    """

    kind: str = "secret"
    label: str = "Secret"
    pattern: re.Pattern[str]
    confidence: float = 0.99
    group: int | str = 0

    def detect(self, text: str) -> Iterator[Match]:
        for m in self.pattern.finditer(text):
            start, end = m.span(self.group)
            if start < 0:  # optional group did not participate
                continue
            yield Match(
                start=start,
                end=end,
                kind=self.kind,
                label=self.label,
                value=m.group(self.group),
                confidence=self.confidence,
                detector=self.name,
            )
