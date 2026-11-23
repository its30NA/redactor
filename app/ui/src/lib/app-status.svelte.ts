/**
 * The shell's `app-status` event: whether the hotkey works now (Settings shows it under the key)
 * and the notices the window shows once: a hotkey that could not be set at start or that the
 * desktop dropped, settings that could not be written or read. Each also comes as a desktop
 * notification where the shell sends one; these stay until dismissed, since the window may not
 * be in front when they arrive.
 */
import type { UnlistenFn } from '@tauri-apps/api/event';
import { onEvent } from './api';
import { toasts, type ToastInput } from './toast.svelte';
import type { AppNotice, AppStatus } from './types';

class HotkeyState {
  /** From the latest `app-status`; null until one comes (app_info's answer holds until then). */
  active = $state<boolean | null>(null);
}

export const hotkeyState = new HotkeyState();

const NOTICES: Record<AppNotice, Omit<ToastInput, 'action'> & { settings?: true }> = {
  hotkeyNotActive: {
    tone: 'warning',
    title: 'Hotkey not active',
    detail: 'Another app holds it, or the desktop refused it. Pick another key in Settings.',
    settings: true,
  },
  hotkeyLost: {
    tone: 'warning',
    title: 'Hotkey stopped working',
    detail: 'The desktop closed its shortcut. Set it again in Settings.',
    settings: true,
  },
  settingsNotSaved: {
    tone: 'warning',
    title: 'Settings not written to disk',
    detail: 'They apply now and hold until Redactor closes.',
  },
  settingsUnreadable: {
    tone: 'warning',
    title: 'Settings could not be read',
    detail: 'Redactor started with the defaults and kept the old file as settings.json.bad.',
  },
};

const isNotice = (n: unknown): n is AppNotice => typeof n === 'string' && Object.hasOwn(NOTICES, n);

/** Subscribes to `app-status`; `openSettings` is the hotkey notices' button. */
export function listenForAppStatus(openSettings: () => void): Promise<UnlistenFn> {
  hotkeyState.active = null;
  // Events are not validated at runtime: a payload that is not what the shell sends changes nothing.
  return onEvent('app-status', (s: AppStatus) => {
    if (typeof s !== 'object' || s === null) return;
    if (typeof s.hotkeyActive === 'boolean') hotkeyState.active = s.hotkeyActive;
    if (!isNotice(s.notice)) return;
    const { settings, ...notice } = NOTICES[s.notice];
    toasts.show({
      ...notice,
      duration: 0,
      action: settings ? { label: 'Open Settings', run: openSettings } : undefined,
    });
  });
}
