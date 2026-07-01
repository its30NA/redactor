"""Optional local-LLM support.

Everything in this subpackage is opt-in and, by policy, *local only*. The backend
talks to a model running on your own machine (Ollama by default); no text ever leaves
the host. The deterministic pipeline is the source of truth — the LLM is a best-effort
second pass that proposes additional spans and **fails open**: if the model is missing
or misbehaves, sanitization proceeds with the deterministic results unharmed.
"""

from __future__ import annotations

from redactor.llm.backend import LLMBackend, LLMUnavailable, OllamaBackend

__all__ = ["LLMBackend", "LLMUnavailable", "OllamaBackend"]
