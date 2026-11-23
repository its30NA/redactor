/**
 * The window's toasts for a hotkey, tray or `--scrub-clipboard` scrub. Where the clipboard needs
 * the window in front, a press first waits for it: the wait shows until the result or the expiry
 * replaces it. An expiry stays until dismissed (the desktop may drop its notification) and offers
 * "Scrub now", an explicit scrub of whatever the clipboard holds then; so does a failed scrub,
 * until a later result replaces it or it is ten minutes old.
 *
 * A result's words are the shell's notification's (`scrubWords`, pinned on both sides by
 * `app/src-tauri/tests/fixtures/scrub_notices.json`): the toast shows the title over the detail.
 */
import type { UnlistenFn } from '@tauri-apps/api/event';
import { onEvent, scrubClipboard } from './api';
import { toasts, type ToastAction, type ToastTone } from './toast.svelte';
import type { ClipboardStatus, ScrubExpired, ScrubResult, ScrubWaiting } from './types';

/** The wait or expiry toast on screen, if any: a newer press or result replaces it. */
let pending: number | null = null;

/**
 * The press that toast belongs to (an expiry belongs to the press that waited). Only a result
 * that covers it ends it: a slow scrub of an earlier clipboard leaves the wait for a later press.
 */
let pendingPress = 0;

/**
 * The last expiry toast shown. The shell tells an expiry twice (at the timer, then at the next
 * focus); while the first notice is still on screen the second is not news, and replacing the
 * node could swallow the click on "Scrub now" that brought the window to the front.
 */
let expired: number | null = null;

/**
 * The latest press a result covered. A waiting notice for it or an earlier press is no news: the
 * shell emits the notice before it arms the scrub, so a result cannot normally come first; if
 * events are reordered all the same, the result wins and no wait toast is left on screen with
 * nothing to replace it.
 */
let covered = 0;

/**
 * The "Clipboard not scrubbed" toast on screen, if any, and the press its scrub covered. A result
 * of a later press retires it (a later failure replaces it), and it leaves on its own after
 * `FAILED_FOR`, so a window that sat in the tray for hours does not show a stale warning.
 */
let failed: { id: number; press: number; timer: ReturnType<typeof setTimeout> } | null = null;

/** How long a failed scrub's toast stays at most: the shell's UNSEEN_FOR for an expiry. */
const FAILED_FOR = 10 * 60_000;

function retireFailed(): void {
  if (failed === null) return;
  clearTimeout(failed.timer);
  toasts.dismiss(failed.id);
  failed = null;
}

function replacePending(id: number | null): void {
  if (pending !== null) toasts.dismiss(pending);
  pending = id;
}

/**
 * An explicit scrub of whatever the clipboard holds now: the tray item's scrub, through the
 * shell. Its result comes as the `clipboard-scrubbed` event; only a refused call is told here.
 */
export function scrubClipboardNow(): void {
  void scrubClipboard().catch(() => toasts.show({ tone: 'error', title: 'Clipboard not scrubbed' }));
}

/** The same scrub as a toast's button, for a scrub that did not happen. */
const scrubNow: ToastAction = { label: 'Scrub now', run: scrubClipboardNow };

/**
 * The press an event names, or null when it names none (an expiry from a shell that does not
 * say) or the payload is not what the shell sends: events are not validated at runtime, and a
 * NaN in the state would leave a wait that no result ends.
 */
function pressOf(payload: unknown): number | null {
  if (typeof payload !== 'object' || payload === null) return null;
  const press = (payload as { press?: unknown }).press;
  return typeof press === 'number' && Number.isSafeInteger(press) && press >= 0 ? press : null;
}

const secrets = (n: number) => `${n} ${n === 1 ? 'secret' : 'secrets'} removed`;

/** What a scrub's outcome says: the title, and the sentence under it. */
export interface ScrubWords {
  title: string;
  detail: string;
}

/** A result in the words of the shell's notification (`ClipboardScrub::words`). */
export function scrubWords(s: ScrubResult): ScrubWords {
  const history = s.historyCleaned ? ' · history cleaned' : '';
  switch (s.status) {
    case 'cleaned':
      return { title: 'Clipboard scrubbed', detail: `${secrets(s.removed)}, safe to paste${history}` };
    case 'unverified':
      // Nothing removed: the shell left the clipboard as it was.
      return s.removed === 0
        ? { title: 'Check before pasting', detail: 'Nothing was removed, but the scan could not be verified.' }
        : { title: 'Clipboard scrubbed', detail: `${secrets(s.removed)}, check before pasting${history}` };
    case 'stripped':
      return { title: 'Hidden characters removed', detail: `Nothing sensitive found${history}` };
    case 'clean':
      return { title: 'Clipboard is clean', detail: 'Nothing sensitive found' };
    case 'empty':
      return { title: 'Nothing to scrub', detail: 'The clipboard holds no text.' };
    case 'protected':
      return { title: 'Left alone', detail: 'A password manager marked that copy private.' };
    case 'tooLarge':
      return { title: 'Left alone', detail: 'The clipboard text is larger than 20 MiB.' };
    case 'error':
      return { title: 'Clipboard not scrubbed', detail: 'It could not be read, or it changed during the scrub.' };
  }
}

/** A press that waited for the window in vain (the shell's `clipscrub::expired`). */
export const EXPIRED: ScrubWords = {
  title: 'Not scrubbed',
  detail: "Redactor's window did not come to the front in time.",
};

const TONE: Record<ClipboardStatus, ToastTone> = {
  cleaned: 'success',
  unverified: 'warning',
  stripped: 'success',
  clean: 'info',
  empty: 'info',
  protected: 'warning',
  tooLarge: 'warning',
  error: 'error',
};

function showResult(s: ScrubResult): void {
  if (typeof s !== 'object' || s === null) return;
  // A result that names no press is taken as the newest: it ends the wait rather than strand it.
  const press = pressOf(s);
  if (press !== null) covered = Math.max(covered, press);
  if (press === null || press >= pendingPress) replacePending(null);
  if (failed !== null && (press === null || press > failed.press || s.status === 'error')) retireFailed();
  // A status this page does not know (a newer shell) shows nothing rather than a blank toast.
  if (!Object.hasOwn(TONE, s.status)) return;
  const words = scrubWords(s);
  if (s.status !== 'error') {
    toasts.show({ tone: TONE[s.status], ...words });
    return;
  }
  // The clipboard is still raw: stay until dismissed (the desktop may drop its notification), a
  // later result, or FAILED_FOR.
  const id = toasts.show({ tone: 'error', ...words, duration: 0, action: scrubNow });
  const timer = setTimeout(() => {
    if (failed?.id === id) retireFailed();
  }, FAILED_FOR);
  failed = { id, press: press ?? covered, timer };
}

function showWaiting(w: ScrubWaiting): void {
  const press = pressOf(w);
  // No press, a press a result covered, or one older than (or the same as) the wait or expiry
  // on screen: a notice that came late or out of order is no news.
  if (press === null || press <= covered || press <= pendingPress) return;
  replacePending(toasts.show({ tone: 'info', title: "Waiting for Redactor's window to scrub the clipboard…", duration: 0 }));
  pendingPress = press;
}

/** `null` (or no valid press): a shell that does not name the press; the expiry is told as before. */
function showExpired(e: ScrubExpired | null): void {
  const press = pressOf(e);
  if (press !== null) {
    // A result covered that press, or a newer press waits: the expiry is no news.
    if (press <= covered || press < pendingPress) return;
    pendingPress = press;
  }
  if (expired !== null && toasts.items.some((t) => t.id === expired)) return;
  expired = toasts.show({ tone: 'warning', ...EXPIRED, duration: 0, action: scrubNow });
  replacePending(expired);
}

/** Subscribes to the shell's scrub events. Resolves to the unsubscribe functions. */
export function listenForClipboardScrubs(): Promise<UnlistenFn>[] {
  pending = null;
  pendingPress = 0;
  expired = null;
  covered = 0;
  if (failed !== null) clearTimeout(failed.timer);
  failed = null;
  return [
    onEvent('clipboard-scrubbed', showResult),
    onEvent('clipboard-scrub-waiting', showWaiting),
    onEvent('clipboard-scrub-expired', showExpired),
  ];
}
