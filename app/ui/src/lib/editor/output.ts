/**
 * Safe output pane: each placeholder renders as a chip with a lock glyph.
 *
 * The chip is a replace widget over the placeholder text. The document underneath is the exact
 * output, so selecting and copying always yields the real placeholder text. A fresh chip
 * "decrypts": an opaque overlay inside the chip scrambles into the placeholder and then removes
 * itself. The final text is in the chip from the first frame; the overlay only covers it.
 */
import { RangeSetBuilder, StateEffect, StateField, type Extension } from '@codemirror/state';
import { Decoration, EditorView, ViewPlugin, WidgetType, type DecorationSet, type ViewUpdate } from '@codemirror/view';
import { decrypt } from '../scramble';
import type { Finding, OutputSpan } from '../types';
import { windowOf } from './positions';

const NONE: ReadonlySet<string> = new Set();
const MARGIN = 2000;
/** Chips that decrypt at once; later ones in the viewport just appear. */
const MAX_DECRYPTS = 24;

export interface OutputPayload {
  spans: readonly OutputSpan[];
  findings: readonly Finding[];
  /** Value keys whose chips decrypt. */
  fresh: ReadonlySet<string>;
  motion: boolean;
}

interface OutputState extends OutputPayload {
  hover: string | null;
}

export const setOutput = StateEffect.define<OutputPayload>();
export const settleOutput = StateEffect.define<null>();
export const hoverChip = StateEffect.define<string | null>();

const outputField = StateField.define<OutputState>({
  create: () => ({ spans: [], findings: [], fresh: NONE, motion: false, hover: null }),
  update(value, tr) {
    let next = value;
    for (const e of tr.effects) {
      if (e.is(setOutput)) next = { ...e.value, hover: next.hover };
      else if (e.is(settleOutput)) next = { ...next, fresh: NONE };
      else if (e.is(hoverChip)) next = { ...next, hover: e.value };
    }
    return next;
  },
});

const PLACEHOLDER = /^\[([A-Z][A-Z ]*?)(: )(.*)\]$/;

class ChipWidget extends WidgetType {
  constructor(
    readonly text: string,
    readonly tone: 'cyan' | 'violet',
    readonly key: string,
    readonly hover: boolean,
    /** Stagger index of the decrypt effect; -1 for none. */
    readonly decryptAt: number,
  ) {
    super();
  }

  // The decrypt index is left out on purpose: when the effect window ends the chip keeps its DOM.
  override eq(other: ChipWidget): boolean {
    return other.text === this.text && other.tone === this.tone && other.hover === this.hover;
  }

  override toDOM(): HTMLElement {
    const chip = document.createElement('span');
    chip.className = `cm-chip tone-${this.tone}${this.hover ? ' is-hover' : ''}`;
    chip.dataset.key = this.key;
    chip.title = this.text;
    const lock = document.createElement('span');
    lock.className = 'cm-chip-lock';
    lock.setAttribute('aria-hidden', 'true');
    const text = document.createElement('span');
    text.className = 'cm-chip-text';
    const m = PLACEHOLDER.exec(this.text);
    if (m) {
      const dim = (s: string) => {
        const span = document.createElement('span');
        span.className = 'cm-chip-dim';
        span.textContent = s;
        return span;
      };
      const label = document.createElement('span');
      label.className = 'cm-chip-label';
      label.textContent = m[3] ?? '';
      text.append(dim(`[${m[1]}${m[2]}`), label, dim(']'));
    } else {
      text.textContent = this.text;
    }
    chip.append(lock, text);
    if (this.decryptAt >= 0) {
      const veil = document.createElement('span');
      veil.className = 'cm-chip-veil';
      veil.setAttribute('aria-hidden', 'true');
      chip.classList.add('is-decrypting');
      chip.append(veil);
      decrypt(veil, this.text, {
        delay: 60 + this.decryptAt * 42,
        duration: 520,
        onDone: () => {
          chip.classList.remove('is-decrypting');
          veil.remove();
        },
      });
    }
    return chip;
  }

  override ignoreEvent(): boolean {
    return false;
  }
}

function buildChips(view: EditorView): DecorationSet {
  const st = view.state.field(outputField);
  if (!st.spans.length) return Decoration.none;
  const docLen = view.state.doc.length;
  const lo = Math.max(0, view.viewport.from - MARGIN);
  const hi = Math.min(docLen, view.viewport.to + MARGIN);
  const [a, b] = windowOf(st.spans, lo, hi);
  const builder = new RangeSetBuilder<Decoration>();
  let n = 0;
  for (let i = a; i < b; i++) {
    const span = st.spans[i] as OutputSpan;
    if (span.end > docLen || span.end <= span.start) continue;
    const f = st.findings[span.findingId];
    const tone = f && (f.source === 'manual' || f.source === 'watchlist') ? 'violet' : 'cyan';
    const key = f?.valueKey ?? '';
    const visible = span.end > view.viewport.from && span.start < view.viewport.to;
    const fresh = st.motion && visible && st.fresh.has(key) && n < MAX_DECRYPTS;
    builder.add(
      span.start,
      span.end,
      Decoration.replace({
        widget: new ChipWidget(view.state.sliceDoc(span.start, span.end), tone, key, st.hover === key, fresh ? n++ : -1),
      }),
    );
  }
  return builder.finish();
}

const chips = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = buildChips(view);
    }
    update(u: ViewUpdate) {
      if (u.docChanged || u.viewportChanged || u.startState.field(outputField) !== u.state.field(outputField))
        this.decorations = buildChips(u.view);
    }
  },
  {
    decorations: (v) => v.decorations,
    provide: (p) => EditorView.atomicRanges.of((view) => view.plugin(p)?.decorations ?? Decoration.none),
  },
);

export function outputExtension(): Extension {
  return [outputField, chips];
}
