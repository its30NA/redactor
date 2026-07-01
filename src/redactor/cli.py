"""Command-line interface for ``scrub``.

Subcommands::

    scrub [FILE]              sanitize a file or stdin -> stdout   (the default)
    scrub scan [PATH...]      scan files/dirs; report findings, exit 1 if any
    scrub check              scan git-staged files (used by the pre-commit hook)
    scrub install-hook       install the pre-commit hook into this repo
    scrub clipboard          sanitize the clipboard in place

``scrub FILE`` and ``scrub`` (stdin) keep working without naming ``sanitize`` — if the
first argument isn't a known subcommand, we dispatch to sanitize. Sanitized text always
goes to stdout; summaries, previews, and warnings go to stderr, so pipes stay clean.
"""

from __future__ import annotations

import argparse
import difflib
import sys
from collections import Counter
from pathlib import Path

from redactor import audit as audit_mod
from redactor.config import Config
from redactor.pipeline import Pipeline, SanitizeResult

_SUBCOMMANDS = frozenset({"sanitize", "scan", "check", "install-hook", "clipboard", "ui"})


# --------------------------------------------------------------------------- #
# Shared helpers
# --------------------------------------------------------------------------- #

def _pipeline(config_path: str | None) -> Pipeline:
    return Pipeline.from_config(Config.load(config_path))


def _read_input(path: str | None) -> str:
    if path is None or path == "-":
        return sys.stdin.read()
    return Path(path).read_text(encoding="utf-8", errors="replace")


def _summary(result: SanitizeResult) -> str:
    if not result.matches:
        return "No sensitive data detected."
    counts = Counter(m.label for m in result.matches)
    lines = [f"Redacted {result.redaction_count} item(s):"]
    lines += [f"  - {label}: {n}" for label, n in sorted(counts.items())]
    return "\n".join(lines)


def _window(text: str, focus: int, width: int) -> str:
    if len(text) <= width:
        return text
    half = width // 2
    start = max(0, focus - half)
    end = min(len(text), start + width)
    start = max(0, end - width)
    prefix = "…" if start > 0 else ""
    suffix = "…" if end < len(text) else ""
    return f"{prefix}{text[start:end]}{suffix}"


def _preview(original: str, result: SanitizeResult, width: int = 72) -> str:
    if not result.matches:
        return "No sensitive data detected."
    lines = [f"Preview — {result.redaction_count} redaction(s):"]
    label_col = min(max((len(m.label) for m in result.matches), default=0), 28)
    for m in result.matches:
        line_no = original.count("\n", 0, m.start) + 1
        line_start = original.rfind("\n", 0, m.start) + 1
        line_end = original.find("\n", m.end)
        if line_end == -1:
            line_end = len(original)
        line = original[line_start:line_end]
        rel_s, rel_e = m.start - line_start, m.end - line_start
        masked = f"{line[:rel_s]}[{m.label}]{line[rel_e:]}"
        lines.append(f"  Ln {line_no:<4} {m.label:<{label_col}}  {_window(masked, rel_s, width)}")
    return "\n".join(lines)


def _diff(original: str, sanitized: str, name: str) -> str:
    return "".join(
        difflib.unified_diff(
            original.splitlines(keepends=True),
            sanitized.splitlines(keepends=True),
            fromfile=f"{name} (original)",
            tofile=f"{name} (sanitized)",
        )
    )


# --------------------------------------------------------------------------- #
# Commands
# --------------------------------------------------------------------------- #

def cmd_sanitize(args: argparse.Namespace) -> int:
    try:
        text = _read_input(args.input)
    except OSError as exc:
        print(f"scrub: cannot read input: {exc}", file=sys.stderr)
        return 2

    result = _pipeline(args.config).sanitize(text)
    name = args.input or "stdin"

    if args.diff:
        sys.stdout.write(_diff(text, result.text, name))
    else:
        sys.stdout.write(result.text)

    if args.preview:
        print(_preview(text, result), file=sys.stderr)
    if args.audit:
        audit_json = audit_mod.dumps(audit_mod.build_audit(result.matches))
        if args.audit == "-":
            print(audit_json, file=sys.stderr)
        else:
            Path(args.audit).write_text(audit_json + "\n", encoding="utf-8")
    if args.summary:
        print(_summary(result), file=sys.stderr)
    return 0


def cmd_scan(args: argparse.Namespace) -> int:
    from redactor.scanner import sanitize_file, scan_paths

    pipeline = _pipeline(args.config)
    paths = [Path(p) for p in (args.paths or ["."])]
    total_files = 0
    total_findings = 0

    for report in scan_paths(paths, pipeline, recursive=not args.no_recursive):
        if report.skipped or not report.has_findings:
            continue
        total_files += 1
        total_findings += len(report.matches)
        kinds = ", ".join(sorted({m.label for m in report.matches}))
        if args.write:
            sanitize_file(report.path, pipeline)
            print(f"fixed  {report.path}  ({len(report.matches)}: {kinds})")
        else:
            print(f"{report.path}: {len(report.matches)} finding(s) — {kinds}")

    verb = "sanitized" if args.write else "found in"
    print(f"\n{total_findings} finding(s) {verb} {total_files} file(s).", file=sys.stderr)
    # Report mode signals findings via exit code (handy for CI); fix mode succeeds.
    return 0 if args.write or total_findings == 0 else 1


def cmd_check(args: argparse.Namespace) -> int:
    from redactor.githook import staged_files
    from redactor.scanner import scan_file

    pipeline = _pipeline(args.config)
    offending = 0
    for path in staged_files():
        if not path.is_file():
            continue
        report = scan_file(path, pipeline)
        if report.has_findings:
            offending += 1
            kinds = ", ".join(sorted({m.label for m in report.matches}))
            print(f"{path}: {len(report.matches)} finding(s) — {kinds}", file=sys.stderr)
    return 1 if offending else 0


def cmd_install_hook(args: argparse.Namespace) -> int:
    from redactor.githook import install_hook

    try:
        path = install_hook(force=args.force)
    except FileExistsError as exc:
        print(f"scrub: {exc}", file=sys.stderr)
        return 1
    except Exception as exc:  # e.g. not a git repo
        print(f"scrub: could not install hook: {exc}", file=sys.stderr)
        return 2
    print(f"Installed pre-commit hook at {path}", file=sys.stderr)
    return 0


def cmd_ui(args: argparse.Namespace) -> int:
    from redactor.webui.server import serve

    return serve(
        host=args.host,
        port=args.port,
        config_path=args.config,
        open_browser=not args.no_browser,
    )


def cmd_clipboard(args: argparse.Namespace) -> int:
    from redactor.clipboard import ClipboardUnavailable, copy, paste

    try:
        text = paste()
        result = _pipeline(args.config).sanitize(text)
        if result.matches:
            copy(result.text)
    except ClipboardUnavailable as exc:
        print(f"scrub: {exc}", file=sys.stderr)
        return 2
    print(_summary(result), file=sys.stderr)
    return 0


# --------------------------------------------------------------------------- #
# Parser wiring
# --------------------------------------------------------------------------- #

def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="scrub",
        description="Redact secrets from text before sharing it with external AI assistants.",
    )
    sub = parser.add_subparsers(dest="command")

    p_san = sub.add_parser("sanitize", help="Sanitize a file or stdin (default).")
    p_san.add_argument("input", nargs="?", help="File to sanitize; omit or '-' for stdin.")
    p_san.add_argument("-c", "--config", help="Path to a redactor.toml config file.")
    p_san.add_argument("--summary", action="store_true", help="Per-kind summary to stderr.")
    p_san.add_argument("--preview", action="store_true", help="Per-redaction report to stderr.")
    p_san.add_argument("--diff", action="store_true", help="Emit a unified diff instead.")
    p_san.add_argument("--audit", metavar="PATH", help="Write JSON audit log ('-' = stderr).")
    p_san.set_defaults(func=cmd_sanitize)

    p_scan = sub.add_parser("scan", help="Scan files/dirs; exit 1 if secrets found.")
    p_scan.add_argument("paths", nargs="*", help="Files or directories (default: '.').")
    p_scan.add_argument("-c", "--config", help="Path to a redactor.toml config file.")
    p_scan.add_argument("--write", action="store_true", help="Rewrite files in place, sanitized.")
    p_scan.add_argument("--no-recursive", action="store_true", help="Do not descend into dirs.")
    p_scan.set_defaults(func=cmd_scan)

    p_check = sub.add_parser("check", help="Scan git-staged files (for pre-commit).")
    p_check.add_argument("-c", "--config", help="Path to a redactor.toml config file.")
    p_check.set_defaults(func=cmd_check)

    p_hook = sub.add_parser("install-hook", help="Install the pre-commit hook.")
    p_hook.add_argument("--force", action="store_true", help="Overwrite an existing hook.")
    p_hook.set_defaults(func=cmd_install_hook)

    p_clip = sub.add_parser("clipboard", help="Sanitize the clipboard in place.")
    p_clip.add_argument("-c", "--config", help="Path to a redactor.toml config file.")
    p_clip.set_defaults(func=cmd_clipboard)

    p_ui = sub.add_parser("ui", help="Launch the local web UI in your browser.")
    p_ui.add_argument("--host", default="127.0.0.1", help="Bind address (default: 127.0.0.1).")
    p_ui.add_argument("--port", type=int, default=8765, help="Port (default: 8765).")
    p_ui.add_argument("-c", "--config", help="Path to a redactor.toml config file.")
    p_ui.add_argument("--no-browser", action="store_true", help="Do not auto-open a browser.")
    p_ui.set_defaults(func=cmd_ui)

    return parser


def _normalize(argv: list[str]) -> list[str]:
    """Default to the sanitize subcommand when none is given.

    ``scrub file.log`` and ``scrub`` (stdin) must keep working, so if the first token
    isn't a known subcommand or a help flag, we insert ``sanitize`` in front.
    """
    if argv and (argv[0] in _SUBCOMMANDS or argv[0] in ("-h", "--help")):
        return argv
    return ["sanitize", *argv]


def main(argv: list[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    args = build_parser().parse_args(_normalize(argv))
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
