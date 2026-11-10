// @vitest-environment jsdom
/**
 * The isolation hook (app/isolation/index.js) is an allowlist. It only runs inside the real
 * shell, so this test runs it here against messages in Tauri 2's real IPC shape, produced by
 * the UI's own calls through `@tauri-apps/api`.
 *
 * The shape is pinned to Tauri 2.12 (crate tauri =2.12.5, @tauri-apps/api 2.12.4):
 * `__TAURI_INTERNALS__.invoke(cmd, payload = {}, options)` in scripts/core.js builds
 * `{ cmd, callback, error, payload, options }` (callback and error are numeric ids); ipc.js
 * serializes the payload (`serializeIpcPayload`) and posts it to the isolation frame
 * (structured clone), whose hook receives it (tauri-utils src/pattern/isolation.js). The fake
 * `__TAURI_INTERNALS__` below does exactly that. A version bump fails the pins at the bottom,
 * so the shape is re-checked when Tauri changes.
 */
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { runInNewContext } from 'node:vm';
import { afterEach, beforeEach, describe, expect, test } from 'vitest';
import * as api from '../../src/lib/api';
import type { ScanRequest, Settings } from '../../src/lib/types';

type Message = { cmd: string; callback: number; error: number; payload: unknown; options: unknown };
type Hook = (m: unknown) => Message;

const APP = join(import.meta.dirname, '../../..');
const read = (path: string): string => readFileSync(join(APP, path), 'utf8');

function loadHook(): Hook {
  const frame: Record<string, unknown> = {};
  runInNewContext(read('isolation/index.js'), { window: frame });
  const hook = frame.__TAURI_ISOLATION_HOOK__;
  if (typeof hook !== 'function') throw new Error('the isolation script set no hook');
  return hook as Hook;
}

const hook = loadHook();

/** ipc.js `serializeIpcPayload` (Tauri 2.12.5). */
function serializeIpcPayload(data: unknown): unknown {
  const KEY = '__TAURI_TO_IPC_KEY__';
  if (typeof data === 'object' && data !== null && data.constructor === Array) {
    return (data as unknown[]).map((v) => serializeIpcPayload(v));
  }
  if (typeof data === 'object' && data !== null && KEY in data) {
    return (data as Record<string, () => unknown>)[KEY]!();
  }
  if (typeof data === 'object' && data !== null && data.constructor === Object) {
    const acc: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(data)) acc[k] = serializeIpcPayload(v);
    return acc;
  }
  return data;
}

let sent: Message[] = [];
let nextId = 1;

/** Answers that keep the UI's calls going (listen resolves to an event id, and so on). */
function answer(cmd: string): unknown {
  if (cmd === 'plugin:event|listen') return nextId++;
  if (cmd === 'plugin:window|is_maximized') return false;
  return null;
}

beforeEach(() => {
  sent = [];
  const w = window as unknown as Record<string, unknown>;
  w.__TAURI_INTERNALS__ = {
    metadata: {
      currentWindow: { label: 'main' },
      currentWebview: { windowLabel: 'main', label: 'main' },
    },
    transformCallback: () => nextId++,
    unregisterCallback: () => {},
    // scripts/core.js `invoke`, then ipc.js: serialize, structured clone into the frame.
    invoke(cmd: string, payload: unknown = {}, options?: unknown) {
      const message = { cmd, callback: nextId++, error: nextId++, payload, options };
      message.payload = serializeIpcPayload(message.payload);
      sent.push(structuredClone(message));
      return Promise.resolve(answer(cmd));
    },
  };
  w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
});

afterEach(() => {
  const w = window as unknown as Record<string, unknown>;
  delete w.__TAURI_INTERNALS__;
  delete w.__TAURI_EVENT_PLUGIN_INTERNALS__;
});

const req: ScanRequest = {
  text: 'DB_PASSWORD = something long',
  sensitivity: 'balanced',
  pii: false,
  stripInvisible: true,
  keep: ['00112233445566778899aabbccddeeff'],
  manual: [{ start: 0, end: 2 }],
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

/** Every call the UI makes, through the same functions the UI uses. */
async function everyUiCall(): Promise<void> {
  await api.scan(req);
  await api.scan({ ...req, keep: [], manual: [] });
  await api.readClipboard();
  await api.copyText('safe text');
  await api.openTextFile();
  await api.saveTextFile('safe text', 'notes.safe.txt');
  await api.sessionList();
  await api.sessionRemember('x = abcdefgh', 4, 12, 'Password');
  await api.sessionAdd('a-long-secret', 'Wi-Fi');
  await api.sessionForget('3f2a');
  await api.sessionClear();
  await api.getSettings();
  await api.setSettings(settings);
  await api.appInfo();
  for (const name of ['clipboard-scrubbed', 'wipe', 'session-changed', 'settings-changed'] as const) {
    const unlisten = await api.onEvent(name, () => {});
    unlisten();
  }
  await api.windowControls.minimize();
  await api.windowControls.toggleMaximize();
  await api.windowControls.isMaximized();
  await api.windowControls.close();
  const unlistenResize = await api.windowControls.onResized(() => {});
  unlistenResize();
  // Tauri's drag script (src/window/scripts/drag.js) for `data-tauri-drag-region`.
  const internals = (window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string) => Promise<unknown> } })
    .__TAURI_INTERNALS__;
  await internals.invoke('plugin:window|start_dragging');
  await internals.invoke('plugin:window|internal_toggle_maximize');
  await new Promise((r) => setTimeout(r, 0)); // let the unlisten calls go out
}

function rejected(m: Message): void {
  const out = hook(m);
  expect(out.cmd, `${m.cmd} ${JSON.stringify(m.payload)?.slice(0, 200)}`).toBe('isolation_rejected');
  expect(out.payload).toEqual({});
  expect(out.callback).toBe(m.callback);
  expect(out.error).toBe(m.error);
}

describe('the isolation hook', () => {
  test('lets every call the UI makes through, unchanged', async () => {
    await everyUiCall();
    const cmds = new Set(sent.map((m) => m.cmd));
    // All 13 app commands and the plugin commands are exercised.
    for (const c of Object.values(api.COMMANDS)) expect(cmds.has(c), c).toBe(true);
    for (const m of sent) {
      expect(Object.keys(m).sort()).toEqual(['callback', 'cmd', 'error', 'options', 'payload']);
      expect(m.options).toBeUndefined();
      expect(hook(m), m.cmd).toBe(m);
    }
    expect([...cmds].filter((c) => c.startsWith('plugin:')).sort()).toEqual([
      'plugin:event|listen',
      'plugin:event|unlisten',
      'plugin:window|close',
      'plugin:window|internal_toggle_maximize',
      'plugin:window|is_maximized',
      'plugin:window|minimize',
      'plugin:window|start_dragging',
      'plugin:window|toggle_maximize',
    ]);
  });

  test('rejects commands the UI does not use', async () => {
    await api.appInfo();
    const base = sent[0]!;
    for (const cmd of [
      'plugin:shell|execute',
      'plugin:fs|read_file',
      'plugin:opener|open_url',
      'plugin:dialog|open',
      'plugin:window|set_size',
      'plugin:webview|create_webview',
      'plugin:event|emit',
      'plugin:__TAURI_CHANNEL__|fetch',
      'constructor',
      '__proto__',
      'toString',
      'hasOwnProperty',
      // The hotkey and the tray scrub the clipboard inside the shell; the UI never asks.
      'scrub_clipboard',
    ]) {
      rejected({ ...base, cmd, payload: {} });
    }
  });

  test('rejects wrong argument names, types and sizes', async () => {
    await everyUiCall();
    const of = (cmd: string): Message => {
      const m = sent.find((s) => s.cmd === cmd);
      if (!m) throw new Error(cmd);
      return structuredClone(m);
    };
    const withPayload = (cmd: string, payload: unknown): Message => ({ ...of(cmd), payload });
    const scanWith = (patch: Partial<Record<keyof ScanRequest, unknown>>): Message =>
      withPayload('scan', { req: { ...req, ...patch } });

    rejected(withPayload('app_info', { extra: 1 }));
    rejected(withPayload('app_info', null));
    rejected(withPayload('scan', { req, more: true }));
    rejected(withPayload('scan', {}));
    rejected(scanWith({ text: 42 }));
    rejected(scanWith({ text: 'x'.repeat(20 * 1024 * 1024 + 1) }));
    rejected(scanWith({ sensitivity: 'loud' }));
    rejected(scanWith({ pii: 'yes' }));
    rejected(scanWith({ keep: ['k'.repeat(65)] }));
    rejected(scanWith({ manual: [{ start: 3, end: 1 }] }));
    rejected(scanWith({ manual: [{ start: 0, end: 10_000 }] }));
    rejected(scanWith({ manual: [{ start: -1, end: 1 }] }));
    rejected(scanWith({ manual: [{ start: 0.5, end: 1 }] }));
    rejected(withPayload('copy_text', { text: 'x'.repeat(64 * 1024 * 1024 + 1) }));
    rejected(withPayload('copy_text', { text: ['array'] }));
    rejected(withPayload('save_text_file', { text: 'a' }));
    rejected(withPayload('session_remember', { text: 'abc', start: 0, end: 9, label: 'L' }));
    rejected(withPayload('session_add', { secret: 's'.repeat(64 * 1024 + 1), label: 'L' }));
    rejected(withPayload('session_forget', { id: 7 }));
    rejected(withPayload('set_settings', { settings: { ...settings, theme: 'neon' } }));
    rejected(withPayload('set_settings', { settings: { ...settings, sneaky: true } }));
    const partial: Record<string, unknown> = { ...settings };
    delete partial.hotkey;
    rejected(withPayload('set_settings', { settings: partial }));
    rejected(withPayload('plugin:window|minimize', { label: 'other' }));
    rejected(withPayload('plugin:window|start_dragging', { label: 'main' }));
    rejected(
      withPayload('plugin:event|listen', { event: 'tauri://drag-drop', target: { kind: 'Any' }, handler: 1 }),
    );
    rejected(
      withPayload('plugin:event|listen', {
        event: 'wipe',
        target: { kind: 'Window', label: 'main' },
        handler: 1,
      }),
    );
    rejected(
      withPayload('plugin:event|listen', {
        event: 'tauri://resize',
        target: { kind: 'Window', label: 'other' },
        handler: 1,
      }),
    );
    rejected(withPayload('plugin:event|unlisten', { event: 'anything', eventId: 1 }));
    // Envelope: options (headers) and unknown keys.
    rejected({ ...of('app_info'), options: { headers: { 'X-Sneak': '1' } } });
    rejected({ ...of('app_info'), extra: true } as Message);
  });

  test('drops what is not an IPC message at all', () => {
    expect(() => hook(null)).toThrow();
    expect(() => hook('scan')).toThrow();
    expect(() => hook({ cmd: 'scan', callback: 'x', error: 2, payload: {} })).toThrow();
  });
});

describe('pins', () => {
  test('the Tauri versions whose IPC shape this test mirrors', () => {
    const pkg = JSON.parse(read('ui/package.json')) as { dependencies: Record<string, string> };
    expect(pkg.dependencies['@tauri-apps/api']).toBe('2.12.4');
    expect(read('src-tauri/Cargo.toml')).toMatch(/^tauri = \{ version = "=2\.12\.5"/m);
  });

  test("the UI's command list is the shell's", () => {
    const rust = [...read('src-tauri/src/ipc_names.rs').matchAll(/^\s+"([a-z_]+)",$/gm)].map((m) => m[1]);
    expect(rust.sort()).toEqual(Object.values(api.COMMANDS).sort());
  });
});
