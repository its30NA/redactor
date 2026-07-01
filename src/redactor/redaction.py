"""The redaction engine: turn detected spans into a sanitized string.

Two design choices matter here:

1. **Right-to-left replacement.** We splice placeholders in from the end of the
   text backwards so earlier offsets stay valid as we go. No offset bookkeeping,
   no bugs.

2. **Stable numbering.** When the *same* secret appears twice, both occurrences
   become the same placeholder; two *different* secrets of the same kind get
   distinct numbers (``#1``, ``#2``). This preserves relational meaning — an AI
   assistant can still see "this is the same key referenced in two places" —
   without ever revealing the value. When a kind appears only once, we omit the
   number for cleaner output: ``[REDACTED: OpenAI API Key]``.
"""

from __future__ import annotations

from dataclasses import dataclass

from redactor.models import Match

DEFAULT_TEMPLATE = "[REDACTED: {label}{suffix}]"


@dataclass(frozen=True, slots=True)
class Redactor:
    template: str = DEFAULT_TEMPLATE
    stable_numbering: bool = True

    def apply(self, text: str, matches: list[Match]) -> str:
        """Return ``text`` with every span in ``matches`` replaced by a placeholder.

        ``matches`` is expected to be non-overlapping (run it through
        :func:`redactor.resolver.resolve` first).
        """
        ordered = sorted(matches, key=lambda m: m.start)
        placeholders = self._placeholders(ordered)

        # Splice from right to left so untouched offsets remain valid.
        out = text
        for match, placeholder in reversed(list(zip(ordered, placeholders, strict=True))):
            out = out[: match.start] + placeholder + out[match.end :]
        return out

    def _placeholders(self, ordered: list[Match]) -> list[str]:
        # First pass: for each label, map distinct values to a 1-based index in
        # order of first appearance, and count how many distinct values exist.
        value_index: dict[tuple[str, str], int] = {}
        distinct_per_label: dict[str, int] = {}
        for m in ordered:
            key = (m.label, m.value)
            if key not in value_index:
                distinct_per_label[m.label] = distinct_per_label.get(m.label, 0) + 1
                value_index[key] = distinct_per_label[m.label]

        placeholders: list[str] = []
        for m in ordered:
            suffix = ""
            if self.stable_numbering and distinct_per_label[m.label] > 1:
                suffix = f" #{value_index[(m.label, m.value)]}"
            placeholders.append(self.template.format(label=m.label, suffix=suffix))
        return placeholders
