import { describe, expect, test } from 'vitest';
import { actionFor, hotkeyFrom, hotkeyProblem, SHORTCUTS, type KeyContext, type KeyLike } from '../../src/lib/keys';

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

const free: KeyContext = { inTextField: false, overlayOpen: false };

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
    expect(SHORTCUTS.find((s) => s.action === 'copySafe')?.keys).toEqual(['Ctrl', 'Shift', 'C']);
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
    const ctx: KeyContext = { inTextField: true, overlayOpen: true };
    expect(actionFor(key('Ctrl+v'), ctx)).toBeNull();
    expect(actionFor(key('Ctrl+k'), ctx)).toBe('palette');
  });

  test('while a sheet, dialog or the palette is open only palette, wipe and escape pass', () => {
    const ctx: KeyContext = { inTextField: false, overlayOpen: true };
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
    expect(hotkeyFrom(key('Ctrl+Alt+r'))).toBe('Ctrl+Alt+R');
    expect(hotkeyFrom(key('Ctrl+Shift+Alt+9'))).toBe('Ctrl+Shift+Alt+9');
    expect(hotkeyFrom(key('Ctrl+Alt+F8', { code: 'F8' }))).toBe('Ctrl+Alt+F8');
    expect(hotkeyFrom(key('Ctrl+Alt+ ', { code: 'Space' }))).toBe('Ctrl+Alt+Space');
    // Letters and digits follow the physical key, so Shift or the layout do not rename them.
    expect(hotkeyFrom({ ...key('Ctrl+Alt+z'), key: 'y', code: 'KeyZ' })).toBe('Ctrl+Alt+Z');
  });

  test('modifier-only presses are still being typed', () => {
    expect(hotkeyFrom(key('Ctrl+Control', { code: 'ControlLeft' }))).toBeNull();
    expect(hotkeyFrom(key('Ctrl+Alt+Alt', { code: 'AltLeft' }))).toBeNull();
  });

  test('refuses weak and reserved combinations', () => {
    expect(hotkeyProblem('Ctrl+Alt+R')).toBeNull();
    expect(hotkeyProblem('Ctrl+Shift+F9')).toBeNull();
    expect(hotkeyProblem('R')).toMatch(/two modifier/);
    expect(hotkeyProblem('Ctrl+R')).toMatch(/two modifier/);
    expect(hotkeyProblem('Ctrl+Alt+Space')).toMatch(/reserved/);
    expect(hotkeyProblem('Ctrl+Alt+A')).toMatch(/reserved/);
    expect(hotkeyProblem('Ctrl+Shift+C')).toMatch(/Redactor/);
    expect(hotkeyProblem('Ctrl+Shift+X')).toMatch(/Redactor/);
    expect(hotkeyProblem('Ctrl+Alt+Delete')).toMatch(/Windows/);
  });
});
