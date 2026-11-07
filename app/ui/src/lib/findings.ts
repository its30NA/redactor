/**
 * Findings as the rail shows them: one item per distinct value (`valueKey`), because Keep,
 * Redact and Remember act on a value, and a value found once is redacted everywhere.
 */
import type { Finding, Source, Tier } from './types';

export const TIERS: readonly Tier[] = ['certain', 'likely', 'possible'];

export interface FindingGroup {
  key: string;
  tier: Tier;
  label: string;
  kind: string;
  source: Source;
  reason: string;
  detector: string;
  /** Highest confidence over the occurrences. */
  confidence: number;
  placeholder: string;
  kept: boolean;
  manual: boolean;
  /** The first occurrence in the text. */
  first: Finding;
  /** The most confident occurrence (its reason and detector are shown). */
  best: Finding;
  count: number;
  ids: number[];
  /** Distinct lines, ascending. */
  lines: number[];
}

export interface Grouped {
  certain: FindingGroup[];
  likely: FindingGroup[];
  possible: FindingGroup[];
  /** Distinct values. */
  total: number;
  /** Distinct values that are redacted (not kept). */
  redacted: number;
  kept: number;
}

export const EMPTY_GROUPS: Grouped = { certain: [], likely: [], possible: [], total: 0, redacted: 0, kept: 0 };

export function groupFindings(findings: readonly Finding[]): Grouped {
  const byKey = new Map<string, FindingGroup>();
  for (const f of findings) {
    const g = byKey.get(f.valueKey);
    if (!g) {
      byKey.set(f.valueKey, {
        key: f.valueKey,
        tier: f.tier,
        label: f.label,
        kind: f.kind,
        source: f.source,
        reason: f.reason,
        detector: f.detector,
        confidence: f.confidence,
        placeholder: f.placeholder,
        kept: f.kept,
        manual: f.source === 'manual',
        first: f,
        best: f,
        count: 1,
        ids: [f.id],
        lines: [f.line],
      });
      continue;
    }
    g.count++;
    g.ids.push(f.id);
    if (g.lines.at(-1) !== f.line) g.lines.push(f.line);
    g.kept ||= f.kept;
    g.manual ||= f.source === 'manual';
    if (f.confidence > g.confidence) {
      g.confidence = f.confidence;
      g.tier = f.tier;
      g.best = f;
      g.reason = f.reason;
      g.detector = f.detector;
    }
  }
  const out: Grouped = { certain: [], likely: [], possible: [], total: byKey.size, redacted: 0, kept: 0 };
  // Map iteration follows insertion order, i.e. the first occurrence: document order.
  for (const g of byKey.values()) {
    out[g.tier].push(g);
    if (g.kept) out.kept++;
    else out.redacted++;
  }
  return out;
}

/** Values in `next` that `prev` did not have (all of them after a paste). These "lock on". */
export function freshKeys(prev: readonly Finding[] | null, next: readonly Finding[]): Set<string> {
  const seen = new Set(prev?.map((f) => f.valueKey) ?? []);
  const out = new Set<string>();
  for (const f of next) if (!seen.has(f.valueKey)) out.add(f.valueKey);
  return out;
}
