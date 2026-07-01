"""M5: filesystem scanning, git hook, and CLI subcommand dispatch."""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest

from redactor.cli import main
from redactor.githook import HOOK_SCRIPT, install_hook
from redactor.pipeline import Pipeline
from redactor.scanner import iter_files, looks_binary, sanitize_file, scan_file, scan_paths

# --- Scanner ----------------------------------------------------------------


def test_scan_file_finds_secret(tmp_path: Path) -> None:
    f = tmp_path / "config.env"
    f.write_text("OPENAI_API_KEY=sk-abcdEFGH1234ijklMNOP5678\n")
    report = scan_file(f, Pipeline())
    assert report.has_findings
    assert report.matches[0].kind == "openai_api_key"


def test_scan_skips_binary(tmp_path: Path) -> None:
    f = tmp_path / "blob.bin"
    f.write_bytes(b"\x00\x01\x02sk-abcdEFGH1234ijklMNOP5678")
    report = scan_file(f, Pipeline())
    assert report.skipped == "binary"
    assert not report.has_findings


def test_looks_binary() -> None:
    assert looks_binary(b"text\x00more")
    assert not looks_binary(b"just text")


def test_iter_files_skips_noise_dirs(tmp_path: Path) -> None:
    (tmp_path / "src").mkdir()
    (tmp_path / "src" / "a.py").write_text("x = 1")
    (tmp_path / ".git").mkdir()
    (tmp_path / ".git" / "config").write_text("secret")
    found = {p.name for p in iter_files([tmp_path])}
    assert "a.py" in found
    assert "config" not in found  # .git was pruned


def test_scan_paths_and_sanitize_file(tmp_path: Path) -> None:
    f = tmp_path / "app.log"
    f.write_text("token: Bearer abc123DEF456ghi789\nplain line\n")
    reports = [r for r in scan_paths([tmp_path], Pipeline()) if r.has_findings]
    assert len(reports) == 1

    sanitize_file(f, Pipeline())
    rewritten = f.read_text()
    assert "abc123DEF456ghi789" not in rewritten
    assert "[REDACTED: Bearer Token]" in rewritten
    assert "plain line" in rewritten


# --- CLI dispatch -----------------------------------------------------------


def test_cli_defaults_to_sanitize(capsys, monkeypatch) -> None:
    monkeypatch.setattr("sys.stdin", _FakeStdin("KEY=sk-abcdEFGH1234ijklMNOP5678"))
    rc = main([])  # no args -> sanitize stdin
    out = capsys.readouterr().out
    assert rc == 0
    assert "[REDACTED: OpenAI API Key]" in out


def test_cli_scan_exit_code(tmp_path: Path, capsys) -> None:
    (tmp_path / "s.env").write_text("GITHUB_TOKEN=ghp_" + "A" * 36)
    rc = main(["scan", str(tmp_path)])
    assert rc == 1  # findings -> non-zero for CI
    assert "finding(s)" in capsys.readouterr().out


def test_cli_scan_clean_tree(tmp_path: Path) -> None:
    (tmp_path / "ok.txt").write_text("nothing secret here\n")
    assert main(["scan", str(tmp_path)]) == 0


# --- Git hook ---------------------------------------------------------------


def test_install_hook(tmp_path: Path) -> None:
    subprocess.run(["git", "init", "-q"], cwd=tmp_path, check=True)
    # install_hook resolves the hooks dir via git run from cwd
    import os

    cwd = os.getcwd()
    try:
        os.chdir(tmp_path)
        path = install_hook()
        assert path.exists()
        assert path.read_text() == HOOK_SCRIPT
        assert path.stat().st_mode & 0o111  # executable
        with pytest.raises(FileExistsError):
            install_hook()
        install_hook(force=True)  # force overwrites without raising
    finally:
        os.chdir(cwd)


class _FakeStdin:
    def __init__(self, text: str) -> None:
        self._text = text

    def read(self) -> str:
        return self._text
