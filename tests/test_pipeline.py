"""Pipeline, resolver, redaction, and config behavior."""

from __future__ import annotations

from redactor.allowlist import Allowlist
from redactor.config import Config
from redactor.models import Match
from redactor.pipeline import Pipeline
from redactor.redaction import Redactor
from redactor.resolver import resolve


def test_end_to_end_env_file() -> None:
    text = "OPENAI_API_KEY=sk-abcdEFGH1234ijklMNOP5678\nDEBUG=true"
    result = Pipeline().sanitize(text)
    assert result.text == "OPENAI_API_KEY=[REDACTED: OpenAI API Key]\nDEBUG=true"
    assert result.redaction_count == 1


def test_anthropic_wins_over_openai_on_shared_prefix() -> None:
    # Both patterns can fire on an sk-ant- key; the resolver must keep Anthropic.
    result = Pipeline().sanitize("key=sk-ant-api03-abcdEFGH1234ijklMNOP5678")
    assert result.matches[0].kind == "anthropic_api_key"
    assert result.text == "key=[REDACTED: Anthropic API Key]"


def test_stable_numbering_distinguishes_distinct_values() -> None:
    a = "sk-aaaaAAAA1111bbbbBBBB2222"
    b = "sk-ccccCCCC3333ddddDDDD4444"
    text = f"one={a} two={b} again={a}"
    result = Pipeline().sanitize(text)
    # Same value reuses the same number; the two distinct keys differ.
    assert result.text == (
        "one=[REDACTED: OpenAI API Key #1] "
        "two=[REDACTED: OpenAI API Key #2] "
        "again=[REDACTED: OpenAI API Key #1]"
    )


def test_single_occurrence_has_no_number() -> None:
    result = Pipeline().sanitize("k=sk-aaaaAAAA1111bbbbBBBB2222")
    assert result.text == "k=[REDACTED: OpenAI API Key]"


def test_resolver_merges_overlaps_to_cover_union() -> None:
    # Overlapping spans must merge to cover their union — never drop one and expose
    # the uncovered remainder. Label/kind come from the higher-confidence match.
    low = Match(0, 10, "a", "A", "0123456789", 0.5, "d1")
    high = Match(5, 15, "b", "B", "56789abcde", 0.9, "d2")
    kept = resolve([low, high])
    assert len(kept) == 1
    assert (kept[0].start, kept[0].end) == (0, 15)  # full union covered
    assert kept[0].label == "B"  # higher-confidence contributor wins the label


def test_resolver_leaves_disjoint_matches_untouched() -> None:
    a = Match(0, 4, "a", "A", "aaaa", 0.9, "d1")
    b = Match(10, 14, "b", "B", "bbbb", 0.9, "d2")
    assert resolve([b, a]) == [a, b]


def test_allowlist_exempts_matches() -> None:
    allow = Allowlist([r"sk-xxxx"])
    pipe = Pipeline(allowlist=allow)
    text = "example key sk-xxxxAAAA1111bbbbBBBB2222"
    assert pipe.sanitize(text).matches == []


def test_config_disables_detector() -> None:
    cfg = Config(disabled_detectors=frozenset({"bearer_token"}))
    pipe = Pipeline.from_config(cfg)
    result = pipe.sanitize("Authorization: Bearer abc123DEF456ghi789")
    assert result.matches == []


def test_email_password_overlap_never_leaks_password() -> None:
    # Regression: an email-shaped password in a connection string must not leak its
    # non-email prefix when PII detection is on and the email match overlaps.
    from redactor.config import Config

    pipe = Pipeline.from_config(Config(redact_pii=True))
    result = pipe.sanitize("DATABASE_URL=postgres://user:S3cr3t!Pass@db.corp.io:5432/app")
    assert "S3cr3t" not in result.text  # the password prefix must be gone
    assert "Pass@db.corp.io" not in result.text


def test_config_custom_template() -> None:
    redactor = Redactor(template="<<{label}{suffix}>>")
    key = "sk-abcdEFGH1234ijklMNOP5678"
    matches = [Match(0, len(key), "openai_api_key", "OpenAI API Key", key, 0.97, "d")]
    out = redactor.apply(key, matches)
    assert out == "<<OpenAI API Key>>"
