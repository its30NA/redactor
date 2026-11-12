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
  /** Whose shortcut conventions apply: Cmd on macOS, Ctrl elsewhere. */
  os: KeyOs;
}

/** The OS whose key rules apply. Mirrors `Os` in the shell (`app/src-tauri/src/os.rs`). */
export type KeyOs = 'windows' | 'macos' | 'linux';

/** From `app_info.platform`: anything but macOS or Linux keeps the Windows rules. */
export const keyOsFrom = (platform: string): KeyOs =>
  platform === 'macos' ? 'macos' : platform === 'linux' ? 'linux' : 'windows';

interface Shortcut {
  action: Action;
  /** Keycaps, with 'Mod' for Cmd on macOS and Ctrl elsewhere, e.g. ['Mod', 'Shift', 'C']. */
  keys: string[];
  mod: boolean;
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
    mod: keys.includes('Mod'),
    shift: keys.includes('Shift'),
    key: last.length === 1 ? last.toLowerCase() : last,
    code,
    label,
  };
};

export const SHORTCUTS: readonly Shortcut[] = [
  s('paste', ['Mod', 'V'], 'Paste and scrub'),
  s('copySafe', ['Mod', 'Shift', 'C'], 'Copy safe text'),
  s('palette', ['Mod', 'K'], 'Command palette'),
  s('open', ['Mod', 'O'], 'Open a text file'),
  s('save', ['Mod', 'S'], 'Save a safe copy'),
  s('wipe', ['Mod', 'Shift', 'X'], 'Wipe both panes'),
  s('sensitivity:relaxed', ['Mod', '1'], 'Sensitivity: Relaxed', 'Digit1'),
  s('sensitivity:balanced', ['Mod', '2'], 'Sensitivity: Balanced', 'Digit2'),
  s('sensitivity:paranoid', ['Mod', '3'], 'Sensitivity: Paranoid', 'Digit3'),
  s('escape', ['Esc'], 'Close the sheet or dialog'),
];

/** Keycaps as this OS writes them: Mod is Cmd on macOS and Ctrl elsewhere; Alt is Option on macOS. */
const capsFor = (keys: readonly string[], os: KeyOs): string[] =>
  keys.map((k) => (k === 'Mod' ? (os === 'macos' ? 'Cmd' : 'Ctrl') : k === 'Alt' && os === 'macos' ? 'Option' : k));

/** Keycaps for an action (for buttons, the palette and tooltips). */
export const keysFor = (action: Action, os: KeyOs): string[] =>
  capsFor(SHORTCUTS.find((x) => x.action === action)?.keys ?? [], os);

/** Actions that still work while an overlay is open. */
const OVER_OVERLAY = new Set<Action>(['palette', 'wipe', 'escape']);

export function actionFor(e: KeyLike, ctx: KeyContext): Action | null {
  const mac = ctx.os === 'macos';
  const mod = mac ? e.metaKey : e.ctrlKey;
  const other = mac ? e.ctrlKey : e.metaKey;
  if (e.repeat || other || e.altKey) return null;
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  const hit = SHORTCUTS.find((x) => {
    if (x.action === 'escape') return key === 'Escape' && !mod && !e.shiftKey;
    if (x.mod !== mod || x.shift !== e.shiftKey) return false;
    return key === x.key || (x.code !== undefined && e.code === x.code);
  });
  if (!hit) return null;
  if (ctx.inTextField && hit.action === 'paste') return null;
  if (ctx.overlayOpen && !OVER_OVERLAY.has(hit.action)) return null;
  return hit.action;
}

// ── Global hotkey (Settings) ──────────────────────────────────────────────────
// The same rules as the shell (`app/src-tauri/src/hotkey.rs`); both are checked against
// `app/src-tauri/tests/fixtures/hotkey_cases.json`.

const MODIFIER_CODES = /^(Control|Shift|Alt|Meta|OS)(Left|Right)?$/;
const NAMED_KEYS = ['Insert', 'Delete', 'Home', 'End', 'PageUp', 'PageDown', 'Backspace', 'Enter', 'Tab'];

/** How the meta key is written in a hotkey: Cmd on macOS, Super on Linux; Windows keeps it. */
const META: Record<KeyOs, string | null> = { macos: 'Cmd', linux: 'Super', windows: null };

/** The modifiers held, in the shell's canonical order (Ctrl+Shift+Alt+Cmd/Super). */
export function modifiersOf(e: KeyLike, os: KeyOs): string[] {
  const meta = META[os];
  return [e.ctrlKey && 'Ctrl', e.shiftKey && 'Shift', e.altKey && 'Alt', e.metaKey && meta].filter(
    (m): m is string => !!m,
  );
}

/**
 * The combination a keydown names, as the shell's global-shortcut parser expects it
 * ("Ctrl+Alt+R"), or null while only modifiers are held. Letters and digits follow the
 * physical key, so the name does not change with Shift or the keyboard layout.
 */
export function hotkeyFrom(e: KeyLike, os: KeyOs): string | null {
  const code = e.code ?? '';
  if (MODIFIER_CODES.test(code) || ['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return null;
  let name: string | null = null;
  if (/^Key[A-Z]$/.test(code)) name = code.slice(3);
  else if (/^Digit\d$/.test(code)) name = code.slice(5);
  else if (/^F([1-9]|1\d|2[0-4])$/.test(code)) name = code;
  else if (code === 'Space' || e.key === ' ') name = 'Space';
  else if (NAMED_KEYS.includes(code)) name = code;
  if (!name) return null;
  return [...modifiersOf(e, os), name].join('+');
}

const UNREADABLE = 'That hotkey cannot be read. Use modifiers and one key, like Ctrl+Alt+R.';
const TOO_FEW: Record<KeyOs, string> = {
  windows: 'Use at least two modifier keys (Ctrl, Alt, Shift), so it never fires while typing.',
  macos: 'Use at least two modifier keys (Cmd, Control, Option, Shift), so it never fires while typing.',
  linux: 'Use at least two modifier keys (Ctrl, Alt, Shift, Super), so it never fires while typing.',
};
const SYSTEM_SAYS: Record<KeyOs, string> = {
  windows: 'Windows keeps this one.',
  macos: 'macOS keeps this one.',
  linux: 'Your desktop keeps this one.',
};
const SYSTEM: Record<KeyOs, string[]> = {
  windows: ['Ctrl+Alt+Delete', 'Ctrl+Shift+Escape'],
  macos: [
    'Shift+Cmd+3',
    'Shift+Cmd+4',
    'Shift+Cmd+5',
    'Shift+Cmd+Q',
    'Ctrl+Cmd+Q',
    'Ctrl+Cmd+Space',
    'Ctrl+Cmd+F',
    'Alt+Cmd+Escape',
  ],
  linux: [
    'Ctrl+Alt+Delete',
    'Ctrl+Alt+T',
    'Ctrl+Alt+Backspace',
    ...Array.from({ length: 12 }, (_, i) => `Ctrl+Alt+F${i + 1}`),
  ],
};
const RESERVED = ['Ctrl+Alt+Space', 'Ctrl+Alt+A'];
const MOD_ORDER = ['Ctrl', 'Shift', 'Alt', 'Cmd', 'Super'];

/** An in-app shortcut's name in hotkey form: Mod as Cmd (macOS) or Ctrl, canonical order. */
const inAppName = (keys: readonly string[], os: KeyOs): string => {
  const named = keys.map((k) => (k === 'Mod' ? (os === 'macos' ? 'Cmd' : 'Ctrl') : k));
  const mods = named.slice(0, -1).sort((a, b) => MOD_ORDER.indexOf(a) - MOD_ORDER.indexOf(b));
  return [...mods, named.at(-1)].join('+');
};

/**
 * Why a hotkey (in the form `hotkeyFrom` writes) cannot be used on `os`, or null if it is fine.
 * Same order and messages as the shell: system keys, reserved, in-app, too few modifiers.
 */
export function hotkeyProblem(combo: string, os: KeyOs): string | null {
  const parts = combo.split('+');
  const mods = parts.slice(0, -1);
  const allowed = ['Ctrl', 'Shift', 'Alt', ...(META[os] ? [META[os]] : [])];
  if (!parts.at(-1) || mods.some((m) => !allowed.includes(m)) || new Set(mods).size < mods.length)
    return UNREADABLE;
  const upper = combo.toUpperCase();
  if (SYSTEM[os].some((x) => x.toUpperCase() === upper)) return SYSTEM_SAYS[os];
  if (RESERVED.includes(combo)) return 'This shortcut is reserved.';
  if (SHORTCUTS.some((x) => x.action !== 'escape' && inAppName(x.keys, os).toUpperCase() === upper))
    return 'Redactor already uses this inside its window.';
  if (mods.length < 2) return TOO_FEW[os];
  return null;
}
