/**
 * App settings, loaded from and saved to the shell (`get_settings` / `set_settings`).
 * Updates are optimistic: the UI changes at once and rolls back if the shell refuses.
 */
import { getSettings, setSettings } from './api';
import { toasts } from './toast.svelte';
import type { Settings } from './types';

export const DEFAULT_SETTINGS: Settings = {
  theme: 'system',
  sensitivity: 'balanced',
  pii: false,
  stripInvisible: true,
  hotkey: 'Ctrl+Alt+R',
  trayMode: true,
  autoWipeMinutes: 10,
  wipeOnLock: true,
  contentProtection: true,
  placeholderStyle: 'labeled',
  reducedMotion: 'system',
  alwaysRedact: [],
  firstRunDone: false,
};

class SettingsStore {
  value = $state<Settings>({ ...DEFAULT_SETTINGS });
  loaded = $state(false);

  async load(): Promise<void> {
    try {
      this.value = await getSettings();
    } catch {
      toasts.show({ tone: 'warning', title: 'Settings could not be loaded', detail: 'Using defaults.' });
    } finally {
      this.loaded = true;
    }
  }

  async update(patch: Partial<Settings>): Promise<void> {
    const before = this.value;
    const next = { ...before, ...patch };
    this.value = next;
    try {
      this.value = await setSettings(next);
    } catch (err) {
      this.value = before;
      // The shell rejects with a plain message (for example a hotkey another app holds).
      const detail = typeof err === 'string' && err.trim() ? err : undefined;
      toasts.show({ tone: 'error', title: 'Setting not saved', detail });
    }
  }
}

export const settings = new SettingsStore();
