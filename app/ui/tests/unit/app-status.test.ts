// @vitest-environment jsdom
/**
 * The shell's `app-status` event: the hotkey's state for Settings, and the notices the window
 * shows once (a hotkey refused at start or lost, settings not written or not readable).
 */
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { hotkeyState, listenForAppStatus } from '../../src/lib/app-status.svelte';
import { installMockBackend, type MockBackend } from '../../src/lib/mock/ipc';
import { toasts } from '../../src/lib/toast.svelte';
import type { AppNotice, AppStatus } from '../../src/lib/types';

let backend: MockBackend;
let unlisten: UnlistenFn;
const openSettings = vi.fn();

beforeEach(async () => {
  backend = installMockBackend({ scanLatencyMs: 0 });
  toasts.items = [];
  openSettings.mockReset();
  unlisten = await listenForAppStatus(openSettings);
});

afterEach(() => {
  unlisten();
  toasts.clear();
  backend.uninstall();
});

const status = (hotkeyActive: boolean, notice: AppNotice | null): AppStatus => ({ hotkeyActive, notice });

describe('app-status', () => {
  test('no event yet: app_info decides', () => expect(hotkeyState.active).toBeNull());

  test('a bare state change shows nothing and updates the hotkey state', async () => {
    await backend.emit('app-status', status(true, null));
    expect(hotkeyState.active).toBe(true);
    expect(toasts.items).toEqual([]);
  });

  test('a hotkey refused at start says so until dismissed and opens Settings', async () => {
    await backend.emit('app-status', status(false, 'hotkeyNotActive'));
    expect(hotkeyState.active).toBe(false);
    const t = toasts.items.at(-1)!;
    expect(t).toMatchObject({
      tone: 'warning',
      title: 'Hotkey not active',
      detail: 'Another app holds it, or the desktop refused it. Pick another key in Settings.',
      duration: 0,
    });
    expect(t.action?.label).toBe('Open Settings');
    toasts.act(t.id);
    expect(openSettings).toHaveBeenCalledOnce();
  });

  test('a hotkey the desktop dropped says so', async () => {
    await backend.emit('app-status', status(false, 'hotkeyLost'));
    expect(toasts.items.at(-1)).toMatchObject({
      title: 'Hotkey stopped working',
      detail: 'The desktop closed its shortcut. Set it again in Settings.',
      duration: 0,
    });
    expect(toasts.items.at(-1)?.action?.label).toBe('Open Settings');
  });

  test('a hotkey that works again retires its notices, and only those', async () => {
    await backend.emit('app-status', status(false, 'hotkeyNotActive'));
    await backend.emit('app-status', status(false, 'settingsNotSaved'));
    await backend.emit('app-status', status(false, 'hotkeyLost'));
    expect(toasts.items.map((t) => t.title)).toEqual([
      'Hotkey not active',
      'Settings not written to disk',
      'Hotkey stopped working',
    ]);
    // Still not active: nothing is retired.
    await backend.emit('app-status', status(false, null));
    expect(toasts.items).toHaveLength(3);
    await backend.emit('app-status', status(true, null));
    expect(hotkeyState.active).toBe(true);
    expect(toasts.items.map((t) => t.title)).toEqual(['Settings not written to disk']);
  });

  test('settings that could not be written or read say so, with no button', async () => {
    await backend.emit('app-status', status(true, 'settingsNotSaved'));
    await backend.emit('app-status', status(true, 'settingsUnreadable'));
    expect(toasts.items.map((t) => [t.title, t.detail, t.duration, t.action])).toEqual([
      ['Settings not written to disk', 'They apply now and hold until Redactor closes.', 0, undefined],
      [
        'Settings could not be read',
        'Redactor started with the defaults and kept the old file as settings.json.bad.',
        0,
        undefined,
      ],
    ]);
    expect(hotkeyState.active).toBe(true);
  });

  test('a payload that is not what the shell sends changes nothing', async () => {
    for (const junk of [null, 7, {}, { hotkeyActive: 'no', notice: 'toString' }, { notice: '__proto__' }])
      await backend.emit('app-status', junk as unknown as AppStatus);
    expect(hotkeyState.active).toBeNull();
    expect(toasts.items).toEqual([]);
  });
});
