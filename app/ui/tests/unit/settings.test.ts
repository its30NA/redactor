// @vitest-environment jsdom
import { afterEach, beforeEach, expect, test } from 'vitest';
import { installMockBackend, type MockBackend } from '../../src/lib/mock/ipc';
import { settings } from '../../src/lib/settings.svelte';
import { toasts } from '../../src/lib/toast.svelte';

let backend: MockBackend;

beforeEach(async () => {
  backend = installMockBackend({ scanLatencyMs: 0 });
  toasts.items = [];
  await settings.load();
});
afterEach(() => backend.uninstall());

test('a message from the shell becomes the toast detail and the change rolls back', async () => {
  backend.rejectSettings('Ctrl+Alt+K is in use by another app.');
  const before = settings.value.hotkey;
  await settings.update({ hotkey: 'Ctrl+Alt+K' });
  expect(settings.value.hotkey).toBe(before);
  expect(toasts.items.at(-1)).toMatchObject({
    tone: 'error',
    title: 'Setting not saved',
    detail: 'Ctrl+Alt+K is in use by another app.',
  });
});

test('a blank rejection shows the title alone', async () => {
  backend.rejectSettings('   ');
  await settings.update({ pii: true });
  expect(settings.value.pii).toBe(false);
  expect(toasts.items.at(-1)?.title).toBe('Setting not saved');
  expect(toasts.items.at(-1)?.detail).toBeUndefined();
});

test('a saved setting shows no toast', async () => {
  await settings.update({ pii: true });
  expect(settings.value.pii).toBe(true);
  expect(toasts.items).toEqual([]);
});
