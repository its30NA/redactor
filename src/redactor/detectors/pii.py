"""Personally identifiable information (PII).

PII is fundamentally different from credentials: an email address or IP is often
*legitimately* part of the text you want an AI to reason about, and over-redacting it
destroys meaning. So the whole group ships **disabled** and is enabled deliberately via
``redact_pii = true`` (or by naming individual detectors in ``enabled_detectors``).

Every detector here sets ``category = "pii"`` so a single toggle can flip the group.
"""

from __future__ import annotations

import re
from collections.abc import Iterator

from redactor.detectors.base import RegexDetector
from redactor.models import Match


class _PiiRegexDetector(RegexDetector):
    """Common base: PII, off by default."""

    category = "pii"
    default_enabled = False


class EmailDetector(_PiiRegexDetector):
    name = "email"
    kind = "email"
    label = "Email Address"
    confidence = 0.95
    pattern = re.compile(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b")


class IPv4Detector(_PiiRegexDetector):
    name = "ipv4_address"
    kind = "ipv4_address"
    label = "IPv4 Address"
    confidence = 0.8
    _octet = r"(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)"
    pattern = re.compile(rf"\b(?:{_octet}\.){{3}}{_octet}\b")


class UsSsnDetector(_PiiRegexDetector):
    name = "us_ssn"
    kind = "us_ssn"
    label = "US Social Security Number"
    confidence = 0.7
    pattern = re.compile(r"\b\d{3}-\d{2}-\d{4}\b")


class PhoneNumberDetector(_PiiRegexDetector):
    name = "phone_number"
    kind = "phone_number"
    label = "Phone Number"
    confidence = 0.6
    # Deliberately strict to limit false positives: require either a leading '+'
    # country code or a parenthesized area code, plus separated groups.
    pattern = re.compile(
        r"""(?x)
        (?<!\w)
        (?:
            \+\d{1,3}[\s.-]?\(?\d{1,4}\)?[\s.-]?\d{3,4}[\s.-]?\d{3,4}  # +49 30 1234 5678
            | \(\d{2,4}\)[\s.-]?\d{3,4}[\s.-]?\d{3,4}                  # (030) 1234 5678
        )
        (?!\w)
        """
    )


class CreditCardDetector(_PiiRegexDetector):
    """Digit runs of 13-19 that pass the Luhn checksum.

    The Luhn check is what makes this trustworthy: random 16-digit numbers almost
    never validate, so this rarely fires on non-card data despite the loose shape.
    """

    name = "credit_card"
    kind = "credit_card"
    label = "Credit Card Number"
    confidence = 0.85
    _candidate = re.compile(r"\b(?:\d[ -]?){13,19}\b")

    def detect(self, text: str) -> Iterator[Match]:
        for m in self._candidate.finditer(text):
            digits = re.sub(r"[ -]", "", m.group(0))
            if 13 <= len(digits) <= 19 and _luhn_ok(digits):
                yield Match(
                    start=m.start(),
                    end=m.end(),
                    kind=self.kind,
                    label=self.label,
                    value=m.group(0),
                    confidence=self.confidence,
                    detector=self.name,
                )


def _luhn_ok(digits: str) -> bool:
    total = 0
    for i, ch in enumerate(reversed(digits)):
        d = ord(ch) - 48
        if i % 2 == 1:
            d *= 2
            if d > 9:
                d -= 9
        total += d
    return total % 10 == 0
