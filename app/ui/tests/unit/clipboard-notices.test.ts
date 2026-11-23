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
import type { ClipboardStatus, ScrubExpired, ScrubResult, ScrubWaiting } from '../../src/lib/types';

let backend: MockBackend;
let unlisten: UnlistenFn[];

/** A result; `press` is the latest press its scrub covers. */
const result = (status: ClipboardStatus, removed = 0, press = 1): ScrubResult => ({
  status,
  removed,
  byLabel: [],
  historyCleaned: false,
  press,
});

const WAITING = "Waiting for Redactor's window to clean the clipboard…";

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

  test('a later result replaces a failed one', async () => {
    await backend.emit('clipboard-scrubbed', result('error', 0, 1));
    await backend.emit('clipboard-scrubbed', result('cleaned', 1, 2));
    expect(titles()).toEqual(['Clipboard scrubbed']);
    // A later failure replaces an earlier one: one notice, not two.
    await backend.emit('clipboard-scrubbed', result('error', 0, 3));
    await backend.emit('clipboard-scrubbed', result('error', 0, 4));
    expect(titles()).toEqual(['Clipboard scrubbed', 'Clipboard not scrubbed']);
  });

  test('a result of the same press leaves the failed notice', async () => {
    await backend.emit('clipboard-scrubbed', result('error', 0, 2));
    await backend.emit('clipboard-scrubbed', result('clean', 0, 1));
    expect(titles()).toEqual(['Clipboard not scrubbed', 'Clipboard is clean']);
  });

  test('a failed scrub older than 10 min is gone', async () => {
    vi.useFakeTimers();
    try {
      await backend.emit('clipboard-scrubbed', result('error'));
      vi.advanceTimersByTime(10 * 60_000 - 1);
      expect(titles()).toEqual(['Clipboard not scrubbed']);
      vi.advanceTimersByTime(1);
      expect(titles()).toEqual([]);
    } finally {
      vi.useRealTimers();
    }
  });

  test('an unverified scan that removed nothing does not claim a scrub', async () => {
    await backend.emit('clipboard-scrubbed', result('unverified', 0));
    expect(toasts.items.at(-1)).toMatchObject({
      tone: 'warning',
      title: 'Check before pasting',
      detail: 'Nothing was removed, but the scan could not be verified.',
    });
    await backend.emit('clipboard-scrubbed', { ...result('unverified', 2, 2), historyCleaned: true });
    expect(toasts.items.at(-1)).toMatchObject({
      tone: 'warning',
      title: 'Clipboard scrubbed',
      detail: '2 secrets removed · check before pasting · history cleaned',
    });
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
    await backend.emit('clipboard-scrub-waiting', { press: 1 });
    expect(toasts.items).toHaveLength(1);
    expect(toasts.items[0]).toMatchObject({
      title: WAITING,
      duration: 0,
    });
    await backend.emit('clipboard-scrubbed', result('clean'));
    expect(titles()).toEqual(['Clipboard is clean']);
  });

  test('an expired wait stays until dismissed and offers Clean now', async () => {
    await backend.emit('clipboard-scrub-waiting', { press: 1 });
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
    await backend.emit('clipboard-scrub-waiting', { press: 1 });
    expect(titles()).toEqual([WAITING]);
  });

  test('an expiry told twice (the timer, then the next focus) stays one toast', async () => {
    await backend.emit('clipboard-scrub-waiting', { press: 1 });
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
    await backend.emit('clipboard-scrub-waiting', { press: 1 });
    await backend.emit('clipboard-scrub-expired', null);
    expect(titles()).toEqual(['Not cleaned']);
  });
});

describe('a waiting notice that comes too late', () => {
  test('cannot overwrite the result that covers its press', async () => {
    await backend.emit('clipboard-scrubbed', result('clean', 0, 2));
    await backend.emit('clipboard-scrub-waiting', { press: 2 });
    await backend.emit('clipboard-scrub-waiting', { press: 1 });
    expect(titles()).toEqual(['Clipboard is clean']);
  });

  test('a press after the last result still waits, however soon', async () => {
    await backend.emit('clipboard-scrubbed', result('clean', 0, 1));
    await backend.emit('clipboard-scrub-waiting', { press: 2 });
    expect(titles()).toEqual(['Clipboard is clean', WAITING]);
  });
});

describe('a result of an earlier scrub', () => {
  test('leaves the wait for a later press on screen', async () => {
    // A slow scrub of clipboard A runs; the user copies B and presses again.
    await backend.emit('clipboard-scrub-waiting', { press: 2 });
    await backend.emit('clipboard-scrubbed', result('clean', 0, 1));
    expect(titles()).toEqual([WAITING, 'Clipboard is clean']);
    // B's own scrub ends the wait.
    await backend.emit('clipboard-scrubbed', result('cleaned', 1, 2));
    expect(titles()).toEqual(['Clipboard is clean', 'Clipboard scrubbed']);
  });

  test('leaves the expiry of a later press on screen', async () => {
    await backend.emit('clipboard-scrub-waiting', { press: 2 });
    await backend.emit('clipboard-scrub-expired', null);
    await backend.emit('clipboard-scrubbed', result('clean', 0, 1));
    expect(titles()).toEqual(['Not cleaned', 'Clipboard is clean']);
  });

  test('a scrub that covers later presses too ends the wait', async () => {
    await backend.emit('clipboard-scrub-waiting', { press: 2 });
    await backend.emit('clipboard-scrubbed', result('clean', 0, 3));
    expect(titles()).toEqual(['Clipboard is clean']);
  });
});

describe('an expiry that names its press', () => {
  test('an expiry of an earlier press leaves the wait for a newer press on screen', async () => {
    await backend.emit('clipboard-scrub-waiting', { press: 1 });
    await backend.emit('clipboard-scrub-waiting', { press: 2 });
    await backend.emit('clipboard-scrub-expired', { press: 1 });
    expect(titles()).toEqual([WAITING]);
  });

  test('an expiry of a press a result already covered is no news', async () => {
    await backend.emit('clipboard-scrub-waiting', { press: 1 });
    await backend.emit('clipboard-scrubbed', result('clean', 0, 1));
    await backend.emit('clipboard-scrub-expired', { press: 1 });
    expect(titles()).toEqual(['Clipboard is clean']);
  });

  test('an expiry of the press that waits replaces the wait', async () => {
    await backend.emit('clipboard-scrub-waiting', { press: 2 });
    await backend.emit('clipboard-scrub-expired', { press: 2 });
    expect(titles()).toEqual(['Not cleaned']);
    // Its wait, told late, does not come back.
    await backend.emit('clipboard-scrub-waiting', { press: 2 });
    expect(titles()).toEqual(['Not cleaned']);
  });

  test('a result of that press ends the expiry notice', async () => {
    await backend.emit('clipboard-scrub-expired', { press: 3 });
    await backend.emit('clipboard-scrubbed', result('clean', 0, 2));
    expect(titles()).toEqual(['Not cleaned', 'Clipboard is clean']);
    await backend.emit('clipboard-scrubbed', result('cleaned', 1, 3));
    expect(titles()).toEqual(['Clipboard is clean', 'Clipboard scrubbed']);
  });
});

describe('a waiting notice out of order', () => {
  test('a wait for an earlier press cannot take the place of a later one', async () => {
    await backend.emit('clipboard-scrub-waiting', { press: 3 });
    await backend.emit('clipboard-scrub-waiting', { press: 2 });
    // The result of press 2 must not end the wait of press 3.
    await backend.emit('clipboard-scrubbed', result('clean', 0, 2));
    expect(titles()).toEqual([WAITING, 'Clipboard is clean']);
    await backend.emit('clipboard-scrubbed', result('clean', 0, 3));
    expect(titles()).toEqual(['Clipboard is clean', 'Clipboard is clean']);
  });
});

describe('a payload without a valid press', () => {
  const bad = [undefined, null, -1, 1.5, Number.NaN, '2', Number.MAX_SAFE_INTEGER + 2];

  for (const press of bad) {
    test(`a result with press ${String(press)} ends the wait and leaves the state sound`, async () => {
      await backend.emit('clipboard-scrub-waiting', { press: 2 });
      await backend.emit('clipboard-scrubbed', { ...result('clean'), press } as unknown as ScrubResult);
      expect(titles()).toEqual(['Clipboard is clean']);
      // The next press still waits and its result still ends the wait.
      await backend.emit('clipboard-scrub-waiting', { press: 3 });
      expect(titles()).toEqual(['Clipboard is clean', WAITING]);
      await backend.emit('clipboard-scrubbed', result('cleaned', 1, 3));
      expect(titles()).toEqual(['Clipboard is clean', 'Clipboard scrubbed']);
    });

    test(`a wait with press ${String(press)} shows nothing and breaks nothing`, async () => {
      await backend.emit('clipboard-scrub-waiting', { press } as unknown as ScrubWaiting);
      expect(titles()).toEqual([]);
      await backend.emit('clipboard-scrub-waiting', { press: 1 });
      await backend.emit('clipboard-scrubbed', result('clean', 0, 1));
      expect(titles()).toEqual(['Clipboard is clean']);
    });

    test(`an expiry with press ${String(press)} is told like one without a press`, async () => {
      await backend.emit('clipboard-scrub-waiting', { press: 1 });
      await backend.emit('clipboard-scrub-expired', { press } as unknown as ScrubExpired);
      expect(titles()).toEqual(['Not cleaned']);
      await backend.emit('clipboard-scrubbed', result('clean', 0, 1));
      expect(titles()).toEqual(['Clipboard is clean']);
    });
  }

  test('a wait with no payload at all is ignored', async () => {
    await backend.emit('clipboard-scrub-waiting', null as unknown as ScrubWaiting);
    expect(titles()).toEqual([]);
  });

  test('a failed result without a press is retired by the next result', async () => {
    await backend.emit('clipboard-scrubbed', { ...result('error'), press: undefined } as unknown as ScrubResult);
    await backend.emit('clipboard-scrubbed', result('clean', 0, 1));
    expect(titles()).toEqual(['Clipboard is clean']);
  });
});
