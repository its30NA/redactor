/**
 * The workspace controller: owns the two editors, the scan scheduler and every action on the
 * text (paste, open, Keep, Redact, Remember, copy, save, wipe). Components render its state
 * and call its methods; nothing else talks to the editors.
 *
 * The Input editor's document is the source of truth for the text. A response is applied only
 * while it matches that document (the scheduler drops stale ones), so finding offsets are
 * always offsets into what the user sees.
 */
/* eslint-disable svelte/prefer-svelte-reactivity -- the sets here are immutable snapshots: held in
   $state.raw and replaced whole, or handed to CodeMirror; none is mutated after creation. */
import { EditorState, Transaction, type Extension } from '@codemirror/state';
import { EditorView, type ViewUpdate } from '@codemirror/view';
import { copyText, openTextFile, readClipboard, saveTextFile, scan } from './api';
import { appearance } from './appearance.svelte';
import {
  currentRange,
  findingsExtension,
  findingsField,
  flashFinding,
  hoverValue,
  setFindings,
  settleFindings,
} from './editor/findings';
import { hoverChip, outputExtension, setOutput, settleOutput } from './editor/output';
import { addRange, mapRanges, minimalChange, removeOverlapping, selectionToManual, type Range } from './editor/positions';
import { inputExtensions, outputExtensions } from './editor/setup';
import { EMPTY_GROUPS, freshKeys, groupFindings, type FindingGroup } from './findings';
import { buildScanRequest, definitelyTooLarge } from './scan-request';
import { createScheduler } from './scheduler';
import { session } from './session.svelte';
import { settings } from './settings.svelte';
import { shred } from './shred';
import { toasts } from './toast.svelte';
import { MIN_SESSION_SECRET, type Finding, type ScanRequest, type ScanResponse, type WipeReason } from './types';

/** How long fresh findings keep their brackets and chips their decrypt overlay. */
const MOMENT_MS = 1500;
/** How long "show in text" holds the brackets. */
const FLASH_MS = 1800;

export interface SelectionInfo extends Range {
  /** Overlaps a finding that is redacted already. */
  covered: boolean;
  /** Value key of a kept finding it overlaps (Redact then means "stop keeping"). */
  keptKey: string | null;
}

const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString('en')} ${n === 1 ? one : many}`;

class Workspace {
  /** The last applied scan response (matches the input, or the input changed since: `stale`). */
  response = $state.raw<ScanResponse | null>(null);
  groups = $derived(this.response ? groupFindings(this.response.findings) : EMPTY_GROUPS);
  busy = $state(false);
  /** The input changed since `response` was computed. */
  stale = $state(false);
  length = $state(0);
  lines = $state(1);
  tooLarge = $state<{ chars: number; bytes: number | null } | null>(null);
  error = $state(false);
  keep = $state.raw<ReadonlySet<string>>(new Set());
  manual = $state.raw<readonly Range[]>([]);
  selection = $state.raw<SelectionInfo | null>(null);
  inputFocused = $state(false);
  hovered = $state<string | null>(null);
  fileName = $state<string | null>(null);
  /** Epochs that restart the one-shot overlays (scan line, status count). */
  pasteEpoch = $state(0);
  resultEpoch = $state(0);
  /** The Wipe shred is playing: the empty states wait for it. */
  shredding = $state(false);
  /** Values that just appeared: their rail items and chips play the arrival moment. */
  fresh = $state.raw<ReadonlySet<string>>(new Set());

  #input: EditorView | null = null;
  #output: EditorView | null = null;
  #outputText = '';
  #moment: 'paste' | null = null;
  #settleTimer: ReturnType<typeof setTimeout> | undefined;
  #flashTimer: ReturnType<typeof setTimeout> | undefined;
  #shredTimer: ReturnType<typeof setTimeout> | undefined;

  #scheduler = createScheduler<ScanRequest, ScanResponse>({
    build: () => this.#build(),
    run: (req) => scan(req),
    onResult: (res) => this.#apply(res),
    onError: () => {
      this.error = true;
      toasts.show({ tone: 'error', title: 'Scan failed', detail: 'The safe output was not updated.' });
    },
    onBusy: (busy) => (this.busy = busy),
  });

  get hasText(): boolean {
    return this.length > 0;
  }

  /** Output is current, verified and copyable. */
  get ready(): boolean {
    return !!this.response && !this.stale && !this.busy && !this.tooLarge && this.hasText;
  }

  // ── Editors ────────────────────────────────────────────────────────────────

  inputExtensions(): Extension {
    return inputExtensions([
      findingsExtension(),
      EditorView.updateListener.of((u) => this.#onInput(u)),
      EditorView.domEventHandlers({
        // Every paste goes through the shell's clipboard read, which refuses password-manager
        // copies; the browser's own paste would bypass that check.
        paste: (e) => {
          e.preventDefault();
          void this.paste('insert');
          return true;
        },
        mousedown: () => {
          this.#dragging = true;
          return false;
        },
      }),
    ]);
  }

  outputExtensions(): Extension {
    return outputExtensions([outputExtension()]);
  }

  #dragging = false;

  attachInput(view: EditorView): void {
    this.#input = view;
    window.addEventListener('mouseup', this.#onMouseUp);
  }

  detachInput(): void {
    window.removeEventListener('mouseup', this.#onMouseUp);
    this.#input = null;
  }

  attachOutput(view: EditorView): void {
    this.#output = view;
    this.#outputText = view.state.doc.toString();
  }

  detachOutput(): void {
    this.#output = null;
  }

  /** The Input editor (the selection bar positions itself against it). */
  get inputView(): EditorView | null {
    return this.#input;
  }

  #onMouseUp = () => {
    if (!this.#dragging) return;
    this.#dragging = false;
    if (this.#input) this.#updateSelection(this.#input);
  };

  focusInput(): void {
    this.#input?.focus();
  }

  // ── Scanning ───────────────────────────────────────────────────────────────

  #build(): ScanRequest | null {
    const view = this.#input;
    if (!view) return null;
    const len = view.state.doc.length;
    if (len === 0) return null;
    if (definitelyTooLarge(len)) {
      this.#showTooLarge(len, null);
      return null;
    }
    return buildScanRequest(view.state.doc.toString(), settings.value, this.keep, this.manual);
  }

  #showTooLarge(chars: number, bytes: number | null) {
    this.tooLarge = { chars, bytes };
    this.response = null;
    this.stale = false;
    this.#setOutput('', { spans: [], findings: [], fresh: new Set(), motion: false });
    this.#input?.dispatch({ effects: setFindings.of({ findings: [], fresh: new Set() }) });
  }

  #apply(res: ScanResponse) {
    const input = this.#input;
    if (!input) return;
    const big = res.warnings.find((w) => w.kind === 'inputTooLarge');
    if (big) {
      this.#showTooLarge(input.state.doc.length, big.count);
      return;
    }
    const motion = !appearance.reducedMotion;
    const fresh =
      this.#moment === 'paste' ? new Set(res.findings.map((f) => f.valueKey)) : freshKeys(this.response?.findings ?? null, res.findings);
    this.#moment = null;
    this.tooLarge = null;
    this.error = false;
    this.stale = false;
    this.response = res;
    this.fresh = motion ? fresh : new Set();
    input.dispatch({ effects: setFindings.of({ findings: res.findings, fresh: motion ? fresh : new Set() }) });
    this.#setOutput(res.output, { spans: res.outputSpans, findings: res.findings, fresh, motion });
    this.resultEpoch++;
    this.#updateSelection(input);
    clearTimeout(this.#settleTimer);
    if (fresh.size)
      this.#settleTimer = setTimeout(() => {
        this.fresh = new Set();
        this.#input?.dispatch({ effects: settleFindings.of(null) });
        this.#output?.dispatch({ effects: settleOutput.of(null) });
      }, MOMENT_MS);
  }

  #setOutput(text: string, payload: Parameters<typeof setOutput.of>[0]) {
    const out = this.#output;
    if (!out) return;
    const change = minimalChange(this.#outputText, text);
    this.#outputText = text;
    out.dispatch({
      changes: change ?? undefined,
      effects: setOutput.of(payload),
      annotations: Transaction.addToHistory.of(false),
    });
  }

  #onInput(u: ViewUpdate) {
    if (u.focusChanged) this.inputFocused = u.view.hasFocus;
    if (u.docChanged) {
      const doc = u.state.doc;
      this.length = doc.length;
      this.lines = doc.lines;
      if (this.manual.length) this.manual = mapRanges(this.manual, (p, a) => u.changes.mapPos(p, a));
      if (doc.length === 0) {
        this.#scheduler.cancel();
        this.response = null;
        this.stale = false;
        this.tooLarge = null;
        this.#setOutput('', { spans: [], findings: [], fresh: new Set(), motion: false });
      } else {
        this.stale = this.response !== null;
        const pasted = u.transactions.some((tr) => tr.isUserEvent('input.paste') || tr.isUserEvent('input.drop'));
        if (pasted) {
          this.#moment = 'paste';
          if (!appearance.reducedMotion) this.pasteEpoch++;
          this.#scheduler.now();
        } else {
          this.#scheduler.schedule();
        }
      }
    }
    if (u.focusChanged) {
      // Focus may be moving into the selection bar; decide once it has landed.
      setTimeout(() => this.refreshSelection(), 0);
    } else if (u.docChanged || u.selectionSet || u.transactions.some((t) => t.effects.length)) {
      this.#updateSelection(u.view);
    }
  }

  refreshSelection(): void {
    if (this.#input) this.#updateSelection(this.#input);
  }

  #updateSelection(view: EditorView) {
    const sel = view.state.selection.main;
    const focused = view.hasFocus || !!document.activeElement?.closest('[data-selbar]');
    if (sel.empty || this.#dragging || !focused) {
      if (this.selection) this.selection = null;
      return;
    }
    const len = view.state.doc.length;
    const codeAt = (pos: number) => (pos < 0 || pos >= len ? NaN : view.state.sliceDoc(pos, pos + 1).charCodeAt(0));
    const r = selectionToManual(sel.from, sel.to, codeAt);
    if (!r) {
      this.selection = null;
      return;
    }
    const st = view.state.field(findingsField);
    let covered = false;
    let keptKey: string | null = null;
    for (const f of this.#findingsIn(r.start, r.end)) {
      const cur = currentRange(st, f);
      if (!cur || cur.to <= r.start || cur.from >= r.end) continue;
      if (f.kept) keptKey = f.valueKey;
      else covered = true;
    }
    const prev = this.selection;
    if (prev && prev.start === r.start && prev.end === r.end && prev.covered === covered && prev.keptKey === keptKey) return;
    this.selection = { ...r, covered, keptKey };
  }

  /** Findings near a range (by their original offsets; callers re-check with currentRange). */
  #findingsIn(start: number, end: number): Finding[] {
    const findings = this.response?.findings ?? [];
    const slack = 4096;
    return findings.filter((f) => f.end >= start - slack && f.start <= end + slack);
  }

  // ── Actions ────────────────────────────────────────────────────────────────

  /**
   * Paste through the shell (null = empty, not text, or a password-manager copy).
   * 'replace' swaps the whole input (Paste & scrub); 'insert' pastes at the cursor when the
   * Input has focus and text, like an editor.
   */
  async paste(mode: 'replace' | 'insert' = 'replace'): Promise<void> {
    let text: string | null;
    try {
      text = await readClipboard();
    } catch (e) {
      // The shell's messages are fixed sentences (busy, larger than 20 MiB), never content.
      toasts.show({
        tone: 'error',
        title: 'Could not read the clipboard',
        detail: typeof e === 'string' ? e : undefined,
      });
      return;
    }
    if (text === null || text === '') {
      toasts.show({
        tone: 'warning',
        title: 'Nothing to paste',
        detail: 'The clipboard is empty, not text, or marked private by a password manager.',
      });
      return;
    }
    const view = this.#input;
    if (!view) return;
    if (mode === 'insert' && view.hasFocus && view.state.doc.length > 0) {
      const sel = view.state.selection.main;
      view.dispatch({
        changes: { from: sel.from, to: sel.to, insert: text },
        selection: { anchor: sel.from + text.length },
        userEvent: 'input.paste',
        scrollIntoView: true,
      });
      return;
    }
    this.#replaceAll(text);
  }

  #replaceAll(text: string) {
    const view = this.#input;
    if (!view) return;
    this.keep = new Set();
    this.manual = [];
    this.response = null;
    this.tooLarge = null;
    this.#setOutput('', { spans: [], findings: [], fresh: new Set(), motion: false });
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: text },
      selection: { anchor: 0 },
      effects: [setFindings.of({ findings: [], fresh: new Set() }), EditorView.scrollIntoView(0)],
      userEvent: 'input.paste',
    });
    this.#output?.dispatch({ effects: EditorView.scrollIntoView(0) });
  }

  async open(): Promise<void> {
    try {
      const file = await openTextFile();
      if (!file) return;
      this.#replaceAll(file.text);
      this.fileName = file.name;
      toasts.show({ tone: 'info', title: `Opened ${file.name}`, detail: plural(file.text.length, 'character') });
    } catch {
      toasts.show({ tone: 'error', title: 'Could not open the file', detail: 'Text files up to 20 MB, UTF-8 or UTF-16.' });
    }
  }

  /** Waits for a pending scan, then returns the output if it is safe to hand out. */
  async #safeOutput(verb: string): Promise<string | null> {
    if (!this.hasText) {
      toasts.show({ tone: 'warning', title: `Nothing to ${verb} yet`, detail: 'Paste text first. The safe version appears on the right.' });
      return null;
    }
    if (this.busy) await this.#scheduler.idle();
    if (this.tooLarge) {
      toasts.show({ tone: 'error', title: 'Too large to scan', detail: 'Nothing was copied. Split the text into parts under 20 MB.' });
      return null;
    }
    if (!this.response || this.stale) return null;
    return this.response.output;
  }

  async copySafe(): Promise<void> {
    const text = await this.#safeOutput('copy');
    if (text === null) return;
    try {
      await copyText(text);
      const n = this.groups.redacted;
      toasts.show({
        tone: this.response?.verified ? 'success' : 'warning',
        title: 'Safe text copied',
        detail: `${n ? `${plural(n, 'secret')} removed` : 'Nothing sensitive found'} · ${this.response?.verified ? 'verified' : 'check it before you paste'}`,
      });
    } catch {
      toasts.show({ tone: 'error', title: 'Could not copy' });
    }
  }

  async save(): Promise<void> {
    const text = await this.#safeOutput('save');
    if (text === null) return;
    const base = this.fileName?.replace(/\.[^.]+$/, '') ?? 'redacted';
    try {
      if (await saveTextFile(text, `${base}.safe.txt`)) toasts.show({ tone: 'success', title: 'Safe copy saved' });
    } catch {
      toasts.show({ tone: 'error', title: 'Could not save' });
    }
  }

  wipe(reason?: WipeReason): void {
    const input = this.#input;
    const output = this.#output;
    const had = this.hasText;
    // The shred plays only for a Wipe the user asked for: after lock or idle nobody watches, and
    // the copy it animates would keep the text on screen a moment longer.
    if (had && !reason && !appearance.reducedMotion) {
      if (input) shred(input);
      if (output && this.#outputText) shred(output);
      this.shredding = true;
      clearTimeout(this.#shredTimer);
      this.#shredTimer = setTimeout(() => (this.shredding = false), 1400);
    }
    this.#scheduler.cancel();
    clearTimeout(this.#settleTimer);
    this.keep = new Set();
    this.manual = [];
    this.response = null;
    this.tooLarge = null;
    this.stale = false;
    this.selection = null;
    this.fileName = null;
    this.length = 0;
    this.lines = 1;
    this.#outputText = '';
    // Fresh states: the undo history must not be able to bring the text back.
    input?.setState(EditorState.create({ doc: '', extensions: this.inputExtensions() }));
    output?.setState(EditorState.create({ doc: '', extensions: this.outputExtensions() }));
    const detail =
      reason === 'idle'
        ? `After ${settings.value.autoWipeMinutes} idle minutes. Both panes are empty.`
        : reason === 'lock'
          ? 'Windows was locked. Both panes are empty.'
          : reason === 'sleep'
            ? 'Windows went to sleep. Both panes are empty.'
            : reason === 'tray'
              ? 'From the tray. Both panes are empty.'
              : 'Both panes are empty. Undo cannot bring it back.';
    if (had || reason) toasts.show({ tone: 'success', title: 'Wiped', detail });
  }

  /** Re-scan now (sensitivity, PII or a remembered secret changed). */
  rescan(): void {
    if (this.hasText) this.#scheduler.now();
  }

  keepValue(key: string): void {
    this.keep = new Set([...this.keep, key]);
    this.#scheduler.now();
  }

  redactValue(key: string): void {
    const next = new Set(this.keep);
    next.delete(key);
    this.keep = next;
    this.#scheduler.now();
  }

  /** Keep for a manual finding: drop the selection ranges behind it. */
  unredactManual(group: FindingGroup): void {
    const st = this.#input?.state.field(findingsField);
    let manual = [...this.manual];
    for (const id of group.ids) {
      const f = this.response?.findings[id];
      const cur = f && st ? currentRange(st, f) : null;
      if (cur) manual = removeOverlapping(manual, { start: cur.from, end: cur.to });
    }
    this.manual = manual;
    this.#scheduler.now();
  }

  redactSelection(): void {
    const sel = this.selection;
    const view = this.#input;
    if (!sel || !view) return;
    if (sel.keptKey) this.redactValue(sel.keptKey);
    else if (!sel.covered) {
      this.manual = addRange(this.manual, sel);
      this.#scheduler.now();
    }
    view.dispatch({ selection: { anchor: sel.end } });
    this.selection = null;
  }

  /**
   * Remember the selection for this session. The shell gets the whole text and the range and
   * keeps the value in memory only; the UI never holds it apart from the text itself.
   */
  async rememberSelection(): Promise<void> {
    const sel = this.selection;
    const view = this.#input;
    if (!sel || !view) return;
    const line = view.state.doc.lineAt(sel.start).number;
    const ok = await this.#remember(sel, `Selection · line ${line}`);
    if (ok) {
      view.dispatch({ selection: { anchor: sel.end } });
      this.selection = null;
    }
  }

  async rememberFinding(group: FindingGroup): Promise<void> {
    const st = this.#input?.state.field(findingsField);
    const cur = st ? currentRange(st, group.first) : null;
    if (!cur) return;
    await this.#remember({ start: cur.from, end: cur.to }, `${group.label} · line ${group.first.line}`);
  }

  async #remember(range: Range, label: string): Promise<boolean> {
    if (range.end - range.start < MIN_SESSION_SECRET) {
      toasts.show({ tone: 'warning', title: `At least ${MIN_SESSION_SECRET} characters`, detail: 'Shorter values would match too much.' });
      return false;
    }
    const ok = await session.remember(this.inputText(), range.start, range.end, label);
    if (!ok) return false;
    toasts.show({
      tone: 'success',
      title: 'Remembered for this session',
      detail: 'Redacted wherever it shows up, until you quit, lock or wipe.',
    });
    this.rescan();
    return true;
  }

  /** The current input text. */
  inputText(): string {
    return this.#input?.state.doc.toString() ?? '';
  }

  /** Scroll both panes to a value's first occurrence and hold its brackets for a moment. */
  reveal(group: FindingGroup): void {
    const input = this.#input;
    if (!input) return;
    const cur = currentRange(input.state.field(findingsField), group.first);
    if (!cur) return;
    input.dispatch({
      effects: [EditorView.scrollIntoView(cur.from, { y: 'center' }), flashFinding.of(group.first.id)],
    });
    const span = this.response?.outputSpans.find((s) => s.findingId === group.first.id);
    if (span && !this.stale) this.#output?.dispatch({ effects: EditorView.scrollIntoView(span.start, { y: 'center' }) });
    clearTimeout(this.#flashTimer);
    this.#flashTimer = setTimeout(() => this.#input?.dispatch({ effects: flashFinding.of(null) }), FLASH_MS);
  }

  hover(key: string | null): void {
    if (this.hovered === key) return;
    this.hovered = key;
    this.#input?.dispatch({ effects: hoverValue.of(key) });
    this.#output?.dispatch({ effects: hoverChip.of(key) });
  }
}

export const workspace = new Workspace();
export type { Workspace };
