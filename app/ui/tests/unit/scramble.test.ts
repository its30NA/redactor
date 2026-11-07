import { describe, expect, test } from 'vitest';
import { GLYPHS, scrambleFrame } from '../../src/lib/scramble';

const seq = (...values: number[]) => {
  let i = 0;
  return () => values[i++ % values.length] ?? 0;
};

describe('scramble (decrypt effect)', () => {
  const final = '[REDACTED: Password #2]';

  test('keeps the length, so the overlay lines up with the chip in a monospace font', () => {
    for (const p of [0, 0.1, 0.5, 0.99]) expect(scrambleFrame(final, p, Math.random)).toHaveLength(final.length);
  });

  test('ends on the exact text', () => {
    expect(scrambleFrame(final, 1, Math.random)).toBe(final);
    expect(scrambleFrame(final, 1.4, Math.random)).toBe(final);
  });

  test('resolves left to right and keeps the structure (brackets, colon, spaces, #)', () => {
    const half = scrambleFrame(final, 0.5, seq(0.99));
    const cut = Math.floor(final.length * 0.5);
    expect(half.slice(0, cut)).toBe(final.slice(0, cut));
    for (let i = cut; i < final.length; i++) {
      const ch = final[i] ?? '';
      if ('[]: #'.includes(ch)) expect(half[i]).toBe(ch);
      else expect(GLYPHS).toContain(half[i]);
    }
  });

  test('at the start nothing but the structure is readable', () => {
    const start = scrambleFrame('abc def', 0, seq(0));
    expect(start).toBe(`${GLYPHS[0]}${GLYPHS[0]}${GLYPHS[0]} ${GLYPHS[0]}${GLYPHS[0]}${GLYPHS[0]}`);
  });

  test('non-ASCII characters are left alone (no width surprises)', () => {
    expect(scrambleFrame('Schlüssel', 0, seq(0))).toContain('ü');
  });
});
