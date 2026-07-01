"""redactor — local, privacy-first text sanitization.

Public API::

    from redactor import Pipeline
    result = Pipeline().sanitize("OPENAI_API_KEY=sk-...")
    print(result.text)
"""

from __future__ import annotations

from redactor.models import Match
from redactor.pipeline import Pipeline, SanitizeResult

__all__ = ["Match", "Pipeline", "SanitizeResult", "__version__"]
__version__ = "0.1.0"
