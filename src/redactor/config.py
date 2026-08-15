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

import re
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

from redactor.redaction import DEFAULT_TEMPLATE


class ConfigError(ValueError):
    """Raised when a config file is present but unusable (bad TOML, bad rule)."""


@dataclass(frozen=True, slots=True)
class CustomRule:
    """A user-defined detector declared in config (see ``[[rules]]``)."""

    name: str
    label: str
    pattern: str
    confidence: float = 0.9
    group: int = 0

    def __post_init__(self) -> None:
        if not self.name.strip():
            raise ConfigError("custom rule is missing a non-empty `name`")
        if not self.label.strip():
            raise ConfigError(f"custom rule {self.name!r} is missing a non-empty `label`")
        if not 0.0 <= self.confidence <= 1.0:
            raise ConfigError(
                f"custom rule {self.name!r}: confidence must be in [0.0, 1.0], "
                f"got {self.confidence}"
            )
        if self.group < 0:
            raise ConfigError(f"custom rule {self.name!r}: group must be >= 0")
        try:
            re.compile(self.pattern)
        except re.error as exc:
            raise ConfigError(
                f"custom rule {self.name!r}: invalid regex {self.pattern!r}: {exc}"
            ) from exc

    @classmethod
    def from_dict(cls, raw: dict) -> CustomRule:
        try:
            return cls(
                name=raw["name"],
                label=raw["label"],
                pattern=raw["pattern"],
                confidence=float(raw.get("confidence", 0.9)),
                group=int(raw.get("group", 0)),
            )
        except KeyError as exc:
            raise ConfigError(
                f"custom rule is missing required key {exc.args[0]!r}; "
                "each [[rules]] entry needs `name`, `label`, and `pattern`"
            ) from exc
        except (TypeError, ValueError) as exc:
            raise ConfigError(
                f"custom rule {raw.get('name', '<unnamed>')!r}: bad value: {exc}"
            ) from exc

# Filenames searched, in order, when no explicit config path is given.
CONFIG_FILENAMES = ("redactor.toml", ".redactor.toml")


@dataclass(frozen=True, slots=True)
class LlmConfig:
    """Optional local-LLM pass. Off unless ``enabled`` is set; local host only."""

    enabled: bool = False
    model: str = "qwen2.5:3b"
    host: str = "http://127.0.0.1:11434"
    timeout: float = 30.0


@dataclass(frozen=True, slots=True)
class Config:
    disabled_detectors: frozenset[str] = frozenset()
    enabled_detectors: frozenset[str] = frozenset()
    redact_pii: bool = False
    custom_rules: tuple[CustomRule, ...] = field(default_factory=tuple)
    allowlist_patterns: tuple[str, ...] = ()
    placeholder_template: str = DEFAULT_TEMPLATE
    stable_numbering: bool = True
    llm: LlmConfig = field(default_factory=LlmConfig)

    @classmethod
    def from_dict(cls, data: dict) -> Config:
        allowlist = data.get("allowlist", {})
        llm_data = data.get("llm", {})
        rules = tuple(CustomRule.from_dict(r) for r in data.get("rules", []))
        return cls(
            disabled_detectors=frozenset(data.get("disabled_detectors", [])),
            enabled_detectors=frozenset(data.get("enabled_detectors", [])),
            redact_pii=bool(data.get("redact_pii", False)),
            custom_rules=rules,
            allowlist_patterns=tuple(allowlist.get("patterns", [])),
            placeholder_template=data.get("placeholder_template", DEFAULT_TEMPLATE),
            stable_numbering=bool(data.get("stable_numbering", True)),
            llm=LlmConfig(
                enabled=bool(llm_data.get("enabled", False)),
                model=llm_data.get("model", "qwen2.5:3b"),
                host=llm_data.get("host", "http://127.0.0.1:11434"),
                timeout=float(llm_data.get("timeout", 30.0)),
            ),
        )

    @classmethod
    def load(cls, path: str | Path | None = None) -> Config:
        """Load config from ``path``, or discover one nearby, or return defaults."""
        resolved = Path(path) if path else _discover()
        if resolved is None:
            return cls()
        try:
            with resolved.open("rb") as fh:
                data = tomllib.load(fh)
        except tomllib.TOMLDecodeError as exc:
            raise ConfigError(f"{resolved}: invalid TOML: {exc}") from exc
        except OSError as exc:
            raise ConfigError(f"{resolved}: cannot read config: {exc}") from exc
        return cls.from_dict(data)


def _discover(start: Path | None = None) -> Path | None:
    """Walk up from ``start`` (cwd by default) looking for a config file."""
    directory = (start or Path.cwd()).resolve()
    for parent in (directory, *directory.parents):
        for name in CONFIG_FILENAMES:
            candidate = parent / name
            if candidate.is_file():
                return candidate
    return None
