"""AI provider credentials: OpenAI, Anthropic, Hugging Face.

The OpenAI and Anthropic keys share the ``sk-`` prefix, so we disambiguate two ways:
a negative lookahead on the OpenAI pattern, and a higher confidence on the more
specific Anthropic detector so the overlap resolver prefers it. Defense in depth keeps
labels correct even if one guard is later loosened.
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
    pattern = re.compile(r"\bsk-(?!ant-)(?:proj-)?[A-Za-z0-9_-]{20,}\b")


class HuggingFaceTokenDetector(RegexDetector):
    name = "huggingface_token"
    kind = "huggingface_token"
    label = "Hugging Face Token"
    confidence = 0.98
    pattern = re.compile(r"\bhf_[A-Za-z0-9]{34,}\b")
