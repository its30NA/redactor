"""HTTP-layer secrets: auth headers, cookies, session identifiers.

All of these are *contextual* — the secret is only meaningful next to its header name
or key. Each pattern captures group 1 (the value alone) so the surrounding context
(``Authorization: Bearer ``, ``Cookie: ``) stays legible in the sanitized output.
"""

from __future__ import annotations

import re

from redactor.detectors.base import RegexDetector


class BearerTokenDetector(RegexDetector):
    name = "bearer_token"
    kind = "bearer_token"
    label = "Bearer Token"
    confidence = 0.9
    group = 1
    pattern = re.compile(r"(?i)\bbearer\s+([A-Za-z0-9._~+/-]{8,}=*)")


class BasicAuthDetector(RegexDetector):
    name = "basic_auth"
    kind = "basic_auth"
    label = "HTTP Basic Credentials"
    confidence = 0.9
    group = 1
    pattern = re.compile(r"(?i)\bauthorization:\s*basic\s+([A-Za-z0-9+/=]{8,})")


class CookieHeaderDetector(RegexDetector):
    name = "cookie_header"
    kind = "cookie_header"
    label = "HTTP Cookie"
    confidence = 0.8
    group = 1
    # Redact the whole cookie header value — individual cookies can be session
    # tokens, and parsing them apart safely is not worth the false-negative risk.
    pattern = re.compile(r"(?im)^(?:set-)?cookie:\s*(.+)$")


class SessionIdDetector(RegexDetector):
    name = "session_id"
    kind = "session_id"
    label = "Session Identifier"
    confidence = 0.75
    group = 1
    pattern = re.compile(
        r"(?i)\b(?:phpsessid|jsessionid|session_id|sessionid|sessid|sid)\s*[=:]\s*"
        r"[\"']?([A-Za-z0-9%._+-]{8,})"
    )
