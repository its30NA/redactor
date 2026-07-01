"""Version-control platform tokens: GitHub and GitLab."""

from __future__ import annotations

import re

from redactor.detectors.base import RegexDetector


class GitHubTokenDetector(RegexDetector):
    name = "github_token"
    kind = "github_token"
    label = "GitHub Token"
    confidence = 0.99
    # Classic (ghp_/gho_/ghu_/ghs_/ghr_) plus fine-grained (github_pat_...) formats.
    pattern = re.compile(r"\b(?:gh[posur]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{22,})\b")


class GitLabTokenDetector(RegexDetector):
    name = "gitlab_token"
    kind = "gitlab_token"
    label = "GitLab Token"
    confidence = 0.98
    pattern = re.compile(r"\bglpat-[A-Za-z0-9_-]{20,}\b")
