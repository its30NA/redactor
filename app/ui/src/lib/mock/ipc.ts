/**
 * Mock backend for `npm run dev` and the Playwright suite (`--mode mock` builds only; the
 * production bundle never imports this module).
 *
 * Installs Tauri's `mockIPC` with handlers for every IPC command, the window controls
 * and events. Scans of a known demo input return its fixture; any other text comes back
 * unchanged with no findings. `keep`, `manual` and remembered secrets are honoured by
 * re-rendering the response, the way the engine would. A tiled demo text (the 5 MiB paste)
 * gets the demo response tiled over it, built in small slices so the mock itself never holds
 * the main thread for long: the real engine runs off the UI thread.
 */
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import { COMMANDS } from '../api';
import { MAX_INPUT_BYTES, utf8Length } from '../scan-request';
import {
  MIN_SESSION_SECRET,
  type AppEventName,
  type AppEvents,
  type Finding,
  type OutputSpan,
  type ScanRequest,
  type ScanResponse,
  type SessionSecret,
  type Settings,
} from '../types';
import { BIG_INPUTS, DEMO_INPUTS, demoInput } from './demo';
import { fixture, type FixtureName } from './fixtures';
import { lineAt, render } from './render';

export interface MockCall {
  cmd: string;
  args: Record<string, unknown>;
}

export interface MockOptions {
  /** Simulated scan latency in ms (the UI's scanning moment needs a beat to show). */
  scanLatencyMs?: number;
  /** Initial session secrets: the demo list (default) or none. */
  session?: 'demo' | 'empty';
  /** What `read_clipboard` returns: a demo or big input name, or null for an empty clipboard. */
  clipboard?: string | null;
  /** Show the first-run card (settings.firstRunDone = false). */
  firstRun?: boolean;
}

export interface MockBackend {
  calls: MockCall[];
  settings: Settings;
  emit<K extends AppEventName>(name: K, payload: AppEvents[K]): Promise<void>;
  /** Replace what `read_clipboard` returns (tests). */
  setClipboard(text: string | null): void;
  /** Make `set_settings` reject with this message, like the shell does (tests); null stops it. */
  rejectSettings(message: string | null): void;
  /** Make `scan` reject, like a failed scan in the shell (tests). */
  failScans(on: boolean): void;
  /** Delay `set_settings` answers by this many ms (tests). */
  delaySettings(ms: number): void;
  uninstall(): void;
}

declare global {
  interface Window {
    /** Present only in mock builds; Playwright drives and inspects the backend through it. */
    __REDACTOR_MOCK__?: MockBackend;
  }
}

const delay = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

const zeroTiers = () => ({ certain: 0, likely: 0, possible: 0 });

function passthrough(text: string): ScanResponse {
  return {
    output: text,
    findings: [],
    outputSpans: [],
    warnings: [],
    verified: true,
    stats: { ms: 0.4, chars: text.length, byTier: zeroTiers(), byLabel: [] },
  };
}

function tooLarge(text: string, bytes: number): ScanResponse {
  return {
    output: '',
    findings: [],
    outputSpans: [],
    warnings: [{ kind: 'inputTooLarge', count: bytes }],
    verified: false,
    stats: { ms: 0, chars: text.length, byTier: zeroTiers(), byLabel: [] },
  };
}

/** A stable fake value key (the engine uses keyed BLAKE3; the mock only needs "same value, same key"). */
function fakeKey(value: string): string {
  let out = '';
  let h = 0x811c9dc5;
  for (let round = 0; round < 4; round++) {
    for (let i = 0; i < value.length; i++) h = Math.imul(h ^ value.charCodeAt(i), 0x01000193) >>> 0;
    h = Math.imul(h ^ round, 0x01000193) >>> 0;
    out += h.toString(16).padStart(8, '0');
  }
  return out;
}

function stats(base: ScanResponse, findings: Finding[]): ScanResponse['stats'] {
  const byTier = zeroTiers();
  const byLabel: { label: string; n: number }[] = [];
  for (const f of findings) {
    byTier[f.tier]++;
    const row = byLabel.find((l) => l.label === f.label);
    if (row) row.n++;
    else byLabel.push({ label: f.label, n: 1 });
  }
  return { ...base.stats, byTier, byLabel };
}

/** A value the engine must find: a session secret (exact) or an always-redact word. */
interface Known {
  value: string;
  label: string;
  /** Always-redact word: case-insensitive, whole words only. */
  word: boolean;
}

interface SessionEntry {
  meta: SessionSecret;
  value: string;
}

const isWordChar = (c: string | undefined) => !!c && /[\p{L}\p{N}_]/u.test(c);

/** Every occurrence of `value` in `text`; words match case-insensitively and whole only. */
function occurrences(text: string, value: string, word: boolean): number[] {
  const hay = word ? text.toLowerCase() : text;
  const needle = word ? value.toLowerCase() : value;
  const out: number[] = [];
  for (let at = hay.indexOf(needle); at >= 0 && needle; at = hay.indexOf(needle, at + needle.length)) {
    if (word && (isWordChar(text[at - 1]) || isWordChar(text[at + needle.length]))) continue;
    out.push(at);
  }
  return out;
}

function scanSmall(req: ScanRequest, known: Known[]): ScanResponse {
  const name = Object.entries(DEMO_INPUTS).find(([, input]) => input() === req.text)?.[0];
  const base = name ? (fixture(`scan-${name}` as FixtureName) as ScanResponse) : passthrough(req.text);
  const findings: Finding[] = base.findings.map((f) => ({ ...f }));
  const free = (start: number, end: number) => !findings.some((f) => f.start < end && start < f.end);
  const extra = (start: number, end: number, f: Omit<Finding, 'id' | 'start' | 'end' | 'line'>) =>
    findings.push({ id: -1, start, end, line: lineAt(req.text, start), ...f });

  for (const k of known) {
    for (const at of occurrences(req.text, k.value, k.word)) {
      if (!free(at, at + k.value.length)) continue;
      extra(at, at + k.value.length, {
        kind: k.word ? 'always_redact' : 'session_secret',
        label: k.label,
        tier: 'certain',
        confidence: 0.99,
        detector: k.word ? 'always_redact' : 'watchlist',
        reason: k.word ? 'On your Always redact list' : `Matches “${k.label}”, remembered for this session`,
        source: k.word ? 'custom' : 'watchlist',
        valueKey: fakeKey(k.word ? k.value.toLowerCase() : k.value),
        placeholder: `[REDACTED: ${k.label}]`,
        kept: false,
      });
    }
  }
  for (const m of req.manual) {
    if (m.end <= m.start || !free(m.start, m.end)) continue;
    const value = req.text.slice(m.start, m.end);
    // A value marked once is redacted everywhere: the other occurrences are propagated.
    for (const at of occurrences(req.text, value, false)) {
      if (!free(at, at + value.length)) continue;
      const own = at === m.start;
      extra(at, at + value.length, {
        kind: 'manual',
        label: 'Manual',
        tier: 'certain',
        confidence: 1,
        detector: own ? 'manual' : 'propagate',
        reason: own ? 'Selected by you' : 'Same value as your selection',
        source: own ? 'manual' : 'propagated',
        valueKey: fakeKey(value),
        placeholder: '[REDACTED: Manual]',
        kept: false,
      });
    }
  }
  const keep = new Set(req.keep);
  if (!keep.size && findings.length === base.findings.length) return base;

  for (const f of findings) f.kept = keep.has(f.valueKey);
  findings.sort((a, b) => a.start - b.start).forEach((f, i) => (f.id = i));
  const { output, outputSpans } = render(req.text, findings);
  return { ...base, output, outputSpans, findings, stats: stats(base, findings) };
}

/** The demo response repeated over a tiled demo text; distinct value keys per tile (worst case for the rail). */
async function scanTiled(tiles: number): Promise<ScanResponse> {
  const base = fixture('scan-demo') as ScanResponse;
  const inLen = demoInput().length;
  const outLen = base.output.length;
  const linesPerTile = demoInput().split('\n').length - 1;
  const findings: Finding[] = [];
  const outputSpans: OutputSpan[] = [];
  const per = base.findings.length;
  for (let t = 0; t < tiles; t++) {
    const suffix = t.toString(16).padStart(8, '0');
    for (const f of base.findings) {
      findings.push({
        ...f,
        id: t * per + f.id,
        start: f.start + t * inLen,
        end: f.end + t * inLen,
        line: f.line + t * linesPerTile,
        valueKey: t === 0 ? f.valueKey : f.valueKey.slice(0, 24) + suffix,
      });
    }
    for (const s of base.outputSpans)
      outputSpans.push({ start: s.start + t * outLen, end: s.end + t * outLen, findingId: t * per + s.findingId });
    if (t % 300 === 299) await delay(0); // yield, like a backend thread would
  }
  const output = base.output.repeat(tiles);
  return {
    ...base,
    output,
    findings,
    outputSpans,
    stats: { ...stats(base, findings), chars: inLen * tiles, ms: 180 },
  };
}

async function scanFor(req: ScanRequest, known: Known[]): Promise<ScanResponse> {
  const bytes = utf8Length(req.text);
  if (bytes > MAX_INPUT_BYTES) return tooLarge(req.text, bytes);
  const demo = demoInput();
  if (
    req.text.length >= demo.length * 8 &&
    req.text.length % demo.length === 0 &&
    req.text.startsWith(demo) &&
    req.text.endsWith(demo)
  )
    return scanTiled(req.text.length / demo.length);
  return scanSmall(req, known);
}

function inputByName(name: string | undefined): string | null {
  if (!name) return null;
  const make = DEMO_INPUTS[name] ?? BIG_INPUTS[name];
  return make ? make() : null;
}

export function installMockBackend(opts: MockOptions = {}): MockBackend {
  const latency = opts.scanLatencyMs ?? 180;
  let settings: Settings = { ...fixture('settings'), firstRunDone: !opts.firstRun };
  // The demo entries have no value the demo text contains, so they never match anything.
  let session: SessionEntry[] =
    opts.session === 'empty' ? [] : fixture('session-demo').map((meta) => ({ meta, value: `\u0000${meta.id}` }));
  let clipboard: string | null = opts.clipboard === null ? null : inputByName(opts.clipboard ?? 'demo');
  let maximized = false;
  let settingsError: string | null = null;
  let scansFail = false;
  let settingsDelay = 0;
  let nextId = 100;
  const calls: MockCall[] = [];

  const list = (): SessionSecret[] => session.map((s) => s.meta);
  const remember = (value: string, label: string): SessionSecret[] => {
    if (value.length < MIN_SESSION_SECRET) throw new Error('secret too short');
    if (/[\r\n]/.test(value)) throw new Error('secret spans lines');
    const meta: SessionSecret = {
      id: `s${nextId++}`,
      label: label.trim() || 'Secret',
      addedAt: new Date().toISOString().replace(/\.\d+Z$/, 'Z'),
    };
    session = [...session.filter((s) => s.value !== value), { meta, value }];
    return list();
  };
  const known = (): Known[] => [
    ...session.map((s) => ({ value: s.value, label: s.meta.label, word: false })),
    ...settings.alwaysRedact.map((w) => ({ value: w, label: 'Always redact', word: true })),
  ];

  mockWindows('main');
  mockIPC(
    async (cmd, payload) => {
      const args = (payload ?? {}) as Record<string, unknown>;
      // Never record secret material, not even in the mock's call log.
      const logged = { ...args };
      if (typeof args.secret === 'string') logged.secret = '•'.repeat(args.secret.length);
      calls.push({ cmd, args: logged });
      switch (cmd) {
        case COMMANDS.scan: {
          await delay(latency);
          if (scansFail) throw 'Internal error.';
          return scanFor(args.req as ScanRequest, known());
        }
        case COMMANDS.readClipboard:
          return clipboard;
        case COMMANDS.copyText:
          clipboard = String(args.text);
          return null;
        case COMMANDS.openTextFile:
          return { name: 'staging-notes.txt', text: demoInput() };
        case COMMANDS.saveTextFile:
          return true;
        case COMMANDS.sessionList:
          return list();
        case COMMANDS.sessionRemember:
          return remember(String(args.text).slice(Number(args.start), Number(args.end)), String(args.label));
        case COMMANDS.sessionAdd:
          return remember(String(args.secret), String(args.label));
        case COMMANDS.sessionForget:
          session = session.filter((s) => s.meta.id !== args.id);
          return list();
        case COMMANDS.sessionClear:
          session = [];
          return list();
        case COMMANDS.getSettings:
          return settings;
        case COMMANDS.setSettings:
          // The shell rejects with a plain string, for example for a hotkey another app holds.
          if (settingsDelay) await delay(settingsDelay);
          if (settingsError !== null) throw settingsError;
          settings = { ...(args.settings as Settings) };
          backend.settings = settings;
          return settings;
        case COMMANDS.appInfo:
          return fixture('app-info');
        case 'plugin:window|minimize':
        case 'plugin:window|close':
          return null;
        case 'plugin:window|toggle_maximize':
          maximized = !maximized;
          return null;
        case 'plugin:window|is_maximized':
          return maximized;
        default:
          throw new Error(`mock backend: unhandled command ${cmd}`);
      }
    },
    { shouldMockEvents: true },
  );

  const backend: MockBackend = {
    calls,
    settings,
    emit: async (name, payload) => {
      // Like the shell: a wipe also forgets the session secrets.
      if (name === 'wipe') session = [];
      await emit(name, payload);
    },
    setClipboard: (text) => {
      clipboard = text;
    },
    rejectSettings: (message) => {
      settingsError = message;
    },
    failScans: (on) => {
      scansFail = on;
    },
    delaySettings: (ms) => {
      settingsDelay = ms;
    },
    uninstall: () => {
      clearMocks();
      delete window.__REDACTOR_MOCK__;
    },
  };
  window.__REDACTOR_MOCK__ = backend;
  return backend;
}
