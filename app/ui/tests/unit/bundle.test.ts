import { mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, expect, test } from 'vitest';
import { build } from 'vite';

const ROOT = join(import.meta.dirname, '../..');
const out = mkdtempSync(join(tmpdir(), 'redactor-ui-build-'));
afterAll(() => rmSync(out, { recursive: true, force: true }));

function files(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? files(join(dir, e.name)) : [join(dir, e.name)],
  );
}

test('production bundle ships no mock backend, no fixtures and nothing remote', async () => {
  // Vitest runs with NODE_ENV=test, which would make Vite and Svelte emit a dev build.
  const env = process.env.NODE_ENV;
  process.env.NODE_ENV = 'production';
  try {
    await build({ root: ROOT, mode: 'production', logLevel: 'silent', build: { outDir: out, emptyOutDir: true } });
  } finally {
    process.env.NODE_ENV = env;
  }
  const all = files(out);
  const text = all
    .filter((f) => /\.(js|css|html)$/.test(f))
    .map((f) => readFileSync(f, 'utf8'))
    .join('\n');

  expect(text).not.toContain('mock backend');
  expect(text).not.toContain('__REDACTOR_MOCK__');
  expect(text).not.toContain('staging rollout'); // demo input
  // No @tauri-apps/api/mocks code (the real core only reads window.__TAURI_INTERNALS__)...
  for (const name of ['mockIPC', 'mockWindows', 'mockConvertFileSrc', 'clearMocks'])
    expect(text, name).not.toContain(name);
  expect(text).not.toMatch(/__TAURI_INTERNALS__\s*=[^=]/);
  // ...and none of the fixture data (demo session labels, demo always-redact words, demo text).
  for (const demo of ['Staging DB', 'NAS admin', 'Nightjar', 'build-07.corp.example'])
    expect(text, demo).not.toContain(demo);
  // Nothing remote is loaded and there is no network API in the bundle. (Svelte's error strings
  // mention svelte.dev doc URLs and SVG/XHTML namespaces; those are text, not requests.)
  expect(text).not.toMatch(/url\(\s*['"]?(https?:)?\/\//i);
  expect(text).not.toMatch(/@import/);
  expect(text).not.toMatch(/\bfetch\(|XMLHttpRequest|new WebSocket|EventSource|sendBeacon/);
  expect(text).not.toMatch(/\beval\(|new Function\(/);
  const html = readFileSync(join(out, 'index.html'), 'utf8');
  expect(html).not.toMatch(/(src|href)="(https?:)?\/\//);
  expect(html).not.toMatch(/<script(?![^>]*\bsrc=)[^>]*>/); // no inline scripts
  expect(html).not.toMatch(/<style|style=/); // no inline styles
  expect(all.some((f) => f.endsWith('.woff2'))).toBe(true); // fonts bundled locally
}, 60_000);
