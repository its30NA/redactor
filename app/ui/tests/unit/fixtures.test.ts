import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, test } from 'vitest';
import * as schema from '../../src/lib/schema';
import { DEMO_INPUTS, demoSecrets } from '../../src/lib/mock/demo';
import { FIXTURES } from '../../src/lib/mock/fixtures';
import { lineAt } from '../../src/lib/mock/render';
import type { ScanResponse } from '../../src/lib/types';

const FIXTURE_DIR = join(import.meta.dirname, '../../src/lib/mock/fixtures');
const files = readdirSync(FIXTURE_DIR).filter((f) => f.endsWith('.json'));

/** Which validator each fixture file must satisfy, by file name prefix. */
function checkFor(file: string): schema.Check {
  if (file.startsWith('scan-')) return schema.scanResponse;
  if (file.startsWith('session-')) return schema.sessionList;
  if (file.startsWith('clipboard-')) return schema.clipboardScrub;
  if (file === 'settings.json') return schema.settings;
  if (file === 'app-info.json') return schema.appInfo;
  throw new Error(`no validator registered for fixture ${file}`);
}

describe('fixtures', () => {
  test('types_match_fixture', () => {
    expect(files.length).toBeGreaterThanOrEqual(8);
    for (const file of files) {
      const json: unknown = JSON.parse(readFileSync(join(FIXTURE_DIR, file), 'utf8'));
      expect(checkFor(file)(json, file)).toEqual([]);
    }
  });

  test('every fixture file is registered with the mock and every registration has a file', () => {
    expect(Object.keys(FIXTURES).map((k) => `${k}.json`).sort()).toEqual([...files].sort());
  });

  test('validators reject a finding that carries a value field', () => {
    const scan = FIXTURES['scan-demo'] as ScanResponse;
    const leaky = { ...scan.findings[0], value: 'x' };
    expect(schema.finding(leaky, 'f')).toEqual(['f.value: unknown key']);
  });

  for (const [name, input] of Object.entries(DEMO_INPUTS)) {
    describe(`scan-${name}`, () => {
      const text = input();
      const res = (FIXTURES as Record<string, unknown>)[`scan-${name}`] as ScanResponse;

      test('exists and counts the input in UTF-16 code units', () => {
        expect(res).toBeDefined();
        expect(res.stats.chars).toBe(text.length);
      });

      test('findings are sorted, indexed, non-overlapping and inside the input', () => {
        let prevEnd = 0;
        res.findings.forEach((f, i) => {
          expect(f.id).toBe(i);
          expect(f.start).toBeGreaterThanOrEqual(prevEnd);
          expect(f.end).toBeGreaterThan(f.start);
          expect(f.end).toBeLessThanOrEqual(text.length);
          expect(f.line).toBe(lineAt(text, f.start));
          expect(text.slice(f.start, f.end)).not.toMatch(/[\r\n]/);
          prevEnd = f.end;
        });
      });

      test('output spans point at the finding placeholders', () => {
        for (const span of res.outputSpans) {
          const f = res.findings[span.findingId];
          expect(f).toBeDefined();
          expect(res.output.slice(span.start, span.end)).toBe(f?.placeholder);
        }
        const redacted = res.findings.filter((f) => !f.kept).length;
        expect(res.outputSpans.length).toBe(redacted);
      });

      test('the same value gets the same key and placeholder', () => {
        const byKey = new Map<string, string>();
        for (const f of res.findings) {
          const value = text.slice(f.start, f.end);
          const seen = byKey.get(value);
          if (seen) expect(f.valueKey).toBe(seen);
          byKey.set(value, f.valueKey);
        }
        const placeholderByKey = new Map<string, string>();
        for (const f of res.findings) {
          const p = placeholderByKey.get(f.valueKey);
          if (p) expect(f.placeholder).toBe(p);
          placeholderByKey.set(f.valueKey, f.placeholder);
        }
      });

      test('tier and label stats agree with the findings', () => {
        const byTier = { certain: 0, likely: 0, possible: 0 };
        const byLabel = new Map<string, number>();
        for (const f of res.findings) {
          byTier[f.tier]++;
          byLabel.set(f.label, (byLabel.get(f.label) ?? 0) + 1);
        }
        expect(res.stats.byTier).toEqual(byTier);
        expect(new Map(res.stats.byLabel.map((l) => [l.label, l.n]))).toEqual(byLabel);
      });
    });
  }

  test('no fixture contains a secret value from the demo input', () => {
    const all = files.map((f) => readFileSync(join(FIXTURE_DIR, f), 'utf8')).join('\n');
    const demo = DEMO_INPUTS.demo?.() ?? '';
    const values = (FIXTURES['scan-demo'] as ScanResponse).findings.map((f) => demo.slice(f.start, f.end));
    for (const v of [...values, ...Object.values(demoSecrets())]) {
      expect(v.length).toBeGreaterThanOrEqual(8);
      expect(all.includes(v), `fixture leaks a demo secret of length ${v.length}`).toBe(false);
    }
  });

  test('the demo fixture redacts every demo secret and nothing else', () => {
    const demo = DEMO_INPUTS.demo?.() ?? '';
    const res = FIXTURES['scan-demo'] as ScanResponse;
    for (const secret of Object.values(demoSecrets())) {
      expect(res.output.includes(secret)).toBe(false);
      expect(res.findings.some((f) => demo.slice(f.start, f.end) === secret)).toBe(true);
    }
    expect(res.output).toContain('550e8400-e29b-41d4-a716-446655440000');
    expect(res.output).toContain('9f8e7d6c5b4a3f2e1d0c9b8a7654321000abcdef');
  });
});
