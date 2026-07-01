"""A tiny, dependency-free local web server for the redactor UI.

Design notes:

* **Loopback only.** Defaults to 127.0.0.1 so the server is reachable only from this
  machine — consistent with the tool's whole reason to exist.
* **Stdlib only.** ``http.server`` + ``json``; no Flask, no FastAPI. Keeps the install
  trivial and the attack surface tiny.
* **Thin.** The request handler just unpacks JSON and calls :func:`handle_sanitize`,
  which is a pure function (easy to unit-test without a socket). The pipeline does the
  actual redaction, exactly as it does for the CLI.
"""

from __future__ import annotations

import contextlib
import json
import sys
import webbrowser
from collections import Counter
from dataclasses import replace
from functools import lru_cache
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from importlib import resources

from redactor.config import Config
from redactor.pipeline import Pipeline


def _index_html() -> str:
    return resources.files("redactor.webui").joinpath("index.html").read_text(encoding="utf-8")


@lru_cache(maxsize=8)
def _pipeline(config_path: str | None, redact_pii: bool) -> Pipeline:
    """Build (and cache) a pipeline for a given config + PII toggle combination."""
    cfg = Config.load(config_path)
    if redact_pii and not cfg.redact_pii:
        cfg = replace(cfg, redact_pii=True)
    return Pipeline.from_config(cfg)


def handle_sanitize(payload: dict, config_path: str | None = None) -> dict:
    """Pure request handler: JSON in, JSON-ready dict out."""
    text = str(payload.get("text", ""))
    redact_pii = bool(payload.get("redact_pii", False))
    result = _pipeline(config_path, redact_pii).sanitize(text)
    counts = Counter(m.label for m in result.matches)
    return {
        "sanitized": result.text,
        "count": result.redaction_count,
        "summary": [{"label": label, "n": n} for label, n in sorted(counts.items())],
    }


class Handler(BaseHTTPRequestHandler):
    config_path: str | None = None

    def _send(self, code: int, body: str, ctype: str) -> None:
        data = body.encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self) -> None:  # noqa: N802 (http.server API)
        if self.path in ("/", "/index.html"):
            self._send(200, _index_html(), "text/html; charset=utf-8")
        else:
            self._send(404, "not found", "text/plain; charset=utf-8")

    def do_POST(self) -> None:  # noqa: N802
        if self.path != "/api/sanitize":
            self._send(404, "not found", "text/plain; charset=utf-8")
            return
        length = int(self.headers.get("Content-Length", 0) or 0)
        raw = self.rfile.read(length) if length else b"{}"
        try:
            payload = json.loads(raw or b"{}")
        except json.JSONDecodeError:
            self._send(400, json.dumps({"error": "invalid JSON"}), "application/json")
            return
        result = handle_sanitize(payload, self.config_path)
        self._send(200, json.dumps(result), "application/json")

    def log_message(self, *_args) -> None:  # keep the console quiet
        pass


def serve(
    host: str = "127.0.0.1",
    port: int = 8765,
    config_path: str | None = None,
    open_browser: bool = True,
) -> int:
    Handler.config_path = config_path
    try:
        httpd = ThreadingHTTPServer((host, port), Handler)
    except OSError as exc:
        print(f"scrub: cannot start UI on {host}:{port} ({exc}).", file=sys.stderr)
        print("Try a different port:  scrub ui --port 8790", file=sys.stderr)
        return 2

    url = f"http://{host}:{port}"
    print(f"redactor UI → {url}   (Ctrl-C to stop)", file=sys.stderr)
    print("100% local · nothing leaves your machine.", file=sys.stderr)
    if open_browser:
        # Opening a browser is best-effort — it often can't from WSL; the URL is printed.
        with contextlib.suppress(Exception):
            webbrowser.open(url)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\nstopped.", file=sys.stderr)
    finally:
        httpd.server_close()
    return 0
