"""M3: PII detection group, custom rules, and the --preview report."""

from __future__ import annotations

import pytest

from redactor.cli import _preview
from redactor.config import Config, CustomRule
from redactor.detectors import pii_detector_names
from redactor.pipeline import Pipeline

# --- PII group --------------------------------------------------------------


def test_pii_is_off_by_default() -> None:
    text = "contact jane.doe@example.com or 192.168.1.1"
    assert Pipeline().sanitize(text).matches == []


def test_redact_pii_toggle_enables_group() -> None:
    pipe = Pipeline.from_config(Config(redact_pii=True))
    result = pipe.sanitize("email jane.doe@corp.io from 10.0.0.5")
    kinds = {m.kind for m in result.matches}
    assert "email" in kinds
    assert "ipv4_address" in kinds


@pytest.mark.parametrize(
    ("text", "kind"),
    [
        ("jane.doe@example.com", "email"),
        ("192.168.100.42", "ipv4_address"),
        ("123-45-6789", "us_ssn"),
        ("+49 30 1234 5678", "phone_number"),
        ("4111 1111 1111 1111", "credit_card"),  # valid Luhn test card
    ],
)
def test_individual_pii_detectors(text: str, kind: str) -> None:
    pipe = Pipeline.from_config(Config(redact_pii=True))
    result = pipe.sanitize(text)
    assert any(m.kind == kind for m in result.matches), (
        f"expected {kind} in {[m.kind for m in result.matches]}"
    )


def test_credit_card_rejects_non_luhn() -> None:
    pipe = Pipeline.from_config(Config(redact_pii=True))
    # 16 digits that fail the Luhn checksum -> not a card.
    assert not any(
        m.kind == "credit_card" for m in pipe.sanitize("1234 5678 9012 3456").matches
    )


def test_pii_names_helper() -> None:
    names = pii_detector_names()
    assert {"email", "ipv4_address", "us_ssn", "phone_number", "credit_card"} <= names


# --- Custom rules -----------------------------------------------------------


def test_custom_rule_detects_and_labels() -> None:
    rule = CustomRule(
        name="acme_token",
        label="ACME Internal Token",
        pattern=r"\bACME-[A-Z0-9]{10}\b",
    )
    pipe = Pipeline.from_config(Config(custom_rules=(rule,)))
    result = pipe.sanitize("token=ACME-ABCD123456 end")
    assert result.text == "token=[REDACTED: ACME Internal Token] end"


def test_custom_rule_group_capture() -> None:
    rule = CustomRule(
        name="secret_header",
        label="Custom Secret",
        pattern=r"X-Secret:\s*(\S+)",
        group=1,
    )
    pipe = Pipeline.from_config(Config(custom_rules=(rule,)))
    result = pipe.sanitize("X-Secret: abc123xyz")
    assert result.text == "X-Secret: [REDACTED: Custom Secret]"


# --- Preview ----------------------------------------------------------------


def test_preview_reports_line_and_label() -> None:
    text = "line one\nOPENAI_API_KEY=sk-abcdEFGH1234ijklMNOP5678\nline three"
    result = Pipeline().sanitize(text)
    report = _preview(text, result)
    assert "Ln 2" in report
    assert "OpenAI API Key" in report
    # The masked context shows the placeholder, never the raw secret.
    assert "sk-abcdEFGH1234ijklMNOP5678" not in report


def test_preview_empty() -> None:
    result = Pipeline().sanitize("nothing to see here")
    assert _preview("nothing to see here", result) == "No sensitive data detected."
