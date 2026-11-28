// @vitest-environment jsdom
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, expect, test } from 'vitest';

// The shell runs this at document start (first_frame.rs `page_theme_script`), where the
// webview may not have created <html> yet (F16: WebView2 showed a dark frame on Light).
const source = readFileSync(resolve(__dirname, '../../../src-tauri/src/page_theme.js'), 'utf8');
const run = (theme: string) => new Function(source.replaceAll('__THEME__', theme))();

afterEach(() => {
  if (!document.documentElement) document.appendChild(document.createElement('html'));
  delete document.documentElement.dataset.theme;
});

test('sets the theme on an existing root', () => {
  run('light');
  expect(document.documentElement.dataset.theme).toBe('light');
});

test('sets the theme on a root that appears later, before anything paints', async () => {
  document.removeChild(document.documentElement);
  run('dark');
  document.appendChild(document.createElement('html'));
  await Promise.resolve(); // mutation observers run as a microtask, before the next paint
  expect(document.documentElement.dataset.theme).toBe('dark');
});
