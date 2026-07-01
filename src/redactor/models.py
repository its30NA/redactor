"""Core data types shared across the pipeline.

The single most important privacy invariant in the whole project lives here:
a ``Match`` holds the raw secret ``value`` **only in memory** so we can audit and
de-duplicate it. That value is *never* written to the sanitized output — the
redaction engine replaces the span with a placeholder built from ``label`` alone.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class Match:
    """A single detected sensitive span within a piece of text.

    Attributes:
        start: Inclusive start offset into the source text.
        end: Exclusive end offset into the source text.
        kind: Canonical machine identifier for the secret family
            (e.g. ``"openai_api_key"``). Stable — safe to key config off.
        label: Human-readable description used to build the placeholder
            (e.g. ``"OpenAI API Key"``).
        value: The raw matched substring. In-memory only; never emitted.
        confidence: Detector confidence in ``[0.0, 1.0]``. Used by the resolver
            to arbitrate overlapping matches.
        detector: Name of the detector that produced this match (for auditing).
    """

    start: int
    end: int
    kind: str
    label: str
    value: str
    confidence: float
    detector: str

    def __post_init__(self) -> None:
        if self.start < 0 or self.end < self.start:
            raise ValueError(f"invalid span: ({self.start}, {self.end})")
        if not 0.0 <= self.confidence <= 1.0:
            raise ValueError(f"confidence out of range: {self.confidence}")

    @property
    def length(self) -> int:
        return self.end - self.start

    def overlaps(self, other: Match) -> bool:
        """True if this span shares any character with ``other``."""
        return self.start < other.end and other.start < self.end
