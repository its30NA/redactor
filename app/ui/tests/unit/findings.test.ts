import { describe, expect, test } from 'vitest';
import { freshKeys, groupFindings } from '../../src/lib/findings';
import { FIXTURES } from '../../src/lib/mock/fixtures';
import type { Finding, ScanResponse } from '../../src/lib/types';

const demo = FIXTURES['scan-demo'] as ScanResponse;

const f = (over: Partial<Finding>): Finding => ({
  id: 0,
  start: 0,
  end: 1,
  line: 1,
  kind: 'password',
  label: 'Password',
  tier: 'certain',
  confidence: 0.95,
  detector: 'x',
  reason: 'r',
  source: 'pattern',
  valueKey: 'k',
  placeholder: '[REDACTED: Password]',
  kept: false,
  ...over,
});

describe('groupFindings', () => {
  test('one rail item per value, grouped by tier, in document order', () => {
    const g = groupFindings(demo.findings);
    const keys = new Set(demo.findings.map((x) => x.valueKey));
    expect(g.certain.length + g.likely.length + g.possible.length).toBe(keys.size);
    expect(g.total).toBe(keys.size);
    for (const tier of ['certain', 'likely', 'possible'] as const) {
      const starts = g[tier].map((x) => x.first.start);
      expect(starts).toEqual([...starts].sort((a, b) => a - b));
      for (const item of g[tier]) expect(item.tier).toBe(tier);
    }
  });

  test('a repeated value is one item with every occurrence and line', () => {
    const findings = [
      f({ id: 0, start: 0, end: 5, line: 2, valueKey: 'pw', confidence: 0.95 }),
      f({ id: 1, start: 9, end: 12, line: 3, valueKey: 'other', label: 'Token', tier: 'likely', confidence: 0.7 }),
      f({ id: 2, start: 20, end: 25, line: 9, valueKey: 'pw', source: 'propagated', confidence: 0.95 }),
    ];
    const g = groupFindings(findings);
    expect(g.certain).toHaveLength(1);
    expect(g.certain[0]?.count).toBe(2);
    expect(g.certain[0]?.lines).toEqual([2, 9]);
    expect(g.certain[0]?.ids).toEqual([0, 2]);
    expect(g.certain[0]?.first.id).toBe(0);
    expect(g.likely[0]?.label).toBe('Token');
  });

  test('the group takes the tier of its most confident occurrence', () => {
    const g = groupFindings([
      f({ id: 0, valueKey: 'v', tier: 'possible', confidence: 0.5 }),
      f({ id: 1, start: 3, end: 4, valueKey: 'v', tier: 'certain', confidence: 0.97 }),
    ]);
    expect(g.possible).toHaveLength(0);
    expect(g.certain[0]?.confidence).toBe(0.97);
  });

  test('counts redacted and kept values; manual findings are flagged', () => {
    const g = groupFindings([
      f({ id: 0, valueKey: 'a', kept: true }),
      f({ id: 1, start: 2, end: 3, valueKey: 'b' }),
      f({ id: 2, start: 4, end: 5, valueKey: 'm', source: 'manual', label: 'Manual' }),
    ]);
    expect(g.kept).toBe(1);
    expect(g.redacted).toBe(2);
    expect(g.certain.find((x) => x.key === 'm')?.manual).toBe(true);
  });

  test('handles 50 000 findings quickly', () => {
    const many = Array.from({ length: 50_000 }, (_, i) =>
      f({ id: i, start: i * 10, end: i * 10 + 8, valueKey: `k${i}`, line: i + 1 }),
    );
    const t0 = performance.now();
    const g = groupFindings(many);
    expect(performance.now() - t0).toBeLessThan(150);
    expect(g.total).toBe(50_000);
  });
});

describe('freshKeys', () => {
  test('everything is fresh after a paste; only new values after an edit', () => {
    const before = [f({ valueKey: 'a' }), f({ valueKey: 'b' })];
    const after = [f({ valueKey: 'a' }), f({ valueKey: 'c' })];
    expect(freshKeys(null, after)).toEqual(new Set(['a', 'c']));
    expect(freshKeys(before, after)).toEqual(new Set(['c']));
  });
});
