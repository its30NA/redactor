"""SaaS provider credentials and webhook URLs.

Payment, email, comms, and package-registry tokens — plus incoming-webhook URLs,
which are themselves secrets (anyone holding one can post to your channel).
"""

from __future__ import annotations

import re

from redactor.detectors.base import RegexDetector


class SlackTokenDetector(RegexDetector):
    name = "slack_token"
    kind = "slack_token"
    label = "Slack Token"
    confidence = 0.99
    pattern = re.compile(r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b")


class SlackWebhookDetector(RegexDetector):
    name = "slack_webhook"
    kind = "slack_webhook"
    label = "Slack Webhook URL"
    confidence = 0.99
    pattern = re.compile(r"https://hooks\.slack\.com/services/[A-Za-z0-9/]+")


class DiscordWebhookDetector(RegexDetector):
    name = "discord_webhook"
    kind = "discord_webhook"
    label = "Discord Webhook URL"
    confidence = 0.99
    pattern = re.compile(
        r"https://(?:ptb\.|canary\.)?discord(?:app)?\.com/api/webhooks/\d+/[\w-]+"
    )


class StripeKeyDetector(RegexDetector):
    name = "stripe_secret_key"
    kind = "stripe_secret_key"
    label = "Stripe Secret Key"
    confidence = 0.98
    # sk_/rk_ live or test. (pk_ publishable keys are not secret and are skipped.)
    pattern = re.compile(r"\b(?:sk|rk)_(?:live|test)_[0-9A-Za-z]{16,}\b")


class SendGridKeyDetector(RegexDetector):
    name = "sendgrid_api_key"
    kind = "sendgrid_api_key"
    label = "SendGrid API Key"
    confidence = 0.99
    pattern = re.compile(r"\bSG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43}\b")


class TwilioKeyDetector(RegexDetector):
    name = "twilio_key"
    kind = "twilio_key"
    label = "Twilio API Key / Account SID"
    confidence = 0.85
    pattern = re.compile(r"\b(?:AC|SK)[0-9a-fA-F]{32}\b")


class NpmTokenDetector(RegexDetector):
    name = "npm_token"
    kind = "npm_token"
    label = "npm Access Token"
    confidence = 0.98
    pattern = re.compile(r"\bnpm_[A-Za-z0-9]{36}\b")


class PyPiTokenDetector(RegexDetector):
    name = "pypi_token"
    kind = "pypi_token"
    label = "PyPI API Token"
    confidence = 0.99
    pattern = re.compile(r"\bpypi-[A-Za-z0-9_-]{16,}\b")
