"""Overlap resolution.

Multiple detectors can flag overlapping spans (e.g. a JWT that also looks like a
high-entropy string, or the Anthropic/OpenAI prefix clash). We must emit a single,
non-overlapping set of redactions, otherwise the replacement offsets collide.

The strategy is a greedy interval selection: consider candidates in priority order
(highest confidence first, then longest, then leftmost) and accept a candidate only
if it does not overlap anything already accepted. This deterministically keeps the
most trustworthy, most specific match at each location.
"""

from __future__ import annotations

from redactor.models import Match


def resolve(matches: list[Match]) -> list[Match]:
    """Return a non-overlapping subset of ``matches``, sorted by position."""
    accepted: list[Match] = []
    # Priority: confidence desc, length desc, start asc — a total order, so the
    # result is fully deterministic regardless of detector execution order.
    for candidate in sorted(matches, key=lambda m: (-m.confidence, -m.length, m.start)):
        if any(candidate.overlaps(a) for a in accepted):
            continue
        accepted.append(candidate)
    accepted.sort(key=lambda m: m.start)
    return accepted
