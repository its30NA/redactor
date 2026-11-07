/**
 * The Wipe moment: the visible part of an editor is copied into an overlay, cut into vertical
 * strips that fall away and dissolve, while the real editor is already empty underneath.
 * Only the rendered viewport is copied (CodeMirror renders little more than what is visible),
 * the copy is inert and hidden from assistive tech, and it removes itself when done.
 * The strips move with transform and opacity only.
 */
import type { EditorView } from '@codemirror/view';

const STRIPS = 9;
const DURATION = 900;

export function shred(view: EditorView): void {
  const host = view.dom.parentElement;
  if (!host) return;
  const scrollTop = view.scrollDOM.scrollTop;
  const overlay = document.createElement('div');
  overlay.className = 'shred';
  overlay.setAttribute('aria-hidden', 'true');
  overlay.inert = true;
  for (let i = 0; i < STRIPS; i++) {
    const strip = document.createElement('div');
    strip.className = `shred-strip s${i}`;
    const copy = view.dom.cloneNode(true) as HTMLElement;
    copy.classList.add('shred-copy');
    copy.removeAttribute('id');
    strip.append(copy);
    overlay.append(strip);
  }
  const bar = document.createElement('div');
  bar.className = 'shred-bar';
  overlay.append(bar);
  host.append(overlay);
  // Clones lose their scroll position; restore it so the copy matches what was on screen.
  for (const scroller of overlay.querySelectorAll<HTMLElement>('.cm-scroller')) scroller.scrollTop = scrollTop;
  setTimeout(() => overlay.remove(), DURATION + 200);
}
