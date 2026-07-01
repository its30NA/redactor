"""Local web UI — a browser front-end for the redactor.

Same privacy guarantees as the CLI: stdlib-only, binds to loopback, nothing leaves the
machine. The server is a thin shell around :class:`~redactor.pipeline.Pipeline`; all the
real work still happens in the pipeline.
"""

from __future__ import annotations

from redactor.webui.server import handle_sanitize, serve

__all__ = ["handle_sanitize", "serve"]
