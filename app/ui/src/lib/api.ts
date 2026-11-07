/**
 * The only module that talks to the Rust shell.
 *
 * One async function per IPC command. Command names are the snake_case Rust function
 * names; arguments are camelCase (Tauri's default mapping of Rust parameter names). Components
 * import from here and never call `invoke` directly, so tests can swap the backend with `mockIPC`.
 */
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type {
  AppEventName,
  AppEvents,
  AppInfo,
  ClipboardScrub,
  OpenedFile,
  ScanRequest,
  ScanResponse,
  SessionSecret,
  Settings,
} from './types';

/** Whitelisted command names, as registered by `app/src-tauri` (and listed in its capabilities). */
export const COMMANDS = {
  scan: 'scan',
  scrubClipboard: 'scrub_clipboard',
  readClipboard: 'read_clipboard',
  copyText: 'copy_text',
  openTextFile: 'open_text_file',
  saveTextFile: 'save_text_file',
  sessionList: 'session_list',
  sessionRemember: 'session_remember',
  sessionAdd: 'session_add',
  sessionForget: 'session_forget',
  sessionClear: 'session_clear',
  getSettings: 'get_settings',
  setSettings: 'set_settings',
  appInfo: 'app_info',
} as const;

export type CommandName = (typeof COMMANDS)[keyof typeof COMMANDS];

/** Scan text. Input over 20 MiB comes back unscanned with an `inputTooLarge` warning. */
export const scan = (req: ScanRequest): Promise<ScanResponse> => invoke(COMMANDS.scan, { req });

/** Scrub the clipboard in place (also bound to the global hotkey in the shell). */
export const scrubClipboard = (): Promise<ClipboardScrub> => invoke(COMMANDS.scrubClipboard);

/** Clipboard text, or null when it is empty, not text, or marked by a password manager. */
export const readClipboard = (): Promise<string | null> => invoke(COMMANDS.readClipboard);

export const copyText = (text: string): Promise<void> => invoke(COMMANDS.copyText, { text });

/** Native open dialog; null when cancelled. */
export const openTextFile = (): Promise<OpenedFile | null> => invoke(COMMANDS.openTextFile);

/** Native save dialog; false when cancelled. */
export const saveTextFile = (text: string, suggestedName: string): Promise<boolean> =>
  invoke(COMMANDS.saveTextFile, { text, suggestedName });

/*
 * Session secrets (memory only in the shell; wiped on quit, lock, idle and the `wipe` event).
 * Each call answers with the full list. The value never comes back: only id, label and time, not even its length.
 */

export const sessionList = (): Promise<SessionSecret[]> => invoke(COMMANDS.sessionList);

/** Remember the text between two UTF-16 offsets of `text` (a selection or a finding) without the UI slicing it out. */
export const sessionRemember = (text: string, start: number, end: number, label: string): Promise<SessionSecret[]> =>
  invoke(COMMANDS.sessionRemember, { text, start, end, label });

/** Remember a secret typed into the masked field (at least 6 characters). */
export const sessionAdd = (secret: string, label: string): Promise<SessionSecret[]> =>
  invoke(COMMANDS.sessionAdd, { secret, label });

export const sessionForget = (id: string): Promise<SessionSecret[]> => invoke(COMMANDS.sessionForget, { id });

export const sessionClear = (): Promise<SessionSecret[]> => invoke(COMMANDS.sessionClear);

export const getSettings = (): Promise<Settings> => invoke(COMMANDS.getSettings);

export const setSettings = (settings: Settings): Promise<Settings> =>
  invoke(COMMANDS.setSettings, { settings });

export const appInfo = (): Promise<AppInfo> => invoke(COMMANDS.appInfo);

/** Subscribe to an event the shell emits. Resolves to the unsubscribe function. */
export async function onEvent<K extends AppEventName>(
  name: K,
  cb: (payload: AppEvents[K]) => void,
): Promise<UnlistenFn> {
  return listen<AppEvents[K]>(name, (e) => cb(e.payload));
}

/** True only inside the real Tauri shell (`mockIPC` does not set it). */
export function isDesktop(): boolean {
  return isTauri();
}

/** Controls for the frameless window's custom title bar. */
export const windowControls = {
  minimize: (): Promise<void> => getCurrentWindow().minimize(),
  toggleMaximize: (): Promise<void> => getCurrentWindow().toggleMaximize(),
  close: (): Promise<void> => getCurrentWindow().close(),
  isMaximized: (): Promise<boolean> => getCurrentWindow().isMaximized(),
  onResized: (cb: () => void): Promise<UnlistenFn> => getCurrentWindow().onResized(() => cb()),
};
