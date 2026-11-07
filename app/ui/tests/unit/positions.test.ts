import { describe, expect, test } from 'vitest';
import { ChangeSet, EditorState } from '@codemirror/state';
import {
  addRange,
  mapRanges,
  mergeRanges,
  minimalChange,
  removeOverlapping,
  selectionToManual,
  windowOf,
} from '../../src/lib/editor/positions';
import { inputState } from '../../src/lib/editor/setup';

const at = (s: string) => (pos: number) => s.charCodeAt(pos);

describe('windowOf', () => {
  const items = [
    { start: 0, end: 4 },
    { start: 10, end: 14 },
    { start: 20, end: 30 },
    { start: 40, end: 41 },
  ];
  const w = (from: number, to: number) => windowOf(items, from, to);

  test('finds the ranges that intersect a window', () => {
    expect(w(0, 100)).toEqual([0, 4]);
    expect(w(5, 9)).toEqual([1, 1]); // gap: empty
    expect(w(12, 25)).toEqual([1, 3]);
    expect(w(14, 20)).toEqual([2, 2]); // touching edges do not intersect
    expect(w(29, 40)).toEqual([2, 3]);
    expect(w(50, 60)).toEqual([4, 4]);
  });

  test('works on an empty list and maps positions when asked', () => {
    expect(windowOf([], 0, 10)).toEqual([0, 0]);
    // A mapping that shifts everything by +5: the window must use mapped positions.
    expect(windowOf(items, 15, 19, (p) => p + 5)).toEqual([1, 2]);
  });
});

describe('selectionToManual', () => {
  test('trims whitespace and line breaks off a selection', () => {
    const doc = 'pw =  hunter22 \r\nnext';
    // A triple-click style selection: the whole first line including CRLF.
    expect(selectionToManual(0, 17, at(doc))).toEqual({ start: 0, end: 14 });
    expect(selectionToManual(4, 16, at(doc))).toEqual({ start: 6, end: 14 });
  });

  test('returns null for an empty or blank selection', () => {
    const doc = 'a   b';
    expect(selectionToManual(2, 2, at(doc))).toBeNull();
    expect(selectionToManual(1, 4, at(doc))).toBeNull();
  });

  test('never splits a surrogate pair (UTF-16 offsets around emoji)', () => {
    const doc = 'key 🔑abc';
    const emoji = doc.indexOf('🔑');
    // A selection that starts in the middle of the emoji widens to include all of it.
    expect(selectionToManual(emoji + 1, doc.length, at(doc))).toEqual({ start: emoji, end: doc.length });
    // One that ends inside it widens to its end.
    expect(selectionToManual(0, emoji + 1, at(doc))).toEqual({ start: 0, end: emoji + 2 });
  });
});

describe('manual ranges', () => {
  test('mergeRanges sorts and merges overlapping and touching ranges', () => {
    expect(
      mergeRanges([
        { start: 10, end: 12 },
        { start: 0, end: 3 },
        { start: 2, end: 5 },
        { start: 12, end: 14 },
        { start: 20, end: 20 },
      ]),
    ).toEqual([
      { start: 0, end: 5 },
      { start: 10, end: 14 },
    ]);
  });

  test('addRange keeps the list merged', () => {
    expect(addRange([{ start: 0, end: 3 }], { start: 1, end: 6 })).toEqual([{ start: 0, end: 6 }]);
  });

  test('removeOverlapping drops every manual range that touches a finding', () => {
    const ranges = [
      { start: 0, end: 3 },
      { start: 5, end: 9 },
      { start: 12, end: 14 },
    ];
    expect(removeOverlapping(ranges, { start: 6, end: 13 })).toEqual([{ start: 0, end: 3 }]);
  });

  test('mapRanges follows edits made with CodeMirror (insert before, inside, delete over)', () => {
    const ranges = [
      { start: 4, end: 8 },
      { start: 10, end: 12 },
    ];
    // Insert 3 chars at 0: both shift.
    const ins = ChangeSet.of({ from: 0, insert: 'abc' }, 20);
    expect(mapRanges(ranges, (p, a) => ins.mapPos(p, a))).toEqual([
      { start: 7, end: 11 },
      { start: 13, end: 15 },
    ]);
    // Typing right at a range's start or end does not grow it.
    const edge = ChangeSet.of({ from: 4, insert: 'X' }, 20);
    expect(mapRanges(ranges, (p, a) => edge.mapPos(p, a))[0]).toEqual({ start: 5, end: 9 });
    const tail = ChangeSet.of({ from: 8, insert: 'X' }, 20);
    expect(mapRanges(ranges, (p, a) => tail.mapPos(p, a))[0]).toEqual({ start: 4, end: 8 });
    // Deleting all of the second range removes it.
    const del = ChangeSet.of({ from: 9, to: 13 }, 20);
    expect(mapRanges(ranges, (p, a) => del.mapPos(p, a))).toEqual([{ start: 4, end: 8 }]);
  });
});

describe('minimalChange', () => {
  test('replaces only the part between the common prefix and suffix', () => {
    expect(minimalChange('abcXYZdef', 'abc12def')).toEqual({ from: 3, to: 6, insert: '12' });
    expect(minimalChange('same', 'same')).toBeNull();
    expect(minimalChange('', 'new')).toEqual({ from: 0, to: 0, insert: 'new' });
    expect(minimalChange('old', '')).toEqual({ from: 0, to: 3, insert: '' });
    // Repeated characters: prefix and suffix must not overlap.
    expect(minimalChange('aaa', 'aaaa')).toEqual({ from: 3, to: 3, insert: 'a' });
  });

  test('does not cut a surrogate pair in half', () => {
    // Same high surrogate, different low surrogates: the change must start at the pair.
    const a = 'x😀y';
    const b = 'x😁y';
    const c = minimalChange(a, b);
    expect(c).toEqual({ from: 1, to: 3, insert: '😁' });
  });
});

describe('editor document = UTF-16 text', () => {
  test('CRLF text keeps every offset: the doc round-trips exactly', () => {
    const text = 'a\r\nb 🔑secret\r\n\r\nend\n';
    const state = EditorState.create({ doc: text, extensions: inputState() });
    expect(state.doc.toString()).toBe(text);
    expect(state.doc.length).toBe(text.length);
    const s = text.indexOf('secret');
    expect(state.sliceDoc(s, s + 6)).toBe('secret');
    expect(state.doc.lineAt(s).number).toBe(2);
  });
});
