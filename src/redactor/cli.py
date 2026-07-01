"""Command-line interface for ``scrub``.

Usage patterns::

    scrub file.log                 # sanitized text to stdout
    cat file.log | scrub           # read from stdin
    scrub file.log --summary       # + per-kind redaction counts on stderr
    scrub file.log --diff          # unified diff instead of full output
    scrub file.log -c my.toml      # explicit config

Design principle: sanitized text goes to **stdout** and everything else
(summaries, warnings) goes to **stderr**, so ``scrub`` composes cleanly in pipes.
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


def _diff(original: str, sanitized: str, name: str) -> str:
    return "".join(
        difflib.unified_diff(
            original.splitlines(keepends=True),
            sanitized.splitlines(keepends=True),
            fromfile=f"{name} (original)",
            tofile=f"{name} (sanitized)",
        )
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="scrub",
        description="Redact secrets from text before sharing it with external AI assistants.",
    )
    parser.add_argument(
        "input",
        nargs="?",
        help="File to sanitize. Omit or use '-' to read from stdin.",
    )
    parser.add_argument("-c", "--config", help="Path to a redactor.toml config file.")
    parser.add_argument(
        "--summary",
        action="store_true",
        help="Print a per-kind redaction summary to stderr.",
    )
    parser.add_argument(
        "--diff",
        action="store_true",
        help="Emit a unified diff instead of the full sanitized text.",
    )
    parser.add_argument(
        "--audit",
        metavar="PATH",
        help="Write a JSON audit log (kinds, offsets, salted fingerprints — never "
        "the raw values) to PATH. Use '-' for stderr.",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)

    try:
        text = _read_input(args.input)
    except OSError as exc:
        print(f"scrub: cannot read input: {exc}", file=sys.stderr)
        return 2

    pipeline = Pipeline.from_config(Config.load(args.config))
    result = pipeline.sanitize(text)

    name = args.input or "stdin"
    if args.diff:
        sys.stdout.write(_diff(text, result.text, name))
    else:
        sys.stdout.write(result.text)

    if args.audit:
        audit_json = audit_mod.dumps(audit_mod.build_audit(result.matches))
        if args.audit == "-":
            print(audit_json, file=sys.stderr)
        else:
            Path(args.audit).write_text(audit_json + "\n", encoding="utf-8")

    if args.summary:
        print(_summary(result), file=sys.stderr)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
