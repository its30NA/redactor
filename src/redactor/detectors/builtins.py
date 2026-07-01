"""The M0 detector suite: high-precision, structural formats plus one contextual case.

Each detector here targets a secret with a well-known *shape*, so it can match with
very high confidence and almost no false positives. Heuristic detectors (assignment
patterns, high-entropy strings) are intentionally deferred to a later milestone so
that this first suite stays trustworthy: everything it flags is almost certainly a
real credential.

Ordering note: some prefixes are ambiguous — an Anthropic key (``sk-ant-...``) also
satisfies a naive OpenAI ``sk-...`` pattern. We resolve this two ways: a negative
lookahead on the OpenAI pattern, and a higher confidence on the more specific
detector so the overlap resolver prefers it. Defense in depth keeps labels correct.
"""

from __future__ import annotations

import re

from redactor.detectors.base import RegexDetector


class AnthropicApiKeyDetector(RegexDetector):
    name = "anthropic_api_key"
    kind = "anthropic_api_key"
    label = "Anthropic API Key"
    confidence = 0.99
    pattern = re.compile(r"\bsk-ant-[A-Za-z0-9_-]{20,}\b")


class OpenAIApiKeyDetector(RegexDetector):
    name = "openai_api_key"
    kind = "openai_api_key"
    label = "OpenAI API Key"
    confidence = 0.97
    # Negative lookahead avoids stealing Anthropic keys, which share the sk- prefix.
    pattern = re.compile(r"\bsk-(?!ant-)(?:proj-)?[A-Za-z0-9_-]{20,}\b")


class GitHubTokenDetector(RegexDetector):
    name = "github_token"
    kind = "github_token"
    label = "GitHub Token"
    confidence = 0.99
    # Classic (ghp_/gho_/ghu_/ghs_/ghr_) plus fine-grained (github_pat_...) formats.
    pattern = re.compile(
        r"\b(?:gh[posur]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{22,})\b"
    )


class AwsAccessKeyDetector(RegexDetector):
    name = "aws_access_key_id"
    kind = "aws_access_key_id"
    label = "AWS Access Key ID"
    confidence = 0.99
    pattern = re.compile(r"\b(?:AKIA|ASIA|AGPA|AIDA|AROA|ANPA)[0-9A-Z]{16}\b")


class SlackTokenDetector(RegexDetector):
    name = "slack_token"
    kind = "slack_token"
    label = "Slack Token"
    confidence = 0.99
    pattern = re.compile(r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b")


class JwtDetector(RegexDetector):
    name = "jwt"
    kind = "jwt"
    label = "JSON Web Token (JWT)"
    confidence = 0.95
    # header.payload.signature — header always begins "eyJ" (base64url of '{"').
    pattern = re.compile(
        r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b"
    )


class PrivateKeyBlockDetector(RegexDetector):
    name = "private_key_block"
    kind = "private_key_block"
    label = "Private Key"
    confidence = 1.0
    # Covers PEM RSA/EC/DSA/PKCS#8 and OpenSSH private key blocks. DOTALL so the
    # body (which spans many lines) is captured whole and redacted as one unit.
    pattern = re.compile(
        r"-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----.*?-----END (?:[A-Z0-9 ]+ )?PRIVATE KEY-----",
        re.DOTALL,
    )


class BearerTokenDetector(RegexDetector):
    name = "bearer_token"
    kind = "bearer_token"
    label = "Bearer Token"
    confidence = 0.9
    # Contextual: the token is only meaningful after "Bearer". We capture *just*
    # the token (group 1) so the surrounding "Authorization: Bearer " context
    # survives into the sanitized output and stays legible to a downstream AI.
    group = 1
    pattern = re.compile(r"(?i)\bbearer\s+([A-Za-z0-9._~+/-]{8,}=*)")
