/**
 * Position maths for the editors. Everything is in UTF-16 code units, which is what both
 * CodeMirror and the IPC contract use. The editors keep CR characters in the
 * document (line separator "\n" only), so an editor position IS an offset into the text the
 * engine scanned, including after CRLF lines and astral characters.
 */

export interface Range {
  start: number;
  end: number;
}

type MapPos = (pos: number, assoc: number) => number;

const identity = (p: number) => p;

/**
 * Index window [lo, hi) of the items that intersect [from, to). Items must be sorted by start
 * and must not overlap (the engine's resolve step guarantees it), so their ends are sorted
 * too. `map` translates item positions first (findings after later edits).
 */
export function windowOf<T extends Range>(
  items: readonly T[],
  from: number,
  to: number,
  map: (pos: number, assoc: number) => number = identity,
): [number, number] {
  // First item whose end is after `from`.
  let lo = 0;
  let hi = items.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    const item = items[mid] as T;
    if (map(item.end, -1) <= from) lo = mid + 1;
    else hi = mid;
  }
  const first = lo;
  // First item whose start is at or after `to`.
  hi = items.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    const item = items[mid] as T;
    if (map(item.start, 1) < to) lo = mid + 1;
    else hi = mid;
  }
  return [first, lo];
}

const isSpace = (c: number) => c === 32 || c === 9 || c === 10 || c === 13 || c === 0xa0 || c === 0x3000;
const isHigh = (c: number) => c >= 0xd800 && c <= 0xdbff;
const isLow = (c: number) => c >= 0xdc00 && c <= 0xdfff;

/**
 * Turns an editor selection into a manual range: trims whitespace and line breaks from both
 * ends and widens it so it never splits a surrogate pair. `codeAt(pos)` returns the UTF-16
 * code unit at a document position (NaN outside it). Null when nothing is left.
 */
export function selectionToManual(from: number, to: number, codeAt: (pos: number) => number): Range | null {
  let start = from;
  let end = to;
  while (start < end && isSpace(codeAt(start))) start++;
  while (end > start && isSpace(codeAt(end - 1))) end--;
  if (start >= end) return null;
  if (isLow(codeAt(start)) && isHigh(codeAt(start - 1))) start--;
  if (isLow(codeAt(end)) && isHigh(codeAt(end - 1))) end++;
  return { start, end };
}

/** Sorted, with overlapping and touching ranges merged and empty ones dropped. */
export function mergeRanges(ranges: readonly Range[]): Range[] {
  const sorted = ranges.filter((r) => r.end > r.start).sort((x, y) => x.start - y.start || x.end - y.end);
  const out: Range[] = [];
  for (const r of sorted) {
    const last = out.at(-1);
    if (last && r.start <= last.end) last.end = Math.max(last.end, r.end);
    else out.push({ start: r.start, end: r.end });
  }
  return out;
}

export const addRange = (ranges: readonly Range[], r: Range): Range[] => mergeRanges([...ranges, r]);

/** Drops every range that overlaps `r` (un-redacting a manual finding). */
export const removeOverlapping = (ranges: readonly Range[], r: Range): Range[] =>
  ranges.filter((m) => m.end <= r.start || m.start >= r.end);

/**
 * Maps ranges through a document change. Text typed right at either edge stays outside the
 * range (start maps forward, end maps backward); a range whose text is deleted disappears.
 */
export function mapRanges(ranges: readonly Range[], mapPos: MapPos): Range[] {
  const out: Range[] = [];
  for (const r of ranges) {
    const start = mapPos(r.start, 1);
    const end = mapPos(r.end, -1);
    if (end > start) out.push({ start, end });
  }
  return out;
}

/**
 * The smallest single replacement that turns `prev` into `next` (common prefix and suffix
 * kept), so a re-scan only redraws the output lines that changed. Null when equal.
 */
export function minimalChange(prev: string, next: string): { from: number; to: number; insert: string } | null {
  if (prev === next) return null;
  const max = Math.min(prev.length, next.length);
  let p = 0;
  while (p < max && prev.charCodeAt(p) === next.charCodeAt(p)) p++;
  if (p > 0 && p < max && isHigh(prev.charCodeAt(p - 1))) p--;
  let s = 0;
  const maxS = max - p;
  while (s < maxS && prev.charCodeAt(prev.length - 1 - s) === next.charCodeAt(next.length - 1 - s)) s++;
  if (s > 0 && s < maxS && isLow(prev.charCodeAt(prev.length - s))) s--;
  return { from: p, to: prev.length - s, insert: next.slice(p, next.length - s) };
}
