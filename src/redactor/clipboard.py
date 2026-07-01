"""Clipboard access, best-effort across platforms.

Clipboard tooling is environment-specific (pbcopy on macOS, wl-copy / xclip on Linux,
PowerShell on WSL/Windows). We probe for whatever is available rather than hard-depend
on any one, and surface a clear error if none is. The read/write pair is deliberately
thin so the interesting logic (sanitization) stays in the pipeline and stays testable.
"""

from __future__ import annotations

import shutil
import subprocess
from dataclasses import dataclass


class ClipboardUnavailable(RuntimeError):
    """No usable clipboard backend was found on this system."""


@dataclass(frozen=True)
class ClipboardBackend:
    name: str
    paste_cmd: list[str]
    copy_cmd: list[str]


# Ordered by preference; first whose primary binary exists wins.
_BACKENDS: tuple[ClipboardBackend, ...] = (
    ClipboardBackend("pbcopy", ["pbpaste"], ["pbcopy"]),
    ClipboardBackend("wl-clipboard", ["wl-paste", "--no-newline"], ["wl-copy"]),
    ClipboardBackend("xclip", ["xclip", "-selection", "clipboard", "-o"],
                     ["xclip", "-selection", "clipboard"]),
    ClipboardBackend("xsel", ["xsel", "--clipboard", "--output"],
                     ["xsel", "--clipboard", "--input"]),
    ClipboardBackend(
        "powershell",  # WSL / Windows
        ["powershell.exe", "-NoProfile", "-Command", "Get-Clipboard"],
        ["clip.exe"],
    ),
)


def detect_backend() -> ClipboardBackend:
    for backend in _BACKENDS:
        if shutil.which(backend.paste_cmd[0]) or shutil.which(backend.copy_cmd[0]):
            return backend
    raise ClipboardUnavailable(
        "no clipboard tool found (tried pbcopy, wl-clipboard, xclip, xsel, powershell)"
    )


def paste(backend: ClipboardBackend | None = None) -> str:
    backend = backend or detect_backend()
    try:
        result = subprocess.run(
            backend.paste_cmd, capture_output=True, text=True, check=True
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        raise ClipboardUnavailable(f"clipboard read failed ({backend.name}): {exc}") from exc
    return result.stdout


def copy(text: str, backend: ClipboardBackend | None = None) -> None:
    backend = backend or detect_backend()
    try:
        subprocess.run(backend.copy_cmd, input=text, text=True, check=True)
    except (OSError, subprocess.CalledProcessError) as exc:
        raise ClipboardUnavailable(f"clipboard write failed ({backend.name}): {exc}") from exc
