/**
 * Texts that name the OS, and what the desktop can do. Windows keeps its own words;
 * macOS uses the Linux wording until it gets its own.
 */
import type { KeyOs } from './keys';
import type { AppInfo } from './types';

const pick = (os: KeyOs, windows: string, other: string): string => (os === 'windows' ? windows : other);

export const lockTitle = (os: KeyOs): string => pick(os, 'Wipe when Windows locks', 'Wipe when the screen locks');

export const lockDesc = (os: KeyOs): string =>
  pick(os, 'Win+L or the screen saver lock.', "The desktop's screen lock, or suspend.");

export const lockShort = (os: KeyOs): string => pick(os, 'when Windows locks', 'when the screen locks');

export const wipeNotice = (reason: 'lock' | 'sleep', os: KeyOs): string =>
  reason === 'lock'
    ? pick(os, 'Windows was locked. Both panes are empty.', 'The screen was locked. Both panes are empty.')
    : pick(os, 'Windows went to sleep. Both panes are empty.', 'The computer went to sleep. Both panes are empty.');

export const themeDesc = (os: KeyOs): string => pick(os, 'Follows Windows by default.', 'Follows the desktop by default.');

/** Why the screen-capture shield is off and cannot be turned on, or null when it works (or app_info has not answered). */
export const shieldNote = (info: Pick<AppInfo, 'shield'> | null): string | null =>
  info?.shield === false ? 'Not available on Linux: the desktop cannot hide a window from screenshots.' : null;

/** Why "Keep running in the tray" is off and cannot be turned on, or null when there is a tray. */
export const trayNote = (info: Pick<AppInfo, 'tray'> | null): string | null =>
  info?.tray === false ? 'This desktop shows no tray, so closing the window quits Redactor.' : null;
