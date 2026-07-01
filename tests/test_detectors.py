"""Per-detector tests: each detector must catch its target and nothing else.

The `should_not_match` cases are the important half — they are the beginning of the
false-positive corpus that keeps the tool trustworthy.
"""

from __future__ import annotations

import pytest

from redactor.detectors import default_detectors
from redactor.pipeline import Pipeline

PEM = (
    "-----BEGIN RSA PRIVATE KEY-----\n"
    "MIIEpAIBAAKCAQEA1c3R1cGlkZXhhbXBsZWtleWRvbnR1c2VtZQ==\n"
    "second line of base64 body here plus more\n"
    "-----END RSA PRIVATE KEY-----"
)

MATCH_CASES = [
    ("openai_api_key", "sk-abcdEFGH1234ijklMNOP5678"),
    ("openai_api_key", "sk-proj-abcdEFGH1234ijklMNOP5678qrst"),
    ("anthropic_api_key", "sk-ant-api03-abcdEFGH1234ijklMNOP5678"),
    ("github_token", "ghp_" + "A" * 36),
    ("github_token", "github_pat_" + "A" * 22 + "_" + "B" * 20),
    ("aws_access_key_id", "AKIAIOSFODNN7EXAMPLE"),
    ("slack_token", "xoxb-1234567890-abcdefghijklmno"),
    ("jwt", "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NSJ9.dBjftJeZ4CVP-mB92K27uhbUJU1p"),
    ("private_key_block", PEM),
    ("bearer_token", "Authorization: Bearer abc123DEF456ghi789"),
]

# Things that look secret-ish but must survive untouched.
NON_MATCH_TEXTS = [
    "A plain sentence with no secrets whatsoever.",
    "commit 9f8e7d6c5b4a3f2e1d0c9b8a7654321000abcdef",  # git sha
    "id = 550e8400-e29b-41d4-a716-446655440000",  # uuid
    "https://example.com/path?ok=1",
    "the word bearer appears but no token follows.",
]


@pytest.mark.parametrize(("kind", "text"), MATCH_CASES)
def test_detector_matches_expected_kind(kind: str, text: str) -> None:
    matches = Pipeline().scan(text)
    assert matches, f"expected a match in: {text!r}"
    assert any(m.kind == kind for m in matches), (
        f"expected kind {kind!r}, got {[m.kind for m in matches]}"
    )


@pytest.mark.parametrize(("kind", "text"), MATCH_CASES)
def test_value_never_leaks_into_output(kind: str, text: str) -> None:
    result = Pipeline().sanitize(text)
    for m in result.matches:
        assert m.value not in result.text, f"{kind}: raw value leaked into output"


@pytest.mark.parametrize("text", NON_MATCH_TEXTS)
def test_no_false_positives(text: str) -> None:
    result = Pipeline().sanitize(text)
    assert result.matches == []
    assert result.text == text


def test_bearer_preserves_context() -> None:
    result = Pipeline().sanitize("Authorization: Bearer sekret-token-value-123")
    assert result.text == "Authorization: Bearer [REDACTED: Bearer Token]"


def test_all_default_detectors_have_unique_names() -> None:
    names = [d.name for d in default_detectors()]
    assert len(names) == len(set(names))
