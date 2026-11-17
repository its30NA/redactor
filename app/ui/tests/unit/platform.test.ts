import { describe, expect, test } from 'vitest';
import type { KeyOs } from '../../src/lib/keys';
import {
  focusNote,
  hotkeyCommandLine,
  hotkeyMode,
  hotkeyNote,
  lockDesc,
  lockShort,
  lockTitle,
  settingsNote,
  shieldNote,
  themeDesc,
  thisComputer,
  trayNote,
  traySection,
  trayTitle,
  trayWipeNotice,
  wipeNotice,
  SCRUB_COMMAND,
} from '../../src/lib/platform';
import type { AppInfo } from '../../src/lib/types';

const info = (flags: Partial<AppInfo> = {}): AppInfo => ({
  version: '2.0.0',
  engineVersion: '2.0.0',
  ruleCount: 175,
  platform: 'linux',
  tray: true,
  shield: true,
  ...flags,
});

describe('platform texts', () => {
  // name, function, Windows (today's text, byte for byte), Linux, macOS
  const table: [string, (os: KeyOs) => string, string, string, string][] = [
    ['lockTitle', lockTitle, 'Wipe when Windows locks', 'Wipe when the screen locks', 'Wipe when the Mac locks'],
    [
      'lockDesc',
      lockDesc,
      'Win+L or the screen saver lock.',
      "The desktop's screen lock, or suspend.",
      'The lock screen, display sleep, or sleep.',
    ],
    ['lockShort', lockShort, 'when Windows locks', 'when the screen locks', 'when the Mac locks'],
    [
      "wipeNotice('lock')",
      (os) => wipeNotice('lock', os),
      'Windows was locked. Both panes are empty.',
      'The screen was locked. Both panes are empty.',
      'The Mac was locked. Both panes are empty.',
    ],
    [
      "wipeNotice('sleep')",
      (os) => wipeNotice('sleep', os),
      'Windows went to sleep. Both panes are empty.',
      'The computer went to sleep. Both panes are empty.',
      'The Mac went to sleep. Both panes are empty.',
    ],
    [
      'themeDesc',
      themeDesc,
      'Follows Windows by default.',
      'Follows the desktop by default.',
      'Follows macOS by default.',
    ],
    ['trayTitle', trayTitle, 'Keep running in the tray', 'Keep running in the tray', 'Keep running in the menu bar'],
    ['traySection', traySection, 'Hotkey and tray', 'Hotkey and tray', 'Hotkey and menu bar'],
    [
      'trayWipeNotice',
      trayWipeNotice,
      'From the tray. Both panes are empty.',
      'From the tray. Both panes are empty.',
      'From the menu bar. Both panes are empty.',
    ],
    ['thisComputer', thisComputer, 'this PC', 'this PC', 'this Mac'],
    [
      'settingsNote',
      settingsNote,
      'Stored on this PC only. Never any text.',
      'Stored on this PC only. Never any text.',
      'Stored on this Mac only. Never any text.',
    ],
  ];

  for (const [name, fn, windows, linux, macos] of table) {
    test(`${name} on Windows keeps today's text`, () => expect(fn('windows')).toBe(windows));
    test(`${name} on Linux`, () => expect(fn('linux')).toBe(linux));
    test(`${name} on macOS`, () => expect(fn('macos')).toBe(macos));
  }
});

describe('shieldNote', () => {
  const os: KeyOs[] = ['windows', 'linux', 'macos'];
  for (const o of os) {
    test(`no note when the shield works (${o})`, () => expect(shieldNote(info({ shield: true }), o)).toBeNull());
    test(`no note before app_info answers (${o})`, () => expect(shieldNote(null, o)).toBeNull());
  }
  test('Linux: the desktop cannot hide the window', () =>
    expect(shieldNote(info({ shield: false }), 'linux')).toBe(
      'Not available on Linux: the desktop cannot hide a window from screenshots.',
    ));
  test('macOS: screen capture ignores it', () =>
    expect(shieldNote(info({ shield: false }), 'macos')).toBe(
      'Not available on macOS: since macOS 15, screen capture ignores it.',
    ));
});

describe('trayNote', () => {
  test('no note when there is a tray', () => expect(trayNote(info({ tray: true }))).toBeNull());
  test('says what closing does without a tray', () =>
    expect(trayNote(info({ tray: false }))).toBe('This desktop shows no tray, so closing the window quits Redactor.'));
  test('no note before app_info answers', () => expect(trayNote(null)).toBeNull());
});

describe('hotkeyMode', () => {
  test('missing field is keys (Windows and macOS)', () => expect(hotkeyMode(info())).toBe('keys'));
  test('before app_info answers it is keys', () => expect(hotkeyMode(null)).toBe('keys'));
  for (const m of ['keys', 'portal', 'command'] as const) {
    test(`reads ${m}`, () => expect(hotkeyMode(info({ hotkey: m }))).toBe(m));
  }
});

describe('hotkeyNote', () => {
  test('keys: no note', () => expect(hotkeyNote(info({ hotkey: 'keys' }))).toBeNull());
  test('missing field: no note', () => expect(hotkeyNote(info())).toBeNull());
  test('before app_info answers: no note', () => expect(hotkeyNote(null)).toBeNull());
  test('portal: the desktop may ask', () =>
    expect(hotkeyNote(info({ hotkey: 'portal' }))).toBe(
      'Your desktop may ask you to confirm this key, or let you pick another in its own keyboard settings.',
    ));
  test('command: add a desktop shortcut', () =>
    expect(hotkeyNote(info({ hotkey: 'command' }))).toBe(
      'This desktop does not let apps set a hotkey. Add a shortcut in its keyboard settings that runs:',
    ));
  test('the command the shortcut runs', () => expect(SCRUB_COMMAND).toBe('redactor --scrub-clipboard'));
});

describe('focusNote', () => {
  test('needs focus: says so', () =>
    expect(focusNote(info({ clipboardNeedsFocus: true }))).toBe(
      'On this desktop Redactor can reach the clipboard only while its window is in front. The hotkey brings it forward first.',
    ));
  test('no need: no note', () => expect(focusNote(info({ clipboardNeedsFocus: false }))).toBeNull());
  test('missing field: no note', () => expect(focusNote(info())).toBeNull());
  test('before app_info answers: no note', () => expect(focusNote(null)).toBeNull());
});

describe('hotkeyCommandLine', () => {
  test('command: the welcome card points to the desktop settings', () =>
    expect(hotkeyCommandLine(info({ hotkey: 'command' }))).toBe(
      "Clean the clipboard with a shortcut you add in your desktop's keyboard settings (see Settings).",
    ));
  test('keys, portal, missing and before app_info: null (the keycaps stay)', () => {
    expect(hotkeyCommandLine(info({ hotkey: 'keys' }))).toBeNull();
    expect(hotkeyCommandLine(info({ hotkey: 'portal' }))).toBeNull();
    expect(hotkeyCommandLine(info())).toBeNull();
    expect(hotkeyCommandLine(null)).toBeNull();
  });
});
