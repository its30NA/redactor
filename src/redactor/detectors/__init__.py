"""Detector package.

``default_detectors()`` is the single source of truth for which detectors ship
enabled. It returns fresh instances so callers (and tests) never share state.
New detectors are wired in here — that is the only edit needed to extend coverage.
"""

from __future__ import annotations

from redactor.detectors.base import Detector, RegexDetector
from redactor.detectors.builtins import (
    AnthropicApiKeyDetector,
    AwsAccessKeyDetector,
    BearerTokenDetector,
    GitHubTokenDetector,
    JwtDetector,
    OpenAIApiKeyDetector,
    PrivateKeyBlockDetector,
    SlackTokenDetector,
)

__all__ = ["Detector", "RegexDetector", "default_detectors"]


def default_detectors() -> list[Detector]:
    """Return a fresh list of every detector enabled by default."""
    return [
        PrivateKeyBlockDetector(),
        AnthropicApiKeyDetector(),
        OpenAIApiKeyDetector(),
        GitHubTokenDetector(),
        AwsAccessKeyDetector(),
        SlackTokenDetector(),
        JwtDetector(),
        BearerTokenDetector(),
    ]
