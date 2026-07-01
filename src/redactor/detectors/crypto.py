"""Cryptographic material: private keys and JWTs.

These are the highest-confidence detections in the whole tool — a PEM/OpenSSH
private-key block is unmistakable, and a JWT's ``eyJ`` header is a base64url-encoded
``{"`` that essentially never occurs by accident.
"""

from __future__ import annotations

import re

from redactor.detectors.base import RegexDetector


class PrivateKeyBlockDetector(RegexDetector):
    name = "private_key_block"
    kind = "private_key_block"
    label = "Private Key"
    confidence = 1.0
    # Covers PEM RSA/EC/DSA/PKCS#8 and OpenSSH private key blocks. DOTALL so the
    # multi-line body is captured whole and redacted as a single unit.
    pattern = re.compile(
        r"-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----.*?-----END (?:[A-Z0-9 ]+ )?PRIVATE KEY-----",
        re.DOTALL,
    )


class JwtDetector(RegexDetector):
    name = "jwt"
    kind = "jwt"
    label = "JSON Web Token (JWT)"
    confidence = 0.95
    # header.payload.signature — header always begins "eyJ" (base64url of '{"').
    pattern = re.compile(r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b")
