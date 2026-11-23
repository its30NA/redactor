/**
 * Texts that name the OS, and what the desktop can do. Each system keeps its own words; the
 * Windows texts are the ones the app had before the port, byte for byte.
 */
import type { KeyOs } from './keys';
import type { AppInfo, HotkeyMode } from './types';

const pick = (os: KeyOs, windows: string, linux: string, macos: string): string =>
  os === 'macos' ? macos : os === 'linux' ? linux : windows;

export const lockTitle = (os: KeyOs): string =>
  pick(os, 'Wipe when Windows locks', 'Wipe when the screen locks', 'Wipe when the Mac locks');

export const lockDesc = (os: KeyOs): string =>
  pick(
    os,
    'Win+L or the screen saver lock.',
    "The desktop's screen lock, or suspend.",
    'The lock screen, display sleep, or sleep.',
  );

export const lockShort = (os: KeyOs): string =>
  pick(os, 'when Windows locks', 'when the screen locks', 'when the Mac locks');

export const wipeNotice = (reason: 'lock' | 'sleep', os: KeyOs): string =>
  reason === 'lock'
    ? pick(
        os,
        'Windows was locked. Both panes are empty.',
        'The screen was locked. Both panes are empty.',
        'The Mac was locked. Both panes are empty.',
      )
    : pick(
        os,
        'Windows went to sleep. Both panes are empty.',
        'The computer went to sleep. Both panes are empty.',
        'The Mac went to sleep. Both panes are empty.',
      );

export const themeDesc = (os: KeyOs): string =>
  pick(os, 'Follows Windows by default.', 'Follows the desktop by default.', 'Follows macOS by default.');

/** The tray is the menu bar on macOS. */
const TRAY: Record<KeyOs, string> = { windows: 'tray', linux: 'tray', macos: 'menu bar' };

export const trayTitle = (os: KeyOs): string => `Keep running in the ${TRAY[os]}`;

export const traySection = (os: KeyOs): string => `Hotkey and ${TRAY[os]}`;

export const trayWipeNotice = (os: KeyOs): string => `From the ${TRAY[os]}. Both panes are empty.`;

/** "this PC", or "this Mac" on macOS: where settings and the always-redact list are stored. */
export const thisComputer = (os: KeyOs): string => (os === 'macos' ? 'this Mac' : 'this PC');

/** The Settings sheet's subtitle. */
export const settingsNote = (os: KeyOs): string => `Stored on ${thisComputer(os)} only. Never any text.`;

/** Why the screen-capture shield is off and cannot be turned on, or null when it works (or app_info has not answered). */
export const shieldNote = (info: Pick<AppInfo, 'shield'> | null, os: KeyOs): string | null =>
  info?.shield !== false
    ? null
    : os === 'macos'
      ? 'Not available on macOS: since macOS 15, screen capture ignores it.'
      : 'Not available on Linux: the desktop cannot hide a window from screenshots.';

/** Why "Keep running in the tray" is off and cannot be turned on, or null when there is a tray. */
export const trayNote = (info: Pick<AppInfo, 'tray'> | null): string | null =>
  info?.tray === false ? 'This desktop shows no tray, so closing the window quits Redactor.' : null;

/** How a hotkey press reaches the app; `keys` where app_info does not say (Windows, macOS) or has not answered. */
export const hotkeyMode = (info: Pick<AppInfo, 'hotkey'> | null): HotkeyMode => info?.hotkey ?? 'keys';

/** What the desktop shortcut runs where the app cannot hold a key itself. */
export const SCRUB_COMMAND = 'redactor --scrub-clipboard';

/** How the hotkey works on this desktop, or null where the app registers the key itself. */
export const hotkeyNote = (info: Pick<AppInfo, 'hotkey'> | null): string | null => {
  switch (hotkeyMode(info)) {
    case 'portal':
      return 'Your desktop may ask you to confirm this key, or let you pick another in its own keyboard settings.';
    case 'command':
      return 'This desktop does not let apps set a hotkey. Add a shortcut in its keyboard settings that runs:';
    default:
      return null;
  }
};

/** The welcome card's line where the key does nothing and the desktop runs the command instead, or null where the keycaps are shown. */
export const hotkeyCommandLine = (info: Pick<AppInfo, 'hotkey'> | null): string | null =>
  hotkeyMode(info) === 'command'
    ? "Clean the clipboard with a shortcut you add in your desktop's keyboard settings (see Settings)."
    : null;

/** Why the hotkey brings the window forward (the row above says that it does), or null where the clipboard works in the background. */
export const focusNote = (info: Pick<AppInfo, 'clipboardNeedsFocus'> | null): string | null =>
  info?.clipboardNeedsFocus === true ? 'Redactor can reach the clipboard only while its window is in front.' : null;

/** The Settings row "Scrub the clipboard": where the clipboard needs focus, the window comes to the front. */
export const scrubRowDesc = (info: Pick<AppInfo, 'clipboardNeedsFocus'> | null): string =>
  info?.clipboardNeedsFocus === true
    ? 'From any app. On this desktop the window comes to the front first.'
    : 'From any app, without opening the window.';

/** The welcome card's line under the keycaps: where the clipboard needs focus, the window comes forward. */
export const firstRunHotkeyDesc = (info: Pick<AppInfo, 'clipboardNeedsFocus'> | null): string =>
  info?.clipboardNeedsFocus === true
    ? 'Brings this window forward, then cleans the clipboard.'
    : 'Cleans the clipboard in place, without opening this window.';
