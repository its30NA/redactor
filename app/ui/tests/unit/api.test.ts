// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, test } from 'vitest';
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import * as api from '../../src/lib/api';
import type { ScanRequest, Settings } from '../../src/lib/types';

type Call = { cmd: string; args: Record<string, unknown> };

const CAMEL = /^[a-z][a-zA-Z0-9]*$/;

let calls: Call[] = [];

beforeEach(() => {
  calls = [];
  mockWindows('main');
  mockIPC(
    (cmd, payload) => {
      calls.push({ cmd, args: (payload ?? {}) as Record<string, unknown> });
      if (cmd === 'plugin:window|is_maximized') return false;
      return null;
    },
    { shouldMockEvents: true },
  );
});

afterEach(() => {
  clearMocks();
});

function lastCall(): Call {
  const c = calls.at(-1);
  if (!c) throw new Error('no IPC call recorded');
  return c;
}

function assertCamel(value: unknown, path = 'args'): void {
  if (Array.isArray(value)) {
    value.forEach((v, i) => assertCamel(v, `${path}[${i}]`));
    return;
  }
  if (value && typeof value === 'object') {
    for (const [k, v] of Object.entries(value)) {
      expect(k, `${path}.${k} must be camelCase`).toMatch(CAMEL);
      assertCamel(v, `${path}.${k}`);
    }
  }
}

const req: ScanRequest = {
  text: 'hello',
  sensitivity: 'balanced',
  pii: false,
  stripInvisible: true,
  keep: ['00112233445566778899aabbccddeeff'],
  manual: [{ start: 1, end: 3 }],
};

const settings: Settings = {
  theme: 'system',
  sensitivity: 'paranoid',
  pii: true,
  stripInvisible: true,
  hotkey: 'Ctrl+Alt+R',
  trayMode: false,
  autoWipeMinutes: 10,
  wipeOnLock: true,
  contentProtection: true,
  placeholderStyle: 'labeled',
  reducedMotion: 'system',
  alwaysRedact: ['Nightjar'],
  firstRunDone: false,
};

/** Every IPC command, the call that exercises it, and the IPC payload it must send. */
const cases: { fn: string; run: () => Promise<unknown>; cmd: string; args: Record<string, unknown> }[] = [
  { fn: 'scan', run: () => api.scan(req), cmd: 'scan', args: { req } },
  { fn: 'readClipboard', run: () => api.readClipboard(), cmd: 'read_clipboard', args: {} },
  { fn: 'copyText', run: () => api.copyText('safe'), cmd: 'copy_text', args: { text: 'safe' } },
  { fn: 'openTextFile', run: () => api.openTextFile(), cmd: 'open_text_file', args: {} },
  {
    fn: 'saveTextFile',
    run: () => api.saveTextFile('safe', 'notes.safe.txt'),
    cmd: 'save_text_file',
    args: { text: 'safe', suggestedName: 'notes.safe.txt' },
  },
  { fn: 'sessionList', run: () => api.sessionList(), cmd: 'session_list', args: {} },
  {
    fn: 'sessionRemember',
    run: () => api.sessionRemember('x = abcdefgh', 4, 12, 'Password'),
    cmd: 'session_remember',
    args: { text: 'x = abcdefgh', start: 4, end: 12, label: 'Password' },
  },
  {
    fn: 'sessionAdd',
    run: () => api.sessionAdd('a-long-secret', 'Wi-Fi'),
    cmd: 'session_add',
    args: { secret: 'a-long-secret', label: 'Wi-Fi' },
  },
  { fn: 'sessionForget', run: () => api.sessionForget('s1'), cmd: 'session_forget', args: { id: 's1' } },
  { fn: 'sessionClear', run: () => api.sessionClear(), cmd: 'session_clear', args: {} },
  { fn: 'getSettings', run: () => api.getSettings(), cmd: 'get_settings', args: {} },
  { fn: 'setSettings', run: () => api.setSettings(settings), cmd: 'set_settings', args: { settings } },
  { fn: 'appInfo', run: () => api.appInfo(), cmd: 'app_info', args: {} },
];

describe('api', () => {
  test('api_calls_invoke_with_camelcase_args', async () => {
    for (const c of cases) {
      await c.run();
      const call = lastCall();
      expect(call.cmd, c.fn).toBe(c.cmd);
      expect(call.args, c.fn).toEqual(c.args);
      assertCamel(call.args, c.fn);
    }
  });

  test('every IPC command has a wrapper and a whitelisted name', () => {
    const names = Object.values(api.COMMANDS).sort();
    expect(names).toEqual(
      [
        'app_info',
        'copy_text',
        'get_settings',
        'open_text_file',
        'read_clipboard',
        'save_text_file',
        'scan',
        'set_settings',
        'session_add',
        'session_clear',
        'session_forget',
        'session_list',
        'session_remember',
      ].sort(),
    );
    const covered = new Set(cases.map((c) => c.cmd));
    for (const n of names) expect(covered.has(n), `${n} has a test case`).toBe(true);
  });

  test('window controls go through the Tauri window API', async () => {
    await api.windowControls.minimize();
    expect(lastCall()).toEqual({ cmd: 'plugin:window|minimize', args: { label: 'main' } });
    await api.windowControls.toggleMaximize();
    expect(lastCall()).toEqual({ cmd: 'plugin:window|toggle_maximize', args: { label: 'main' } });
    await expect(api.windowControls.isMaximized()).resolves.toBe(false);
    await api.windowControls.close();
    expect(lastCall()).toEqual({ cmd: 'plugin:window|close', args: { label: 'main' } });
  });

  test('onEvent delivers typed payloads and unlistens', async () => {
    const got: string[] = [];
    const un = await api.onEvent('wipe', (p) => got.push(p.reason));
    await emit('wipe', { reason: 'idle' });
    expect(got).toEqual(['idle']);
    un();
    await emit('wipe', { reason: 'lock' });
    expect(got).toEqual(['idle']);
  });

  test('isDesktop is false outside the Tauri shell', () => {
    expect(api.isDesktop()).toBe(false);
  });
});
