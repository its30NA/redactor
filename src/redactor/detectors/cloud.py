"""Cloud provider credentials: AWS, Azure, GCP.

Two flavors live here. *Prefixed* keys (AWS access key IDs, Google API keys, Google
OAuth tokens) have a distinctive shape and match on their own. *Shapeless* secrets —
an AWS secret access key or an Azure storage account key are just 40+ base64 chars —
would be indistinguishable from ordinary data, so we only flag them when they appear
next to their unmistakable label. Those contextual patterns capture group 1 (the
value) so the surrounding key name survives into the output.
"""

from __future__ import annotations

import re

from redactor.detectors.base import RegexDetector


class AwsAccessKeyDetector(RegexDetector):
    name = "aws_access_key_id"
    kind = "aws_access_key_id"
    label = "AWS Access Key ID"
    confidence = 0.99
    pattern = re.compile(r"\b(?:AKIA|ASIA|AGPA|AIDA|AROA|ANPA)[0-9A-Z]{16}\b")


class AwsSecretKeyDetector(RegexDetector):
    name = "aws_secret_access_key"
    kind = "aws_secret_access_key"
    label = "AWS Secret Access Key"
    confidence = 0.92
    group = 1
    # Contextual: a bare 40-char base64 string is meaningless, so require the label.
    pattern = re.compile(
        r"""(?ix)
        aws_secret_access_key         # the tell-tale key name
        ["']? \s* [:=] \s* ["']?      # separator: = or :, optional quotes
        ([A-Za-z0-9/+]{40})           # the 40-char secret
        """
    )


class AzureStorageKeyDetector(RegexDetector):
    name = "azure_storage_key"
    kind = "azure_storage_key"
    label = "Azure Storage Account Key"
    confidence = 0.9
    group = 1
    pattern = re.compile(r"(?i)AccountKey=([A-Za-z0-9/+]{40,}={0,2})")


class GoogleApiKeyDetector(RegexDetector):
    name = "google_api_key"
    kind = "google_api_key"
    label = "Google API Key"
    confidence = 0.98
    pattern = re.compile(r"\bAIza[0-9A-Za-z_-]{35}\b")


class GoogleOAuthTokenDetector(RegexDetector):
    name = "google_oauth_token"
    kind = "google_oauth_token"
    label = "Google OAuth Access Token"
    confidence = 0.95
    pattern = re.compile(r"\bya29\.[0-9A-Za-z_-]{20,}\b")
