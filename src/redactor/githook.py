"""Git integration: a pre-commit hook that blocks commits containing secrets.

The hook shells back into ``scrub check``, which scans the staged files and exits
non-zero if anything sensitive is found — turning the redactor into a safety net that
stops secrets from ever entering history in the first place.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

# {scrub} is replaced at install time with the command used to invoke scrub, so
# the hook works even when the venv is not activated in the hook environment.
HOOK_SCRIPT = """\
#!/bin/sh
# Installed by `scrub install-hook`. Blocks commits that contain secrets.
# Bypass in an emergency with `git commit --no-verify`.
{scrub} check || {{
    echo "redactor: commit blocked — sensitive data found in staged files." >&2
    echo "Review the findings above, or bypass with 'git commit --no-verify'." >&2
    exit 1
}}
"""


def resolve_scrub_command() -> str:
    """Return a shell command that reliably invokes scrub.

    Prefers the ``scrub`` entry point on PATH; falls back to the running
    interpreter's ``redactor.cli`` module so the hook keeps working inside a
    non-activated venv.
    """
    exe = shutil.which("scrub")
    if exe:
        return exe
    return f'"{sys.executable}" -m redactor.cli'


def staged_files() -> list[Path]:
    """Paths staged for commit (added/copied/modified), relative to repo root."""
    result = subprocess.run(
        ["git", "diff", "--cached", "--name-only", "--diff-filter=ACM"],
        capture_output=True,
        text=True,
        check=True,
    )
    return [Path(line) for line in result.stdout.splitlines() if line.strip()]


def _git_hooks_dir() -> Path:
    result = subprocess.run(
        ["git", "rev-parse", "--git-path", "hooks"],
        capture_output=True,
        text=True,
        check=True,
    )
    return Path(result.stdout.strip())


def install_hook(force: bool = False) -> Path:
    """Write the pre-commit hook. Returns its path. Raises if one exists (unless force)."""
    hooks = _git_hooks_dir()
    hooks.mkdir(parents=True, exist_ok=True)
    hook_path = hooks / "pre-commit"
    if hook_path.exists() and not force:
        raise FileExistsError(
            f"{hook_path} already exists; re-run with --force to overwrite"
        )
    hook_path.write_text(
        HOOK_SCRIPT.format(scrub=resolve_scrub_command()), encoding="utf-8"
    )
    hook_path.chmod(0o755)
    return hook_path
