"""Overlap resolution.

Multiple detectors can flag overlapping spans — a JWT that also looks like a
high-entropy string, or (worse) an email detector whose match bleeds into the host
portion of a ``user:password@host`` connection string.

For a *security* tool, the cardinal rule is: **never leave part of a sensitive span
exposed.** An earlier greedy design that simply kept the highest-confidence match and
dropped overlapping ones could do exactly that — if the winner was shorter than a
dropped match, the uncovered remainder (e.g. the first characters of a password)
leaked into the output.

So instead we **merge** overlapping matches into a single redaction spanning their
union. The merged redaction takes its label from the highest-confidence contributor
(ties broken by length). This guarantees full coverage at the cost of occasionally
redacting a little extra context — the right trade-off when the alternative is a leak.
"""

from __future__ import annotations

from dataclasses import replace

from redactor.models import Match


def resolve(matches: list[Match]) -> list[Match]:
    """Merge overlapping matches, returning non-overlapping spans sorted by position."""
    if not matches:
        return []

    ordered = sorted(matches, key=lambda m: (m.start, m.end))
    clusters: list[list[Match]] = [[ordered[0]]]
    cluster_end = ordered[0].end
    for m in ordered[1:]:
        if m.start < cluster_end:  # touches the current run of overlapping spans
            clusters[-1].append(m)
            cluster_end = max(cluster_end, m.end)
        else:
            clusters.append([m])
            cluster_end = m.end

    resolved: list[Match] = []
    for cluster in clusters:
        # Representative label/kind come from the most trustworthy contributor.
        rep = max(cluster, key=lambda m: (m.confidence, m.length))
        start = min(m.start for m in cluster)
        end = max(m.end for m in cluster)
        if start == rep.start and end == rep.end:
            resolved.append(rep)
        else:
            # Extend the winner to cover the whole overlapping region.
            resolved.append(replace(rep, start=start, end=end))
    return resolved
