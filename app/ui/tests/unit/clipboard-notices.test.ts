// @vitest-environment jsdom
/**
 * The window's toasts for a hotkey, tray or `--scrub-clipboard` scrub: the result, the wait for
 * the window, and the scrub that expired before the window came to the front.
 */
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { listenForClipboardScrubs } from '../../src/lib/clipboard-notices';
import { installMockBackend, type MockBackend } from '../../src/lib/mock/ipc';
import { toasts } from '../../src/lib/toast.svelte';
import type { ClipboardScrub, ClipboardStatus } from '../../src/lib/types';

let backend: MockBackend;
let unlisten: UnlistenFn[];

const result = (status: ClipboardStatus, removed = 0): ClipboardScrub => ({
  status,
  removed,
  byLabel: [],
  historyCleaned: false,
});

const titles = () => toasts.items.map((t) => t.title);

beforeEach(async () => {
  backend = installMockBackend({ scanLatencyMs: 0 });
  toasts.items = [];
  unlisten = await Promise.all(listenForClipboardScrubs());
});

afterEach(() => {
  for (const u of unlisten) u();
  toasts.clear();
  backend.uninstall();
});

describe('scrub results', () => {
  test('a scrub that failed says so', async () => {
    await backend.emit('clipboard-scrubbed', result('error'));
    expect(toasts.items.at(-1)).toMatchObject({
      tone: 'error',
      title: 'Clipboard not scrubbed',
      detail: 'It could not be read, or it changed during the scrub.',
    });
  });

  test('a failed scrub stays until dismissed and offers Clean now', async () => {
    await backend.emit('clipboard-scrubbed', result('error'));
    const notice = toasts.items.at(-1)!;
    expect(notice.duration).toBe(0);
    expect(notice.action?.label).toBe('Clean now');
    toasts.act(notice.id);
    expect(backend.calls.at(-1)?.cmd).toBe('scrub_clipboard');
    expect(toasts.items.some((t) => t.id === notice.id)).toBe(false);
  });

  test('a clipboard too large to scrub has no retry and leaves on its own', async () => {
    await backend.emit('clipboard-scrubbed', result('tooLarge'));
    const notice = toasts.items.at(-1)!;
    expect(notice.duration).toBeGreaterThan(0);
    expect(notice.action).toBeUndefined();
  });

  test('an empty clipboard says there was no text', async () => {
    await backend.emit('clipboard-scrubbed', result('empty'));
    expect(toasts.items.at(-1)).toMatchObject({
      tone: 'info',
      title: 'Nothing to scrub',
      detail: 'The clipboard holds no text.',
    });
  });

  test('the earlier results keep their toasts', async () => {
    await backend.emit('clipboard-scrubbed', { ...result('cleaned', 2), historyCleaned: true });
    expect(toasts.items.at(-1)).toMatchObject({
      tone: 'success',
      title: 'Clipboard scrubbed',
      detail: '2 secrets removed · history cleaned',
    });
    await backend.emit('clipboard-scrubbed', result('tooLarge'));
    expect(toasts.items.at(-1)).toMatchObject({
      tone: 'warning',
      title: 'Left alone',
      detail: 'The clipboard text is larger than 20 MiB.',
    });
  });
});

describe('a scrub that waits for the window', () => {
  test('the wait shows until the result replaces it', async () => {
    await backend.emit('clipboard-scrub-waiting', null);
    expect(toasts.items).toHaveLength(1);
    expect(toasts.items[0]).toMatchObject({
      title: "Waiting for Redactor's window to clean the clipboard…",
      duration: 0,
    });
    await backend.emit('clipboard-scrubbed', result('clean'));
    expect(titles()).toEqual(['Clipboard is clean']);
  });

  test('an expired wait stays until dismissed and offers Clean now', async () => {
    await backend.emit('clipboard-scrub-waiting', null);
    await backend.emit('clipboard-scrub-expired', null);
    expect(toasts.items).toHaveLength(1);
    expect(toasts.items[0]).toMatchObject({
      tone: 'warning',
      title: 'Not cleaned',
      detail: "Redactor's window did not come to the front in time.",
      duration: 0,
    });
    expect(toasts.items[0]?.action?.label).toBe('Clean now');
  });

  test('Clean now scrubs the clipboard through the shell and closes the notice', async () => {
    await backend.emit('clipboard-scrub-expired', null);
    const notice = toasts.items[0]!;
    toasts.act(notice.id);
    expect(backend.calls.at(-1)?.cmd).toBe('scrub_clipboard');
    expect(toasts.items.some((t) => t.id === notice.id)).toBe(false);
  });

  test('a new press replaces an expired notice', async () => {
    await backend.emit('clipboard-scrub-expired', null);
    await backend.emit('clipboard-scrub-waiting', null);
    expect(titles()).toEqual(["Waiting for Redactor's window to clean the clipboard…"]);
  });

  test('an expiry told twice (the timer, then the next focus) stays one toast', async () => {
    await backend.emit('clipboard-scrub-waiting', null);
    await backend.emit('clipboard-scrub-expired', null);
    await backend.emit('clipboard-scrub-expired', null);
    expect(titles()).toEqual(['Not cleaned']);
  });

  test('a second expiry leaves the notice on screen untouched', async () => {
    await backend.emit('clipboard-scrub-expired', null);
    const first = toasts.items[0]!.id;
    await backend.emit('clipboard-scrub-expired', null);
    expect(toasts.items.map((t) => t.id)).toEqual([first]);
  });

  test('an expiry after the notice was dismissed shows again', async () => {
    await backend.emit('clipboard-scrub-expired', null);
    toasts.dismiss(toasts.items[0]!.id);
    await backend.emit('clipboard-scrub-expired', null);
    expect(titles()).toEqual(['Not cleaned']);
  });

  test('an expiry after a result replaced the notice shows again', async () => {
    await backend.emit('clipboard-scrub-expired', null);
    await backend.emit('clipboard-scrubbed', result('clean'));
    await backend.emit('clipboard-scrub-expired', null);
    expect(titles()).toEqual(['Clipboard is clean', 'Not cleaned']);
  });

  test('an expiry after a new wait replaced the notice shows again', async () => {
    await backend.emit('clipboard-scrub-expired', null);
    await backend.emit('clipboard-scrub-waiting', null);
    await backend.emit('clipboard-scrub-expired', null);
    expect(titles()).toEqual(['Not cleaned']);
  });
});

describe('a waiting notice that comes too late', () => {
  afterEach(() => vi.useRealTimers());

  test('cannot overwrite the result that just arrived', async () => {
    vi.useFakeTimers();
    await backend.emit('clipboard-scrubbed', result('clean'));
    await backend.emit('clipboard-scrub-waiting', null);
    expect(titles()).toEqual(['Clipboard is clean']);
  });

  test('a press a moment later than the last result still waits', async () => {
    vi.useFakeTimers();
    await backend.emit('clipboard-scrubbed', result('clean'));
    vi.advanceTimersByTime(2000);
    await backend.emit('clipboard-scrub-waiting', null);
    expect(titles()).toContain("Waiting for Redactor's window to clean the clipboard…");
  });
});
