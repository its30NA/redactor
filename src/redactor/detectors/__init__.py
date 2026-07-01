"""Detector package.

``default_detectors()`` is the single source of truth for which detectors ship
enabled. It returns fresh instances so callers (and tests) never share state. To add
coverage, drop your detector into the relevant themed module (``crypto``, ``cloud``,
``saas``, …) and list it here — that is the only wiring step.

Detectors are grouped by domain for maintainability, not by behavior; the pipeline
treats them all identically.
"""

from __future__ import annotations

from redactor.detectors.ai_providers import (
    AnthropicApiKeyDetector,
    HuggingFaceTokenDetector,
    OpenAIApiKeyDetector,
)
from redactor.detectors.base import Detector, RegexDetector
from redactor.detectors.cloud import (
    AwsAccessKeyDetector,
    AwsSecretKeyDetector,
    AzureStorageKeyDetector,
    GoogleApiKeyDetector,
    GoogleOAuthTokenDetector,
)
from redactor.detectors.connection import (
    ConnectionStringPasswordDetector,
    UrlBasicAuthPasswordDetector,
)
from redactor.detectors.crypto import JwtDetector, PrivateKeyBlockDetector
from redactor.detectors.saas import (
    DiscordWebhookDetector,
    NpmTokenDetector,
    PyPiTokenDetector,
    SendGridKeyDetector,
    SlackTokenDetector,
    SlackWebhookDetector,
    StripeKeyDetector,
    TwilioKeyDetector,
)
from redactor.detectors.vcs import GitHubTokenDetector, GitLabTokenDetector
from redactor.detectors.web import (
    BasicAuthDetector,
    BearerTokenDetector,
    CookieHeaderDetector,
    SessionIdDetector,
)

__all__ = ["Detector", "RegexDetector", "default_detectors"]

# Detector classes enabled by default. Order is irrelevant to correctness — the
# resolver arbitrates overlaps by confidence, not by position — but we group them
# by domain for readability.
_DEFAULT_DETECTOR_CLASSES: tuple[type[Detector], ...] = (
    # Cryptographic material
    PrivateKeyBlockDetector,
    JwtDetector,
    # AI providers
    AnthropicApiKeyDetector,
    OpenAIApiKeyDetector,
    HuggingFaceTokenDetector,
    # Version control
    GitHubTokenDetector,
    GitLabTokenDetector,
    # Cloud providers
    AwsAccessKeyDetector,
    AwsSecretKeyDetector,
    AzureStorageKeyDetector,
    GoogleApiKeyDetector,
    GoogleOAuthTokenDetector,
    # SaaS
    SlackTokenDetector,
    SlackWebhookDetector,
    DiscordWebhookDetector,
    StripeKeyDetector,
    SendGridKeyDetector,
    TwilioKeyDetector,
    NpmTokenDetector,
    PyPiTokenDetector,
    # HTTP layer
    BearerTokenDetector,
    BasicAuthDetector,
    CookieHeaderDetector,
    SessionIdDetector,
    # Connection strings
    ConnectionStringPasswordDetector,
    UrlBasicAuthPasswordDetector,
)


def default_detectors() -> list[Detector]:
    """Return a fresh list of every detector enabled by default."""
    return [cls() for cls in _DEFAULT_DETECTOR_CLASSES]
