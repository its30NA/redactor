/**
 * Input pane: finding highlights and the "lock on" brackets.
 *
 * Findings live in a state field exactly as the engine returned them (UTF-16 offsets into the
 * scanned text) plus the edits made since, composed into one ChangeDesc, so highlights follow
 * typing until the next scan lands. Only the viewport (plus a margin) is ever decorated, so a
 * 5 MiB paste with 50 000 findings costs the same as a short one.
 */
import { EditorSelection, RangeSetBuilder, StateEffect, StateField, type ChangeDesc, type Extension } from '@codemirror/state';
import {
  Decoration,
  EditorView,
  layer,
  RectangleMarker,
  ViewPlugin,
  type DecorationSet,
  type ViewUpdate,
} from '@codemirror/view';
import type { Finding } from '../types';
import { windowOf } from './positions';

const NONE: ReadonlySet<string> = new Set();
/** Characters decorated beyond the viewport on each side. */
const MARGIN = 2000;
/** At most this many brackets lock on at once (the rest just glow). */
const MAX_LOCKS = 48;

export interface FindingsPayload {
  findings: readonly Finding[];
  /** Value keys that lock on with brackets (a paste: all; a re-scan: new values only). */
  fresh: ReadonlySet<string>;
}

interface FindingsState {
  findings: readonly Finding[];
  /** Edits since the findings were applied; null when none. */
  changes: ChangeDesc | null;
  fresh: ReadonlySet<string>;
  hover: string | null;
  flash: number | null;
  /** Bumped per result, so identical brackets of a new result still replay. */
  epoch: number;
}

export const setFindings = StateEffect.define<FindingsPayload>();
/** Ends the lock-on window (brackets fade, highlights stay). */
export const settleFindings = StateEffect.define<null>();
/** Highlight one value (the rail item under the pointer); null clears. */
export const hoverValue = StateEffect.define<string | null>();
/** Hold brackets on one finding (after "show in text"); null clears. */
export const flashFinding = StateEffect.define<number | null>();

export const findingsField = StateField.define<FindingsState>({
  create: () => ({ findings: [], changes: null, fresh: NONE, hover: null, flash: null, epoch: 0 }),
  update(value, tr) {
    let next = value;
    if (tr.docChanged)
      next = { ...next, changes: next.changes ? next.changes.composeDesc(tr.changes.desc) : tr.changes.desc };
    for (const e of tr.effects) {
      if (e.is(setFindings))
        next = { ...next, findings: e.value.findings, fresh: e.value.fresh, changes: null, flash: null, epoch: next.epoch + 1 };
      else if (e.is(settleFindings)) next = { ...next, fresh: NONE };
      else if (e.is(hoverValue)) next = { ...next, hover: e.value };
      else if (e.is(flashFinding)) next = { ...next, flash: e.value };
    }
    return next;
  },
});

type MapFn = (pos: number, assoc: number) => number;
const identity: MapFn = (p) => p;
const mapper = (st: FindingsState): MapFn => {
  const c = st.changes;
  return c ? (p, a) => c.mapPos(p, a) : identity;
};

/** Current document range of a finding (after later edits), or null if its text is gone. */
export function currentRange(state: FindingsState, f: Finding): { from: number; to: number } | null {
  const map = mapper(state);
  const from = map(f.start, 1);
  const to = map(f.end, -1);
  return to > from ? { from, to } : null;
}

function markClass(f: Finding, hover: string | null): string {
  let c = `cm-finding t-${f.tier}`;
  if (f.kept) c += ' is-kept';
  if (f.source === 'manual') c += ' is-manual';
  if (f.source === 'watchlist') c += ' is-known';
  if (hover === f.valueKey) c += ' is-hover';
  return c;
}

function buildMarks(view: EditorView): DecorationSet {
  const st = view.state.field(findingsField);
  if (!st.findings.length) return Decoration.none;
  const docLen = view.state.doc.length;
  const lo = Math.max(0, view.viewport.from - MARGIN);
  const hi = Math.min(docLen, view.viewport.to + MARGIN);
  const map = mapper(st);
  const [a, b] = windowOf(st.findings, lo, hi, map);
  const builder = new RangeSetBuilder<Decoration>();
  for (let i = a; i < b; i++) {
    const f = st.findings[i] as Finding;
    const from = map(f.start, 1);
    const to = map(f.end, -1);
    if (to <= from || to > docLen) continue;
    builder.add(
      from,
      to,
      Decoration.mark({
        class: markClass(f, st.hover),
        attributes: { 'data-fid': String(f.id), 'data-key': f.valueKey },
      }),
    );
  }
  return builder.finish();
}

const marks = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = buildMarks(view);
    }
    update(u: ViewUpdate) {
      if (u.docChanged || u.viewportChanged || u.startState.field(findingsField) !== u.state.field(findingsField))
        this.decorations = buildMarks(u.view);
    }
  },
  { decorations: (v) => v.decorations },
);

/** Corner brackets that zoom in onto each fresh finding, drawn in a layer above the text. */
const locks = layer({
  above: true,
  class: 'cm-lock-layer',
  update: (u) =>
    u.docChanged ||
    u.viewportChanged ||
    u.geometryChanged ||
    u.startState.field(findingsField) !== u.state.field(findingsField),
  markers(view) {
    const st = view.state.field(findingsField);
    if (!st.fresh.size && st.hover === null && st.flash === null) return [];
    const map = mapper(st);
    const { from, to } = view.viewport;
    const [a, b] = windowOf(st.findings, from, to, map);
    const out: RectangleMarker[] = [];
    let n = 0;
    for (let i = a; i < b && n < MAX_LOCKS; i++) {
      const f = st.findings[i] as Finding;
      const fresh = !f.kept && st.fresh.has(f.valueKey);
      const hold = st.hover === f.valueKey || st.flash === f.id;
      if (!fresh && !hold) continue;
      const s = map(f.start, 1);
      const e = map(f.end, -1);
      if (e <= s) continue;
      let cls = `cm-lock t-${f.tier} e${st.epoch % 2}`;
      if (fresh) cls += ` is-fresh d${Math.min(n, 15)}`;
      if (hold) cls += ' is-hold';
      n++;
      out.push(...RectangleMarker.forRange(view, cls, EditorSelection.range(s, e)));
    }
    return out;
  },
});

export function findingsExtension(): Extension {
  return [findingsField, marks, locks];
}
