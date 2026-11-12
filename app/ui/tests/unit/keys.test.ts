import { describe, expect, test } from 'vitest';
import cases from '../../../src-tauri/tests/fixtures/hotkey_cases.json';
import {
  actionFor,
  hotkeyFrom,
  hotkeyProblem,
  keyOsFrom,
  keysFor,
  SHORTCUTS,
  type KeyContext,
  type KeyLike,
  type KeyOs,
} from '../../src/lib/keys';

const key = (combo: string, extra: Partial<KeyLike> = {}): KeyLike => {
  const parts = combo.split('+');
  const k = parts.pop() ?? '';
  const shift = parts.includes('Shift');
  return {
    key: k.length === 1 && shift ? k.toUpperCase() : k,
    code: /^\d$/.test(k) ? `Digit${k}` : k.length === 1 ? `Key${k.toUpperCase()}` : k,
    ctrlKey: parts.includes('Ctrl'),
    shiftKey: shift,
    altKey: parts.includes('Alt'),
    metaKey: false,
    ...extra,
  };
};

const free: KeyContext = { inTextField: false, overlayOpen: false, os: 'windows' };

describe('keyboard map', () => {
  test('every shortcut maps to its action', () => {
    const cases: [string, string][] = [
      ['Ctrl+v', 'paste'],
      ['Ctrl+Shift+c', 'copySafe'],
      ['Ctrl+k', 'palette'],
      ['Ctrl+o', 'open'],
      ['Ctrl+s', 'save'],
      ['Ctrl+Shift+x', 'wipe'],
      ['Ctrl+1', 'sensitivity:relaxed'],
      ['Ctrl+2', 'sensitivity:balanced'],
      ['Ctrl+3', 'sensitivity:paranoid'],
      ['Escape', 'escape'],
    ];
    for (const [combo, action] of cases) expect(actionFor(key(combo), free), combo).toBe(action);
  });

  test('the shortcut table lists every action with its keycaps', () => {
    const actions = SHORTCUTS.map((s) => s.action).sort();
    expect(actions).toEqual(
      [
        'copySafe',
        'escape',
        'open',
        'palette',
        'paste',
        'save',
        'sensitivity:balanced',
        'sensitivity:paranoid',
        'sensitivity:relaxed',
        'wipe',
      ].sort(),
    );
    expect(keysFor('copySafe', 'windows')).toEqual(['Ctrl', 'Shift', 'C']);
  });

  test('near misses do nothing', () => {
    for (const combo of ['v', 'Ctrl+c', 'Ctrl+Alt+v', 'Ctrl+Shift+v', 'Ctrl+x', 'Ctrl+4', 'Alt+1', 'Shift+Escape'])
      expect(actionFor(key(combo), free), combo).toBeNull();
    expect(actionFor(key('Ctrl+v', { metaKey: true }), free)).toBeNull();
  });

  test('digits match by physical key on layouts where they need Shift (AZERTY)', () => {
    expect(actionFor({ ...key('Ctrl+1'), key: '&' }, free)).toBe('sensitivity:relaxed');
  });

  test('letters match the character, so QWERTZ and Dvorak follow the printed key', () => {
    expect(actionFor({ ...key('Ctrl+k'), code: 'KeyT' }, free)).toBe('palette');
  });

  test('inside a native text field, Ctrl+V is the field’s own paste', () => {
    const ctx: KeyContext = { inTextField: true, overlayOpen: true, os: 'windows' };
    expect(actionFor(key('Ctrl+v'), ctx)).toBeNull();
    expect(actionFor(key('Ctrl+k'), ctx)).toBe('palette');
  });

  test('while a sheet, dialog or the palette is open only palette, wipe and escape pass', () => {
    const ctx: KeyContext = { inTextField: false, overlayOpen: true, os: 'windows' };
    expect(actionFor(key('Ctrl+o'), ctx)).toBeNull();
    expect(actionFor(key('Ctrl+2'), ctx)).toBeNull();
    expect(actionFor(key('Ctrl+Shift+c'), ctx)).toBeNull();
    expect(actionFor(key('Ctrl+k'), ctx)).toBe('palette');
    expect(actionFor(key('Ctrl+Shift+x'), ctx)).toBe('wipe');
    expect(actionFor(key('Escape'), ctx)).toBe('escape');
  });

  test('key repeat of a held shortcut is ignored', () => {
    expect(actionFor(key('Ctrl+Shift+x', { repeat: true }), free)).toBeNull();
  });
});

describe('global hotkey capture', () => {
  test('formats a pressed combination the way the shell parses it', () => {
    expect(hotkeyFrom(key('Ctrl+Alt+r'), 'windows')).toBe('Ctrl+Alt+R');
    expect(hotkeyFrom(key('Ctrl+Shift+Alt+9'), 'windows')).toBe('Ctrl+Shift+Alt+9');
    expect(hotkeyFrom(key('Ctrl+Alt+F8', { code: 'F8' }), 'windows')).toBe('Ctrl+Alt+F8');
    expect(hotkeyFrom(key('Ctrl+Alt+ ', { code: 'Space' }), 'windows')).toBe('Ctrl+Alt+Space');
    // Letters and digits follow the physical key, so Shift or the layout do not rename them.
    expect(hotkeyFrom({ ...key('Ctrl+Alt+z'), key: 'y', code: 'KeyZ' }, 'windows')).toBe('Ctrl+Alt+Z');
  });

  test('modifier-only presses are still being typed', () => {
    expect(hotkeyFrom(key('Ctrl+Control', { code: 'ControlLeft' }), 'windows')).toBeNull();
    expect(hotkeyFrom(key('Ctrl+Alt+Alt', { code: 'AltLeft' }), 'windows')).toBeNull();
  });

  test('refuses weak and reserved combinations', () => {
    expect(hotkeyProblem('Ctrl+Alt+R', 'windows')).toBeNull();
    expect(hotkeyProblem('Ctrl+Shift+F9', 'windows')).toBeNull();
    expect(hotkeyProblem('R', 'windows')).toMatch(/two modifier/);
    expect(hotkeyProblem('Ctrl+R', 'windows')).toMatch(/two modifier/);
    expect(hotkeyProblem('Ctrl+Alt+Space', 'windows')).toMatch(/reserved/);
    expect(hotkeyProblem('Ctrl+Alt+A', 'windows')).toMatch(/reserved/);
    expect(hotkeyProblem('Ctrl+Shift+C', 'windows')).toMatch(/Redactor/);
    expect(hotkeyProblem('Ctrl+Shift+X', 'windows')).toMatch(/Redactor/);
    expect(hotkeyProblem('Ctrl+Alt+Delete', 'windows')).toMatch(/Windows/);
  });
});

describe('per OS', () => {
  type Case = { os: KeyOs; combo: string; expect: string; rust_only?: boolean };

  test('every case in the shared hotkey table', () => {
    const rows = (cases as Case[]).filter((c) => !c.rust_only);
    expect(rows.length).toBeGreaterThan(40);
    for (const c of rows) {
      const want = c.expect === c.combo ? null : c.expect;
      expect(hotkeyProblem(c.combo, c.os), `${c.os} ${c.combo}`).toBe(want);
    }
  });

  test('macOS shortcuts use Cmd', () => {
    const mac: KeyContext = { inTextField: false, overlayOpen: false, os: 'macos' };
    expect(actionFor(key('k', { metaKey: true }), mac)).toBe('palette');
    expect(actionFor(key('Ctrl+k'), mac)).toBeNull();
    expect(keysFor('copySafe', 'macos')).toEqual(['Cmd', 'Shift', 'C']);
    expect(keysFor('copySafe', 'windows')).toEqual(['Ctrl', 'Shift', 'C']);
  });

  test('hotkey capture names Cmd on macOS and Super on Linux', () => {
    expect(hotkeyFrom(key('Shift+r', { metaKey: true }), 'macos')).toBe('Shift+Cmd+R');
    expect(hotkeyFrom(key('Shift+r', { metaKey: true }), 'linux')).toBe('Shift+Super+R');
    // On Windows the Windows key is the system's: it is left out, as before.
    expect(hotkeyFrom(key('Ctrl+Alt+r', { metaKey: true }), 'windows')).toBe('Ctrl+Alt+R');
  });

  test('an unknown platform keeps the Windows rules', () => {
    expect(keyOsFrom('unsupported')).toBe('windows');
    expect(keyOsFrom('windows-x86_64')).toBe('windows');
    expect(keyOsFrom('macos')).toBe('macos');
    expect(keyOsFrom('linux')).toBe('linux');
  });
});
