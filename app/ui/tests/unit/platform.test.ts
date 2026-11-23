import { describe, expect, test } from 'vitest';
import type { KeyOs } from '../../src/lib/keys';
import {
  firstRunHotkeyDesc,
  focusNote,
  hotkeyCommandLine,
  hotkeyInactiveNote,
  hotkeyMode,
  hotkeyNote,
  lockDesc,
  lockNote,
  lockShort,
  lockTitle,
  scrubRowDesc,
  settingsNote,
  shieldNote,
  themeDesc,
  thisComputer,
  trayNote,
  traySection,
  trayTitle,
  trayWipeNotice,
  wipeNotice,
  scrubCommand,
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
      'Lock signals from logind or a screen saver, and suspend. Lockers that send none go unseen.',
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
});

describe('lockNote', () => {
  test('no lock source at all: a screen lock does not wipe', () =>
    expect(lockNote(info({ lockWatched: false }))).toBe(
      'No lock signal found on this desktop: a screen lock does not wipe. Suspend and the idle timer still do.',
    ));
  test('a lock source, a missing field and no app_info yet: no note', () => {
    expect(lockNote(info({ lockWatched: true }))).toBeNull();
    expect(lockNote(info())).toBeNull();
    expect(lockNote(null)).toBeNull();
  });
});

describe('hotkeyInactiveNote', () => {
  test('a key that does not work says so', () => {
    for (const hotkey of [undefined, 'keys', 'portal'] as const)
      expect(hotkeyInactiveNote(info({ hotkey, hotkeyActive: false }))).toBe('Not active: pick another key.');
  });
  test('a working key, a missing field and no app_info yet: no note', () => {
    expect(hotkeyInactiveNote(info({ hotkeyActive: true }))).toBeNull();
    expect(hotkeyInactiveNote(info())).toBeNull();
    expect(hotkeyInactiveNote(null)).toBeNull();
  });
  test('where the app holds no key there is nothing to pick', () =>
    expect(hotkeyInactiveNote(info({ hotkey: 'command', hotkeyActive: false }))).toBeNull());
});

describe('scrubCommand', () => {
  test('runs the program the shell names', () =>
    expect(scrubCommand(info({ executable: '/home/me/redactor/target/release/Redactor' }))).toBe(
      '/home/me/redactor/target/release/Redactor --scrub-clipboard',
    ));
  test('an AppImage path as the shell gives it', () =>
    expect(scrubCommand(info({ executable: '/home/me/Apps/Redactor_2.1.1_amd64.AppImage' }))).toBe(
      '/home/me/Apps/Redactor_2.1.1_amd64.AppImage --scrub-clipboard',
    ));
  test('a path with spaces is quoted', () =>
    expect(scrubCommand(info({ executable: '/home/me/My Apps/Redactor' }))).toBe(
      "'/home/me/My Apps/Redactor' --scrub-clipboard",
    ));
  test('a quote inside the path survives the quoting', () =>
    expect(scrubCommand(info({ executable: "/home/me/Ann's apps/Redactor" }))).toBe(
      "'/home/me/Ann'\\''s apps/Redactor' --scrub-clipboard",
    ));
  test('other shell characters are quoted too', () => {
    for (const odd of ['/opt/$HOME/Redactor', '/opt/a;b/Redactor', '/opt/a*b/Redactor', '/opt/tab\there/Redactor'])
      expect(scrubCommand(info({ executable: odd }))).toBe(`'${odd}' --scrub-clipboard`);
  });
  test('letters beyond ASCII need no quotes', () =>
    expect(scrubCommand(info({ executable: '/home/\u00e9t\u00e9/Redactor' }))).toBe(
      '/home/\u00e9t\u00e9/Redactor --scrub-clipboard',
    ));
  test('without a path (not Unicode, older shell, before app_info) the plain name', () => {
    expect(scrubCommand(info())).toBe('redactor --scrub-clipboard');
    expect(scrubCommand(null)).toBe('redactor --scrub-clipboard');
  });
});

describe('focusNote', () => {
  // The row above it says the window comes to the front; the note keeps only the why.
  test('needs focus: says why', () =>
    expect(focusNote(info({ clipboardNeedsFocus: true }))).toBe(
      'Redactor can reach the clipboard only while its window is in front.',
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

describe('scrubRowDesc', () => {
  test("no need for focus keeps today's text", () =>
    expect(scrubRowDesc(info({ clipboardNeedsFocus: false }))).toBe('From any app, without opening the window.'));
  test("missing field and before app_info keep today's text", () => {
    expect(scrubRowDesc(info())).toBe('From any app, without opening the window.');
    expect(scrubRowDesc(null)).toBe('From any app, without opening the window.');
  });
  test('needs focus: the window comes to the front first', () =>
    expect(scrubRowDesc(info({ clipboardNeedsFocus: true }))).toBe(
      'From any app. On this desktop the window comes to the front first.',
    ));
});

describe('firstRunHotkeyDesc', () => {
  test("no need for focus keeps today's text", () => {
    expect(firstRunHotkeyDesc(info({ clipboardNeedsFocus: false }))).toBe(
      'Cleans the clipboard in place, without opening this window.',
    );
    expect(firstRunHotkeyDesc(info())).toBe('Cleans the clipboard in place, without opening this window.');
    expect(firstRunHotkeyDesc(null)).toBe('Cleans the clipboard in place, without opening this window.');
  });
  test('needs focus: the window comes forward first', () =>
    expect(firstRunHotkeyDesc(info({ clipboardNeedsFocus: true }))).toBe(
      'Brings this window forward, then cleans the clipboard.',
    ));
});
