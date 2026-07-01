"""Heuristic detectors, audit log, and detector enable/disable resolution."""

from __future__ import annotations

import pytest

from redactor.audit import build_audit
from redactor.config import Config
from redactor.detectors import build_detectors, default_detectors
from redactor.detectors.heuristics import shannon_entropy
from redactor.pipeline import Pipeline

# --- Assignment detector (on by default) -----------------------------------

def test_assignment_catches_custom_named_secret() -> None:
    # No vendor prefix — only the key name marks this as sensitive.
    result = Pipeline().sanitize("MYAPP_DB_PASSWORD=hunter2horse")
    assert result.text == "MYAPP_DB_PASSWORD=[REDACTED: Password]"


def test_assignment_labels_by_keyword() -> None:
    result = Pipeline().sanitize("SERVICE_API_KEY=abcdef123456")
    assert result.matches[0].label == "API Key"


@pytest.mark.parametrize(
    "text",
    [
        "DEBUG_PASSWORD=changeme",       # obvious placeholder
        "FEATURE_SECRET=true",           # config flag
        "AUTH_TOKEN=${VAULT_TOKEN}",     # templated reference
        "API_KEY=<your-key-here>",       # angle-bracket stand-in
    ],
)
def test_assignment_skips_placeholders(text: str) -> None:
    assert Pipeline().sanitize(text).matches == []


def test_assignment_does_not_fire_on_innocent_keys() -> None:
    assert Pipeline().sanitize("USERNAME=alice\nHOSTNAME=web01").matches == []


# --- Entropy detector (off by default) -------------------------------------

def test_entropy_detector_is_off_by_default() -> None:
    high = "aGVsbG8td29ybGQtdGhpcy1pcy1yYW5kb20xMjM0NТ"
    assert Pipeline().sanitize(f"blob {high}").matches == []


def test_entropy_detector_can_be_enabled() -> None:
    cfg = Config(enabled_detectors=frozenset({"high_entropy_string"}))
    pipe = Pipeline.from_config(cfg)
    token = "Xy9Kq2Lm7Pw4Rs8Tv1Zb3Nc6Df0Gh5Jk"  # long, mixed, high entropy
    result = pipe.sanitize(f"opaque {token}")
    assert any(m.kind == "high_entropy_string" for m in result.matches)


def test_shannon_entropy_ranges() -> None:
    assert shannon_entropy("") == 0.0
    assert shannon_entropy("aaaaaaaa") == 0.0
    assert shannon_entropy("abcdefgh") > 2.9  # 8 distinct chars -> 3 bits


# --- Detector set resolution -----------------------------------------------

def test_default_excludes_entropy_but_includes_assignment() -> None:
    names = {d.name for d in default_detectors()}
    assert "assignment_secret" in names
    assert "high_entropy_string" not in names


def test_build_detectors_respects_disabled_over_enabled() -> None:
    # Disabled always wins, even if also listed as enabled.
    active = build_detectors(
        disabled=frozenset({"high_entropy_string"}),
        enabled=frozenset({"high_entropy_string"}),
    )
    assert all(d.name != "high_entropy_string" for d in active)


# --- Audit log --------------------------------------------------------------

def test_audit_never_contains_raw_value() -> None:
    text = "OPENAI_API_KEY=sk-abcdEFGH1234ijklMNOP5678"
    result = Pipeline().sanitize(text)
    audit = build_audit(result.matches, salt="fixed-test-salt")
    blob = str(audit)
    assert "sk-abcdEFGH1234ijklMNOP5678" not in blob
    assert audit["redaction_count"] == 1
    assert audit["records"][0]["kind"] == "openai_api_key"
    assert len(audit["records"][0]["fingerprint"]) == 12


def test_audit_same_value_same_fingerprint() -> None:
    key = "sk-aaaaAAAA1111bbbbBBBB2222"
    result = Pipeline().sanitize(f"a={key} b={key}")
    audit = build_audit(result.matches, salt="s")
    fps = {r["fingerprint"] for r in audit["records"]}
    assert len(audit["records"]) == 2
    assert len(fps) == 1  # identical values -> identical fingerprints
