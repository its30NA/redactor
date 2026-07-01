"""Filesystem scanning.

Walks files or directories and reports (or fixes) secrets, reusing the exact same
:class:`~redactor.pipeline.Pipeline` as the CLI filter — so scanning a tree and piping a
single file give identical verdicts. Binary and oversized files are skipped: they are
not text you'd paste into an assistant, and trying to redact them wastes time and risks
corruption.
"""

from __future__ import annotations

import os
from collections.abc import Iterator
from dataclasses import dataclass, field
from pathlib import Path

from redactor.models import Match
from redactor.pipeline import Pipeline

# Directories never worth scanning — noise, dependencies, VCS internals.
_SKIP_DIRS = frozenset(
    {".git", ".hg", ".svn", "node_modules", ".venv", "venv", "__pycache__", ".mypy_cache",
     ".ruff_cache", ".pytest_cache", "dist", "build", ".idea", ".vscode"}
)
_MAX_BYTES = 5 * 1024 * 1024  # 5 MB — larger files are almost certainly not chat fodder


@dataclass(frozen=True, slots=True)
class FileReport:
    path: Path
    matches: list[Match] = field(default_factory=list)
    skipped: str | None = None  # reason, if the file was not scanned

    @property
    def has_findings(self) -> bool:
        return bool(self.matches)


def looks_binary(sample: bytes) -> bool:
    """Heuristic: a NUL byte in the first chunk means binary."""
    return b"\x00" in sample


def iter_files(paths: list[Path], recursive: bool = True) -> Iterator[Path]:
    """Yield candidate files from ``paths`` (files pass through; dirs are walked)."""
    for path in paths:
        if path.is_file():
            yield path
        elif path.is_dir():
            if not recursive:
                yield from (p for p in path.iterdir() if p.is_file())
                continue
            for root, dirs, files in os.walk(path):
                dirs[:] = [d for d in dirs if d not in _SKIP_DIRS]
                for name in files:
                    yield Path(root) / name


def scan_file(path: Path, pipeline: Pipeline) -> FileReport:
    try:
        raw = path.read_bytes()
    except OSError as exc:
        return FileReport(path, skipped=f"unreadable: {exc}")
    if len(raw) > _MAX_BYTES:
        return FileReport(path, skipped="too large")
    if looks_binary(raw[:1024]):
        return FileReport(path, skipped="binary")
    text = raw.decode("utf-8", errors="replace")
    return FileReport(path, matches=pipeline.scan(text))


def scan_paths(
    paths: list[Path], pipeline: Pipeline, recursive: bool = True
) -> Iterator[FileReport]:
    for file in iter_files(paths, recursive=recursive):
        yield scan_file(file, pipeline)


def sanitize_file(path: Path, pipeline: Pipeline) -> FileReport:
    """Rewrite ``path`` in place with sanitized content. Returns what was redacted."""
    report = scan_file(path, pipeline)
    if report.skipped or not report.has_findings:
        return report
    text = path.read_text(encoding="utf-8", errors="replace")
    path.write_text(pipeline.sanitize(text).text, encoding="utf-8")
    return report
