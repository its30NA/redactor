import { describe, expect, test } from 'vitest';
import type { KeyOs } from '../../src/lib/keys';
import {
  lockDesc,
  lockShort,
  lockTitle,
  shieldNote,
  themeDesc,
  trayNote,
  wipeNotice,
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
  const table: [string, (os: KeyOs) => string, string, string][] = [
    ['lockTitle', lockTitle, 'Wipe when Windows locks', 'Wipe when the screen locks'],
    ['lockDesc', lockDesc, 'Win+L or the screen saver lock.', "The desktop's screen lock, or suspend."],
    ['lockShort', lockShort, 'when Windows locks', 'when the screen locks'],
    [
      "wipeNotice('lock')",
      (os) => wipeNotice('lock', os),
      'Windows was locked. Both panes are empty.',
      'The screen was locked. Both panes are empty.',
    ],
    [
      "wipeNotice('sleep')",
      (os) => wipeNotice('sleep', os),
      'Windows went to sleep. Both panes are empty.',
      'The computer went to sleep. Both panes are empty.',
    ],
    ['themeDesc', themeDesc, 'Follows Windows by default.', 'Follows the desktop by default.'],
  ];

  for (const [name, fn, windows, linux] of table) {
    test(`${name} on Windows keeps today's text`, () => expect(fn('windows')).toBe(windows));
    test(`${name} on Linux`, () => expect(fn('linux')).toBe(linux));
    test(`${name} on macOS uses the Linux wording for now`, () => expect(fn('macos')).toBe(linux));
  }
});

describe('shieldNote', () => {
  test('no note when the shield works', () => expect(shieldNote(info({ shield: true }))).toBeNull());
  test('says why when the desktop cannot hide the window', () =>
    expect(shieldNote(info({ shield: false }))).toBe(
      'Not available on Linux: the desktop cannot hide a window from screenshots.',
    ));
  test('no note before app_info answers', () => expect(shieldNote(null)).toBeNull());
});

describe('trayNote', () => {
  test('no note when there is a tray', () => expect(trayNote(info({ tray: true }))).toBeNull());
  test('says what closing does without a tray', () =>
    expect(trayNote(info({ tray: false }))).toBe('This desktop shows no tray, so closing the window quits Redactor.'));
  test('no note before app_info answers', () => expect(trayNote(null)).toBeNull());
});
