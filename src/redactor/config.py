"""Configuration.

Config is optional — the tool works with sensible defaults and zero setup. When a
config file is present it can disable specific detectors, extend the allowlist, and
tweak the placeholder template. We use TOML via the stdlib ``tomllib`` (Python 3.11+)
so there is no third-party dependency and the format is friendly to hand-edit.

Example ``redactor.toml``::

    disabled_detectors = ["bearer_token"]
    placeholder_template = "<<{label}{suffix}>>"

    [allowlist]
    patterns = ["example\\.com", "sk-xxxx"]
"""

from __future__ import annotations

import tomllib
from dataclasses import dataclass
from pathlib import Path

from redactor.redaction import DEFAULT_TEMPLATE

# Filenames searched, in order, when no explicit config path is given.
CONFIG_FILENAMES = ("redactor.toml", ".redactor.toml")


@dataclass(frozen=True, slots=True)
class Config:
    disabled_detectors: frozenset[str] = frozenset()
    enabled_detectors: frozenset[str] = frozenset()
    allowlist_patterns: tuple[str, ...] = ()
    placeholder_template: str = DEFAULT_TEMPLATE
    stable_numbering: bool = True

    @classmethod
    def from_dict(cls, data: dict) -> Config:
        allowlist = data.get("allowlist", {})
        return cls(
            disabled_detectors=frozenset(data.get("disabled_detectors", [])),
            enabled_detectors=frozenset(data.get("enabled_detectors", [])),
            allowlist_patterns=tuple(allowlist.get("patterns", [])),
            placeholder_template=data.get("placeholder_template", DEFAULT_TEMPLATE),
            stable_numbering=bool(data.get("stable_numbering", True)),
        )

    @classmethod
    def load(cls, path: str | Path | None = None) -> Config:
        """Load config from ``path``, or discover one nearby, or return defaults."""
        resolved = Path(path) if path else _discover()
        if resolved is None:
            return cls()
        with resolved.open("rb") as fh:
            return cls.from_dict(tomllib.load(fh))


def _discover(start: Path | None = None) -> Path | None:
    """Walk up from ``start`` (cwd by default) looking for a config file."""
    directory = (start or Path.cwd()).resolve()
    for parent in (directory, *directory.parents):
        for name in CONFIG_FILENAMES:
            candidate = parent / name
            if candidate.is_file():
                return candidate
    return None
