// @vitest-environment jsdom
import { afterEach, expect, test } from 'vitest';
import { applyAppearance } from '../../src/lib/appearance.svelte';
import { installMockBackend } from '../../src/lib/mock/ipc';
import { settings } from '../../src/lib/settings.svelte';

afterEach(() => {
  delete document.documentElement.dataset.theme;
});

// The shell sets data-theme before the page's first paint (first_frame.rs), so the page starts in
// the window's colour. The defaults the page holds before `get_settings` answers say "system":
// applying them would drop that theme and show the OS scheme for a moment (F16).
test('before the settings load, the theme the shell set stays', () => {
  expect(settings.loaded).toBe(false);
  const root = document.documentElement;
  root.dataset.theme = 'light';
  applyAppearance(root);
  expect(root.dataset.theme).toBe('light');
});

test('once loaded, the saved theme decides', async () => {
  const backend = installMockBackend({ scanLatencyMs: 0 });
  try {
    await settings.load();
    const root = document.documentElement;
    root.dataset.theme = 'light';
    settings.value = { ...settings.value, theme: 'system' };
    applyAppearance(root);
    expect(root.dataset.theme).toBeUndefined();
    settings.value = { ...settings.value, theme: 'dark' };
    applyAppearance(root);
    expect(root.dataset.theme).toBe('dark');
  } finally {
    backend.uninstall();
  }
});
