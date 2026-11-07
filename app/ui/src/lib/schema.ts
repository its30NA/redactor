/**
 * Runtime validators for the IPC contract in `types.ts`.
 *
 * Objects are checked with an exact key set: a missing key and an unknown key are both errors,
 * so a field like `value` sneaking into a Finding fails loudly. Each object shape is typed
 * against its interface (`{ [K in keyof T]-?: Check }`), so adding a field to `types.ts`
 * without adding it here fails the typecheck.
 */
import {
  MAX_ALWAYS_REDACT,
  MIN_ALWAYS_REDACT,
  type AppInfo,
  type ClipboardScrub,
  type Finding,
  type LabelCount,
  type OutputSpan,
  type ScanRequest,
  type ScanResponse,
  type ScanStats,
  type SessionSecret,
  type Settings,
  type Tier,
  type Warning,
} from './types';

export type Check = (value: unknown, path: string) => string[];

const ok: string[] = [];

export const str: Check = (v, p) => (typeof v === 'string' ? ok : [`${p}: expected string`]);
export const bool: Check = (v, p) => (typeof v === 'boolean' ? ok : [`${p}: expected boolean`]);
export const num: Check = (v, p) =>
  typeof v === 'number' && Number.isFinite(v) ? ok : [`${p}: expected finite number`];
export const uint: Check = (v, p) =>
  Number.isInteger(v) && (v as number) >= 0 ? ok : [`${p}: expected non-negative integer`];

export const oneOf =
  <T extends string | number>(...values: readonly T[]): Check =>
  (v, p) =>
    values.includes(v as T) ? ok : [`${p}: expected one of ${values.join(' | ')}, got ${String(v)}`];

export const nullable =
  (inner: Check): Check =>
  (v, p) =>
    v === null ? ok : inner(v, p);

export const arrayOf =
  (inner: Check): Check =>
  (v, p) =>
    Array.isArray(v) ? v.flatMap((item, i) => inner(item, `${p}[${i}]`)) : [`${p}: expected array`];

export const range =
  (min: number, max: number): Check =>
  (v, p) =>
    typeof v === 'number' && v >= min && v <= max ? ok : [`${p}: expected number in [${min}, ${max}]`];

export const pattern =
  (re: RegExp): Check =>
  (v, p) =>
    typeof v === 'string' && re.test(v) ? ok : [`${p}: expected string matching ${re}`];

export function object<T>(shape: { [K in keyof T]-?: Check }): Check {
  const keys = Object.keys(shape) as (keyof T & string)[];
  return (v, p) => {
    if (!v || typeof v !== 'object' || Array.isArray(v)) return [`${p}: expected object`];
    const rec = v as Record<string, unknown>;
    const errors: string[] = [];
    for (const k of keys) {
      if (!(k in rec)) errors.push(`${p}.${k}: missing`);
      else errors.push(...shape[k](rec[k], `${p}.${k}`));
    }
    for (const k of Object.keys(rec)) {
      if (!keys.includes(k as keyof T & string)) errors.push(`${p}.${k}: unknown key`);
    }
    return errors;
  };
}

const tier = oneOf<Tier>('certain', 'likely', 'possible');
const sensitivity = oneOf('relaxed', 'balanced', 'paranoid');
const isoDate = pattern(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$/);
const labelCount = object<LabelCount>({ label: str, n: uint });

export const scanRequest = object<ScanRequest>({
  text: str,
  sensitivity,
  pii: bool,
  stripInvisible: bool,
  keep: arrayOf(str),
  manual: arrayOf(object<{ start: number; end: number }>({ start: uint, end: uint })),
});

export const finding = object<Finding>({
  id: uint,
  start: uint,
  end: uint,
  line: uint,
  kind: str,
  label: str,
  tier,
  confidence: range(0, 1),
  detector: str,
  reason: str,
  source: oneOf(
    'pattern',
    'context',
    'classifier',
    'decoded',
    'watchlist',
    'propagated',
    'manual',
    'custom',
    'pii',
  ),
  valueKey: pattern(/^[0-9a-f]{32}$/),
  placeholder: str,
  kept: bool,
});

export const outputSpan = object<OutputSpan>({ start: uint, end: uint, findingId: uint });

export const warning = object<Warning>({
  kind: oneOf('invisibleChars', 'bidiControls', 'tagChars', 'decodeLimit', 'verifyCapHit', 'inputTooLarge'),
  count: uint,
});

export const scanStats = object<ScanStats>({
  ms: num,
  chars: uint,
  byTier: object<Record<Tier, number>>({ certain: uint, likely: uint, possible: uint }),
  byLabel: arrayOf(labelCount),
});

export const scanResponse = object<ScanResponse>({
  output: str,
  findings: arrayOf(finding),
  outputSpans: arrayOf(outputSpan),
  warnings: arrayOf(warning),
  verified: bool,
  stats: scanStats,
});

export const clipboardScrub = object<ClipboardScrub>({
  status: oneOf('cleaned', 'clean', 'empty', 'protected', 'tooLarge', 'error'),
  removed: uint,
  byLabel: arrayOf(labelCount),
  historyCleaned: bool,
});

/** A session secret: id, label and time only; never the value, not even its length. */
export const sessionSecret = object<SessionSecret>({
  id: str,
  label: str,
  addedAt: isoDate,
});

export const sessionList = arrayOf(sessionSecret);

const alwaysRedactWord: Check = (v, p) =>
  typeof v === 'string' && v.trim().length >= MIN_ALWAYS_REDACT && !/[\r\n]/.test(v)
    ? ok
    : [`${p}: expected a word of at least ${MIN_ALWAYS_REDACT} characters on one line`];

const alwaysRedact: Check = (v, p) => {
  const errors = arrayOf(alwaysRedactWord)(v, p);
  if (Array.isArray(v) && v.length > MAX_ALWAYS_REDACT) errors.push(`${p}: at most ${MAX_ALWAYS_REDACT} entries`);
  return errors;
};

export const settings = object<Settings>({
  theme: oneOf('system', 'dark', 'light'),
  sensitivity,
  pii: bool,
  stripInvisible: bool,
  hotkey: str,
  trayMode: bool,
  autoWipeMinutes: oneOf(0, 5, 10, 30),
  wipeOnLock: bool,
  contentProtection: bool,
  placeholderStyle: oneOf('labeled', 'generic'),
  reducedMotion: oneOf('system', 'on', 'off'),
  alwaysRedact,
  firstRunDone: bool,
});

export const appInfo = object<AppInfo>({
  version: str,
  engineVersion: str,
  ruleCount: uint,
  platform: str,
});

/** Throws with every problem listed when `value` does not match `check`. */
export function assertValid<T>(check: Check, value: unknown, name: string): asserts value is T {
  const errors = check(value, name);
  if (errors.length) throw new Error(`invalid ${name}:\n  ${errors.join('\n  ')}`);
}
