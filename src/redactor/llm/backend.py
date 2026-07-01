"""LLM backends.

A backend is anything that can turn a prompt into a completion string. Keeping this
behind a tiny protocol means the detector never knows (or cares) whether it's talking
to Ollama, llama.cpp, or a test double — which is exactly what keeps the LLM detector
unit-testable without a model or a network.

The shipped backend targets **Ollama** over localhost using only stdlib ``urllib`` —
no third-party HTTP client, no added dependency, no way to reach a non-local host by
accident (the host defaults to 127.0.0.1).
"""

from __future__ import annotations

import json
import urllib.error
import urllib.request
from typing import Protocol, runtime_checkable


class LLMUnavailable(RuntimeError):
    """Raised when a local model cannot be reached or errors out."""


@runtime_checkable
class LLMBackend(Protocol):
    def complete(self, prompt: str) -> str:
        """Return the model's completion for ``prompt`` (temperature 0)."""
        ...


class OllamaBackend:
    """Talk to a locally running Ollama server.

    Recommended models for consumer hardware: ``qwen2.5:3b`` or ``llama3.2:3b``
    (~2 GB, fast). Point ``host`` only at a local address — this tool exists to keep
    data on your machine.
    """

    def __init__(
        self,
        model: str = "qwen2.5:3b",
        host: str = "http://127.0.0.1:11434",
        timeout: float = 30.0,
    ) -> None:
        self.model = model
        self.host = host.rstrip("/")
        self.timeout = timeout

    def complete(self, prompt: str) -> str:
        payload = json.dumps(
            {
                "model": self.model,
                "prompt": prompt,
                "stream": False,
                "options": {"temperature": 0},
            }
        ).encode("utf-8")
        req = urllib.request.Request(
            f"{self.host}/api/generate",
            data=payload,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                body = json.loads(resp.read().decode("utf-8"))
        except (urllib.error.URLError, TimeoutError, OSError) as exc:
            raise LLMUnavailable(f"cannot reach Ollama at {self.host}: {exc}") from exc
        except json.JSONDecodeError as exc:
            raise LLMUnavailable(f"invalid response from Ollama: {exc}") from exc
        return str(body.get("response", ""))
