/**
 * The window's toasts for a hotkey, tray or `--scrub-clipboard` scrub. Where the clipboard needs
 * the window in front, a press first waits for it: the wait shows until the result or the expiry
 * replaces it. An expiry stays until dismissed (the desktop may drop its notification) and offers
 * "Clean now", an explicit scrub of whatever the clipboard holds then; so does a failed scrub.
 */
import type { UnlistenFn } from '@tauri-apps/api/event';
import { onEvent, scrubClipboard } from './api';
import { toasts, type ToastAction } from './toast.svelte';
import type { ClipboardScrub } from './types';

/** The wait or expiry toast on screen, if any: a newer press or result replaces it. */
let pending: number | null = null;

/**
 * The last expiry toast shown. The shell tells an expiry twice (at the timer, then at the next
 * focus); while the first notice is still on screen the second is not news, and replacing the
 * node could swallow the click on "Clean now" that brought the window to the front.
 */
let expired: number | null = null;

/**
 * How long after a result a waiting notice is too late to be news. The shell emits the notice
 * before it arms the scrub, so a result cannot normally come first; if events are reordered all
 * the same, the result wins and no wait toast is left on screen with nothing to replace it.
 */
const LATE_WAITING_MS = 1000;

/** When the last result arrived (`Date.now()`), 0 for none yet. */
let lastResultAt = 0;

function replacePending(id: number | null): void {
  if (pending !== null) toasts.dismiss(pending);
  pending = id;
}

/** An explicit scrub of whatever the clipboard holds now, for a scrub that did not happen. */
const cleanNow: ToastAction = {
  label: 'Clean now',
  run: () => void scrubClipboard().catch(() => toasts.show({ tone: 'error', title: 'Clipboard not scrubbed' })),
};

const secrets = (n: number) => `${n} ${n === 1 ? 'secret' : 'secrets'} removed`;

function showResult(s: ClipboardScrub): void {
  replacePending(null);
  lastResultAt = Date.now();
  const history = s.historyCleaned ? ' · history cleaned' : '';
  switch (s.status) {
    case 'cleaned':
      toasts.show({ tone: 'success', title: 'Clipboard scrubbed', detail: `${secrets(s.removed)}${history}` });
      break;
    case 'unverified':
      toasts.show({
        tone: 'warning',
        title: 'Clipboard scrubbed',
        detail: `${secrets(s.removed)} · check before pasting${history}`,
      });
      break;
    case 'clean':
      toasts.show({ tone: 'info', title: 'Clipboard is clean' });
      break;
    case 'empty':
      toasts.show({ tone: 'info', title: 'Nothing to scrub', detail: 'The clipboard holds no text.' });
      break;
    case 'protected':
      toasts.show({ tone: 'warning', title: 'Left alone', detail: 'A password manager marked that copy private.' });
      break;
    case 'tooLarge':
      toasts.show({ tone: 'warning', title: 'Left alone', detail: 'The clipboard text is larger than 20 MiB.' });
      break;
    case 'error':
      // The clipboard is still raw: stay until dismissed (the desktop may drop its notification).
      toasts.show({
        tone: 'error',
        title: 'Clipboard not scrubbed',
        detail: 'It could not be read, or it changed during the scrub.',
        duration: 0,
        action: cleanNow,
      });
      break;
  }
}

function showWaiting(): void {
  if (Date.now() - lastResultAt < LATE_WAITING_MS) return;
  replacePending(toasts.show({ tone: 'info', title: "Waiting for Redactor's window to clean the clipboard…", duration: 0 }));
}

function showExpired(): void {
  if (expired !== null && toasts.items.some((t) => t.id === expired)) return;
  expired = toasts.show({
    tone: 'warning',
    title: 'Not cleaned',
    detail: "Redactor's window did not come to the front in time.",
    duration: 0,
    action: cleanNow,
  });
  replacePending(expired);
}

/** Subscribes to the shell's scrub events. Resolves to the unsubscribe functions. */
export function listenForClipboardScrubs(): Promise<UnlistenFn>[] {
  pending = null;
  expired = null;
  lastResultAt = 0;
  return [
    onEvent('clipboard-scrubbed', showResult),
    onEvent('clipboard-scrub-waiting', showWaiting),
    onEvent('clipboard-scrub-expired', showExpired),
  ];
}
