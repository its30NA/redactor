/**
 * The command and type contract between the UI and the Rust shell.
 *
 * Mirrors the Rust shell's IPC contract exactly:
 * camelCase JSON, offsets in UTF-16 code units. Findings never carry the secret value,
 * only offsets into the text the UI already holds.
 */

export type Sensitivity = 'relaxed' | 'balanced' | 'paranoid';
export type Tier = 'certain' | 'likely' | 'possible';
export type Source =
  | 'pattern'
  | 'context'
  | 'classifier'
  | 'decoded'
  | 'watchlist'
  | 'propagated'
  | 'manual'
  | 'custom'
  | 'pii';

export interface ScanRequest {
  text: string;
  sensitivity: Sensitivity;
  pii: boolean;
  stripInvisible: boolean;
  keep: string[];
  manual: { start: number; end: number }[];
}

export interface Finding {
  id: number;
  start: number;
  end: number;
  line: number;
  kind: string;
  label: string;
  tier: Tier;
  confidence: number;
  detector: string;
  reason: string;
  source: Source;
  valueKey: string;
  placeholder: string;
  kept: boolean;
}

export interface OutputSpan {
  start: number;
  end: number;
  findingId: number;
}

export type WarningKind =
  | 'invisibleChars'
  | 'bidiControls'
  | 'tagChars'
  | 'decodeLimit'
  | 'verifyCapHit'
  | 'inputTooLarge';

export interface Warning {
  kind: WarningKind;
  count: number;
}

export interface LabelCount {
  label: string;
  n: number;
}

export interface ScanStats {
  ms: number;
  chars: number;
  byTier: Record<Tier, number>;
  byLabel: LabelCount[];
}

export interface ScanResponse {
  output: string;
  findings: Finding[];
  outputSpans: OutputSpan[];
  warnings: Warning[];
  verified: boolean;
  stats: ScanStats;
}

/**
 * `unverified`: the engine could not verify its output (check before pasting); the clipboard was
 * rewritten when something was removed, and left as it was when `removed` is 0.
 */
/** `stripped`: no secret, but hidden (zero-width, bidi, tag) characters were removed and written back. */
export type ClipboardStatus =
  | 'cleaned'
  | 'unverified'
  | 'stripped'
  | 'clean'
  | 'empty'
  | 'protected'
  | 'tooLarge'
  | 'error';

export interface ClipboardScrub {
  status: ClipboardStatus;
  removed: number;
  byLabel: LabelCount[];
  historyCleaned: boolean;
}

/**
 * The `clipboard-scrubbed` event: a scrub's result and the latest press it covers (it read the
 * clipboard after that press). The shell counts presses (hotkey, tray, flag, "Clean now") from 1.
 */
export interface ScrubResult extends ClipboardScrub {
  press: number;
}

/** The `clipboard-scrub-waiting` event: the press that waits for the window. */
export interface ScrubWaiting {
  press: number;
}

/** The `clipboard-scrub-expired` event: the press that waited for the window in vain. */
export interface ScrubExpired {
  press: number;
}

/**
 * A secret remembered for this session only (lead's contract change of 2026-11-06, replacing
 * the persistent "My secrets" store). Held in the shell's memory, wiped on quit, lock and idle.
 * The value never crosses back to the UI: only its label, not even its length.
 */
export interface SessionSecret {
  id: string;
  label: string;
  addedAt: string;
}

/** Minimum length of a session secret typed into the masked field. */
export const MIN_SESSION_SECRET = 6;
/** Always-redact words: at least 3 characters, at most 500 entries. */
export const MIN_ALWAYS_REDACT = 3;
export const MAX_ALWAYS_REDACT = 500;

export type ThemeSetting = 'system' | 'dark' | 'light';
export type MotionSetting = 'system' | 'on' | 'off';

export interface Settings {
  theme: ThemeSetting;
  sensitivity: Sensitivity;
  pii: boolean;
  stripInvisible: boolean;
  hotkey: string;
  trayMode: boolean;
  autoWipeMinutes: 0 | 5 | 10 | 30;
  wipeOnLock: boolean;
  contentProtection: boolean;
  placeholderStyle: 'labeled' | 'generic';
  /** 'on' = reduced motion on (no animation), 'off' = full motion, 'system' = follow the OS. */
  reducedMotion: MotionSetting;
  /**
   * Plain words that are not passwords (names, company, internal hostnames), redacted as whole
   * words, case-insensitive. Stored locally in plain text, so never passwords. ≥ 3 chars, ≤ 500.
   */
  alwaysRedact: string[];
  /** The first-run card was dismissed (UI-only setting; persisted by the shell). */
  firstRunDone: boolean;
}

export interface AppInfo {
  version: string;
  engineVersion: string;
  ruleCount: number;
  platform: string;
  /** The desktop shows a tray (status notifier) icon, so closing can keep Redactor running. */
  tray: boolean;
  /** The window can be hidden from screenshots and screen sharing. */
  shield: boolean;
  /** How a hotkey press reaches the app. Absent (Windows, macOS) means `keys`. */
  hotkey?: HotkeyMode;
  /** The clipboard works only while the window is in front. Absent means false. */
  clipboardNeedsFocus?: boolean;
  /**
   * The hotkey works now. False: it could not be registered at start (another app holds it, the
   * desktop refused it), the desktop dropped it, or a new one was refused and none is held.
   * Changes come as the `app-status` event. Absent means true.
   */
  hotkeyActive?: boolean;
  /**
   * Something reports a screen lock here. False where no lock source could be subscribed at all
   * (Linux: no logind session, no screen saver on the session bus): a lock does not wipe, suspend
   * and the idle timer still do. Absent: not known in time.
   */
  lockWatched?: boolean;
  /**
   * The program a desktop shortcut runs with `--scrub-clipboard`: this executable, or the
   * AppImage file. Absent where its path is not Unicode.
   */
  executable?: string;
}

/**
 * `keys`: the app registers the key from Settings. `portal`: the desktop binds it and may ask
 * the user. `command`: the app holds no key; a desktop shortcut runs it with `--scrub-clipboard`.
 */
export type HotkeyMode = 'keys' | 'portal' | 'command';

/** What the window shows once, as a notice (`app-status`). */
export type AppNotice = 'hotkeyNotActive' | 'hotkeyLost' | 'settingsNotSaved' | 'settingsUnreadable';

/** The `app-status` event: whether the hotkey works now, and a notice, if any. */
export interface AppStatus {
  hotkeyActive: boolean;
  notice: AppNotice | null;
}

export interface OpenedFile {
  name: string;
  text: string;
}

export type WipeReason = 'idle' | 'lock' | 'sleep' | 'tray';

/** Events emitted by Rust to the UI, keyed by event name. */
export interface AppEvents {
  'clipboard-scrubbed': ScrubResult;
  /** A hotkey scrub waits for the window to come to the front (the clipboard needs focus). */
  'clipboard-scrub-waiting': ScrubWaiting;
  /** That scrub waited in vain and did not run; null from a shell that does not name the press. */
  'clipboard-scrub-expired': ScrubExpired | null;
  /** Also clears the session secrets. */
  wipe: { reason: WipeReason };
  'session-changed': SessionSecret[];
  /** Settings changed outside the Settings sheet (the tray's Screen-share shield toggle). */
  'settings-changed': Settings;
  /** The hotkey's state changed, or the window has something to say (`AppNotice`). */
  'app-status': AppStatus;
}

export type AppEventName = keyof AppEvents;
