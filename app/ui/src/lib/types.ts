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

/** `unverified`: rewritten, but the engine could not verify the output (check before pasting). */
export type ClipboardStatus = 'cleaned' | 'unverified' | 'clean' | 'empty' | 'protected' | 'tooLarge' | 'error';

export interface ClipboardScrub {
  status: ClipboardStatus;
  removed: number;
  byLabel: LabelCount[];
  historyCleaned: boolean;
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
}

export interface OpenedFile {
  name: string;
  text: string;
}

export type WipeReason = 'idle' | 'lock' | 'sleep' | 'tray';

/** Events emitted by Rust to the UI, keyed by event name. */
export interface AppEvents {
  'clipboard-scrubbed': ClipboardScrub;
  /** Also clears the session secrets. */
  wipe: { reason: WipeReason };
  'session-changed': SessionSecret[];
  /** Settings changed outside the Settings sheet (the tray's Screen-share shield toggle). */
  'settings-changed': Settings;
}

export type AppEventName = keyof AppEvents;
