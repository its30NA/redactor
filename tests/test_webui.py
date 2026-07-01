"""Web UI: pure handler logic + a real localhost round-trip."""

from __future__ import annotations

import json
import threading
import urllib.request
from http.server import ThreadingHTTPServer

from redactor.webui.server import Handler, _index_html, handle_sanitize


def test_handle_sanitize_redacts_and_summarizes() -> None:
    out = handle_sanitize({"text": "KEY=sk-abcdEFGH1234ijklMNOP5678"})
    assert out["sanitized"] == "KEY=[REDACTED: OpenAI API Key]"
    assert out["count"] == 1
    assert out["summary"] == [{"label": "OpenAI API Key", "n": 1}]


def test_handle_sanitize_pii_toggle() -> None:
    text = "email jane.doe@corp.io"
    assert handle_sanitize({"text": text, "redact_pii": False})["count"] == 0
    on = handle_sanitize({"text": text, "redact_pii": True})
    assert on["count"] == 1
    assert on["summary"][0]["label"] == "Email Address"


def test_handle_sanitize_empty() -> None:
    out = handle_sanitize({"text": ""})
    assert out["count"] == 0
    assert out["sanitized"] == ""


def test_index_html_loads() -> None:
    html = _index_html()
    assert "<html" in html.lower()
    assert "redactor" in html.lower()
    assert "/api/sanitize" in html  # front-end wired to the endpoint


def test_live_server_roundtrip() -> None:
    Handler.config_path = None
    httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    port = httpd.server_address[1]
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    try:
        base = f"http://127.0.0.1:{port}"
        html = urllib.request.urlopen(base + "/", timeout=5).read().decode()
        assert "<html" in html.lower()

        req = urllib.request.Request(
            base + "/api/sanitize",
            data=json.dumps({"text": "tok=ghp_" + "A" * 36}).encode(),
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        resp = json.loads(urllib.request.urlopen(req, timeout=5).read())
        assert resp["sanitized"] == "tok=[REDACTED: GitHub Token]"
        assert resp["count"] == 1
    finally:
        httpd.shutdown()
        httpd.server_close()
