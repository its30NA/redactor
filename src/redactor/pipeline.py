"""The pipeline ties the pieces together: detect → allowlist → resolve → redact.

This is the one object most callers touch. It is deliberately small and free of
I/O so it can be unit-tested directly and reused by the CLI, a future clipboard
watcher, a pre-commit hook, or a GUI without change.
"""

from __future__ import annotations

from dataclasses import dataclass

from redactor.allowlist import Allowlist
from redactor.config import Config
from redactor.detectors import (
    Detector,
    build_detectors,
    default_detectors,
    pii_detector_names,
)
from redactor.detectors.custom import CustomRegexDetector
from redactor.models import Match
from redactor.redaction import Redactor
from redactor.resolver import resolve


@dataclass(frozen=True, slots=True)
class SanitizeResult:
    """Outcome of sanitizing a piece of text."""

    text: str
    matches: list[Match]

    @property
    def redaction_count(self) -> int:
        return len(self.matches)


class Pipeline:
    def __init__(
        self,
        detectors: list[Detector] | None = None,
        allowlist: Allowlist | None = None,
        redactor: Redactor | None = None,
    ) -> None:
        self.detectors = detectors if detectors is not None else default_detectors()
        self.allowlist = allowlist or Allowlist()
        self.redactor = redactor or Redactor()

    @classmethod
    def from_config(cls, config: Config) -> Pipeline:
        """Build a pipeline honoring a loaded :class:`~redactor.config.Config`."""
        enabled = config.enabled_detectors
        if config.redact_pii:
            enabled = enabled | pii_detector_names()
        detectors = build_detectors(disabled=config.disabled_detectors, enabled=enabled)
        # User-defined rules are always active and run alongside the built-ins.
        detectors += [
            CustomRegexDetector(
                name=rule.name,
                label=rule.label,
                pattern=rule.pattern,
                confidence=rule.confidence,
                group=rule.group,
            )
            for rule in config.custom_rules
        ]
        return cls(
            detectors=detectors,
            allowlist=Allowlist(config.allowlist_patterns),
            redactor=Redactor(
                template=config.placeholder_template,
                stable_numbering=config.stable_numbering,
            ),
        )

    def scan(self, text: str) -> list[Match]:
        """Detect sensitive spans without modifying the text."""
        found: list[Match] = []
        for detector in self.detectors:
            found.extend(detector.detect(text))
        found = self.allowlist.filter(found)
        return resolve(found)

    def sanitize(self, text: str) -> SanitizeResult:
        """Detect and redact in one call."""
        matches = self.scan(text)
        return SanitizeResult(text=self.redactor.apply(text, matches), matches=matches)
