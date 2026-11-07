/**
 * The app's keyboard map. Pure: App.svelte feeds it keydown events plus a little
 * context and runs whatever action comes back.
 *
 * Letters match the printed character (`key`), so the shortcuts follow QWERTZ or Dvorak
 * labels; digits also match the physical key (`code`), because AZERTY needs Shift for them.
 */

export type Action =
  | 'paste'
  | 'copySafe'
  | 'palette'
  | 'open'
  | 'save'
  | 'wipe'
  | 'sensitivity:relaxed'
  | 'sensitivity:balanced'
  | 'sensitivity:paranoid'
  | 'escape';

export interface KeyLike {
  key: string;
  code?: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
  repeat?: boolean;
}

export interface KeyContext {
  /** Focus is in a native <input>/<textarea> (not the Input editor). */
  inTextField: boolean;
  /** A sheet, dialog or the command palette is open. */
  overlayOpen: boolean;
}

interface Shortcut {
  action: Action;
  /** Keycaps for display, e.g. ['Ctrl', 'Shift', 'C']. */
  keys: string[];
  ctrl: boolean;
  shift: boolean;
  /** Lower-case character or key name. */
  key: string;
  /** Physical key alternative (digits). */
  code?: string;
  label: string;
}

const s = (action: Action, keys: string[], label: string, code?: string): Shortcut => {
  const last = keys.at(-1) ?? '';
  return {
    action,
    keys,
    ctrl: keys.includes('Ctrl'),
    shift: keys.includes('Shift'),
    key: last.length === 1 ? last.toLowerCase() : last,
    code,
    label,
  };
};

export const SHORTCUTS: readonly Shortcut[] = [
  s('paste', ['Ctrl', 'V'], 'Paste and scrub'),
  s('copySafe', ['Ctrl', 'Shift', 'C'], 'Copy safe text'),
  s('palette', ['Ctrl', 'K'], 'Command palette'),
  s('open', ['Ctrl', 'O'], 'Open a text file'),
  s('save', ['Ctrl', 'S'], 'Save a safe copy'),
  s('wipe', ['Ctrl', 'Shift', 'X'], 'Wipe both panes'),
  s('sensitivity:relaxed', ['Ctrl', '1'], 'Sensitivity: Relaxed', 'Digit1'),
  s('sensitivity:balanced', ['Ctrl', '2'], 'Sensitivity: Balanced', 'Digit2'),
  s('sensitivity:paranoid', ['Ctrl', '3'], 'Sensitivity: Paranoid', 'Digit3'),
  s('escape', ['Esc'], 'Close the sheet or dialog'),
];

/** Keycaps for an action (for buttons, the palette and tooltips). */
export const keysFor = (action: Action): string[] => SHORTCUTS.find((x) => x.action === action)?.keys ?? [];

/** Actions that still work while an overlay is open. */
const OVER_OVERLAY = new Set<Action>(['palette', 'wipe', 'escape']);

export function actionFor(e: KeyLike, ctx: KeyContext): Action | null {
  if (e.repeat || e.metaKey || e.altKey) return null;
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  const hit = SHORTCUTS.find((x) => {
    if (x.action === 'escape') return key === 'Escape' && !e.ctrlKey && !e.shiftKey;
    if (x.ctrl !== e.ctrlKey || x.shift !== e.shiftKey) return false;
    return key === x.key || (x.code !== undefined && e.code === x.code);
  });
  if (!hit) return null;
  if (ctx.inTextField && hit.action === 'paste') return null;
  if (ctx.overlayOpen && !OVER_OVERLAY.has(hit.action)) return null;
  return hit.action;
}

// ── Global hotkey (Settings) ──────────────────────────────────────────────────

const MODIFIER_CODES = /^(Control|Shift|Alt|Meta|OS)(Left|Right)?$/;
const NAMED_KEYS = ['Insert', 'Delete', 'Home', 'End', 'PageUp', 'PageDown', 'Backspace', 'Enter', 'Tab'];

/**
 * The combination a keydown names, as the shell's global-shortcut parser expects it
 * ("Ctrl+Alt+R"), or null while only modifiers are held. Letters and digits follow the
 * physical key, so the name does not change with Shift or the keyboard layout.
 */
export function hotkeyFrom(e: KeyLike): string | null {
  const code = e.code ?? '';
  if (MODIFIER_CODES.test(code) || ['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return null;
  let name: string | null = null;
  if (/^Key[A-Z]$/.test(code)) name = code.slice(3);
  else if (/^Digit\d$/.test(code)) name = code.slice(5);
  else if (/^F([1-9]|1\d|2[0-4])$/.test(code)) name = code;
  else if (code === 'Space' || e.key === ' ') name = 'Space';
  else if (NAMED_KEYS.includes(code)) name = code;
  if (!name) return null;
  const mods = [e.ctrlKey && 'Ctrl', e.shiftKey && 'Shift', e.altKey && 'Alt'].filter(Boolean);
  return [...mods, name].join('+');
}

/** Why a hotkey cannot be used, or null if it is fine. */
export function hotkeyProblem(combo: string): string | null {
  const parts = combo.split('+');
  const mods = parts.filter((p) => p === 'Ctrl' || p === 'Shift' || p === 'Alt');
  if (mods.length < 2) return 'Use at least two modifier keys (Ctrl, Alt, Shift), so it never fires while typing.';
  if (combo === 'Ctrl+Alt+Space' || combo === 'Ctrl+Alt+A') return 'This shortcut is reserved.';
  if (combo === 'Ctrl+Alt+Delete' || combo === 'Ctrl+Shift+Escape') return 'Windows keeps this one.';
  if (SHORTCUTS.some((s) => s.keys.join('+').toUpperCase() === combo.toUpperCase()))
    return 'Redactor already uses this inside its window.';
  return null;
}
