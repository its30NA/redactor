/**
 * The "decrypt" moment: a placeholder resolves left to right out of random glyphs.
 *
 * `scrambleFrame` is pure. `decrypt` drives frames on DOM elements from one shared
 * requestAnimationFrame loop; it only ever writes an overlay's text, never the editor
 * content, and the loop stops when the last job ends (nothing runs at idle).
 */

export const GLYPHS = 'ABCDEFGHJKLMNPQRSTUVWXYZ0123456789#$%&*+=<>?/';

/** Characters that keep their place, so the chip's shape reads from the first frame. */
const FIXED = new Set(['[', ']', ':', ' ', '#', '.', '-', '_']);

/** One frame: the first `progress` share is final, the rest random glyphs. Same length always. */
export function scrambleFrame(final: string, progress: number, rand: () => number): string {
  if (progress >= 1) return final;
  const resolved = Math.floor(Math.max(0, progress) * final.length);
  let out = '';
  for (let i = 0; i < final.length; i++) {
    const ch = final[i] ?? '';
    if (i < resolved || FIXED.has(ch) || ch.charCodeAt(0) > 0x7e) out += ch;
    else out += GLYPHS[Math.floor(rand() * GLYPHS.length) % GLYPHS.length];
  }
  return out;
}

interface Job {
  el: HTMLElement;
  final: string;
  start: number;
  duration: number;
  lastSwap: number;
  onDone?: () => void;
}

const jobs = new Set<Job>();
let raf = 0;
/** Glyphs change ~22 times a second; faster reads as noise rather than decoding. */
const SWAP_MS = 45;

const easeOut = (t: number) => 1 - (1 - t) ** 3;

function tick(now: number) {
  for (const job of jobs) {
    if (!job.el.isConnected && now > job.start) {
      jobs.delete(job);
      continue;
    }
    const t = (now - job.start) / job.duration;
    if (t >= 1) {
      job.el.textContent = job.final;
      jobs.delete(job);
      job.onDone?.();
      continue;
    }
    if (now - job.lastSwap < SWAP_MS) continue;
    job.lastSwap = now;
    job.el.textContent = scrambleFrame(job.final, t < 0 ? 0 : easeOut(t), Math.random);
  }
  raf = jobs.size ? requestAnimationFrame(tick) : 0;
}

/** Runs the decrypt effect on `el` (an overlay). Returns a cancel function. */
export function decrypt(
  el: HTMLElement,
  final: string,
  opts: { delay?: number; duration?: number; onDone?: () => void } = {},
): () => void {
  const job: Job = {
    el,
    final,
    start: performance.now() + (opts.delay ?? 0),
    duration: opts.duration ?? 560,
    lastSwap: -Infinity,
    onDone: opts.onDone,
  };
  el.textContent = scrambleFrame(final, 0, Math.random);
  jobs.add(job);
  if (!raf) raf = requestAnimationFrame(tick);
  return () => {
    jobs.delete(job);
  };
}

/** Number of running decrypt jobs (tests and the idle check). */
export const activeDecrypts = (): number => jobs.size;
