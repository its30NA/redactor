"""Credentials embedded in connection strings and URLs.

We redact *just the password* out of ``scheme://user:password@host/db`` rather than
the whole URL. That is deliberate: the scheme, user, host, and database name are exactly
the context a downstream AI needs to reason about a config, while the password is the
only part that is actually sensitive. (Private/internal hostnames are a separate concern
handled by a later, configurable detector.)
"""

from __future__ import annotations

import re

from redactor.detectors.base import RegexDetector

# Schemes whose URLs commonly carry inline credentials.
_DB_SCHEMES = (
    r"postgres(?:ql)?|mysql|mariadb|mongodb\+srv|mongodb|redis|rediss|"
    r"amqps?|ftp|sftp|mssql|jdbc:[a-z0-9]+"
)


class ConnectionStringPasswordDetector(RegexDetector):
    name = "connection_string_password"
    kind = "connection_string_password"
    label = "Connection String Password"
    confidence = 0.9
    group = 1
    pattern = re.compile(
        rf"(?i)\b(?:{_DB_SCHEMES})://[^\s:@/]+:([^\s@/]+)@",
    )


class UrlBasicAuthPasswordDetector(RegexDetector):
    name = "url_basic_auth_password"
    kind = "url_basic_auth_password"
    label = "URL Password"
    confidence = 0.85
    group = 1
    # Generic http(s) userinfo. Port-only URLs (host:8080/path) don't match because
    # the password class excludes '/' and a trailing '@' is required.
    pattern = re.compile(r"(?i)\bhttps?://[^\s:@/]+:([^\s@/]+)@")
