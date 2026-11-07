import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { expect, test } from 'vitest';
import { demoSecrets } from '../../src/lib/mock/demo';

/**
 * The repo is public with push protection on. Demo and test secrets must be assembled at
 * runtime; this guard fails if a contiguous vendor-shaped token is written into any UI file.
 * The patterns are built from parts so this file does not trip scanners either.
 */
const ROOT = join(import.meta.dirname, '../..');
const SKIP = new Set(['node_modules', 'dist', 'dist-mock', 'test-results', 'playwright-report', '__screenshots__']);
const EXT = /\.(ts|js|svelte|json|html|css|md)$/;

const p = (...parts: string[]) => new RegExp(parts.join(''));
const SHAPES: [string, RegExp][] = [
  ['github', p('gh', '[pousr]_', '[A-Za-z0-9]{30,}')],
  ['github fine-grained', p('github', '_pat_', '[A-Za-z0-9_]{40,}')],
  ['openai', p('sk', '-(proj-|svcacct-|admin-)?', '[A-Za-z0-9_-]{32,}')],
  ['anthropic', p('sk', '-ant-', '[a-z0-9]+-', '[A-Za-z0-9_-]{32,}')],
  ['aws key id', p('(AKIA|ASIA)', '[A-Z2-7]{16}\\b')],
  ['slack', p('xox', '[bpars]-', '[0-9A-Za-z-]{20,}')],
  ['stripe', p('[sr]k_', '(live|test)_', '[0-9A-Za-z]{20,}')],
  ['google api', p('AI', 'za', '[0-9A-Za-z_-]{35}')],
  ['private key', p('-----BEGIN ', '[A-Z ]*PRIVATE KEY-----')],
];

function walk(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    if (SKIP.has(name)) continue;
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path, out);
    else if (EXT.test(name) && name !== 'package-lock.json') out.push(path);
  }
  return out;
}

test('the guard recognises the runtime-assembled demo tokens', () => {
  const s = demoSecrets();
  const shape = (name: string) => SHAPES.find(([n]) => n === name)?.[1] ?? /$^/;
  expect(shape('github').test(s.githubToken)).toBe(true);
  expect(shape('openai').test(s.openaiKey)).toBe(true);
  expect(shape('aws key id').test(s.awsKeyId)).toBe(true);
});

test('no contiguous vendor-shaped token in any UI source file', () => {
  const files = walk(ROOT);
  expect(files.length).toBeGreaterThan(10);
  const hits: string[] = [];
  for (const file of files) {
    const text = readFileSync(file, 'utf8');
    for (const [name, re] of SHAPES) if (re.test(text)) hits.push(`${relative(ROOT, file)}: ${name}`);
  }
  expect(hits).toEqual([]);
});
