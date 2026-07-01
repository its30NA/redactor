"""Audit log — a record of *what* was redacted, safe to keep and share.

An audit trail is essential for trust: you want to see what the tool removed without
re-exposing the secrets it removed. So a record never stores the raw value. Instead it
stores a **salted fingerprint**: ``sha256(run_salt + value)`` truncated to a few bytes.

Why salted, and why per-run? A plain hash of a value would let anyone with a guessed
secret confirm it against the log (a hash oracle). A fresh random salt per run breaks
that while still letting you see, *within one run*, that two redactions share the same
underlying value (identical fingerprints). Cross-run correlation is intentionally not
possible.
"""

from __future__ import annotations

import hashlib
import json
import secrets
from collections.abc import Iterable
from dataclasses import asdict, dataclass

from redactor.models import Match

_FINGERPRINT_BYTES = 6  # 12 hex chars — enough to spot duplicates, useless as an oracle


@dataclass(frozen=True, slots=True)
class AuditRecord:
    kind: str
    label: str
    detector: str
    start: int
    end: int
    length: int
    fingerprint: str


def _fingerprint(value: str, salt: str) -> str:
    digest = hashlib.sha256((salt + value).encode("utf-8")).hexdigest()
    return digest[: _FINGERPRINT_BYTES * 2]


def build_records(matches: Iterable[Match], salt: str) -> list[AuditRecord]:
    return [
        AuditRecord(
            kind=m.kind,
            label=m.label,
            detector=m.detector,
            start=m.start,
            end=m.end,
            length=m.length,
            fingerprint=_fingerprint(m.value, salt),
        )
        for m in matches
    ]


def build_audit(matches: Iterable[Match], salt: str | None = None) -> dict:
    """Return a JSON-serializable audit document for ``matches``.

    A random ``salt`` is generated per call unless one is supplied (tests pass a
    fixed salt for determinism). The salt is included so a user could re-verify a
    known value against the log if they choose to.
    """
    salt = salt or secrets.token_hex(16)
    records = build_records(matches, salt)
    return {
        "version": 1,
        "salt": salt,
        "redaction_count": len(records),
        "records": [asdict(r) for r in records],
    }


def dumps(audit: dict) -> str:
    return json.dumps(audit, indent=2, sort_keys=False)
