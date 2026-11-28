/** The toast queue: at most three on screen, and a toast meant to stay is the last to make room. */
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { toasts } from '../../src/lib/toast.svelte';

const titles = () => toasts.items.map((t) => t.title);

beforeEach(() => {
  vi.useFakeTimers();
  toasts.clear();
});

afterEach(() => {
  toasts.clear();
  vi.useRealTimers();
});

describe('over the limit', () => {
  test('the oldest toast that leaves on its own makes room', () => {
    toasts.show({ title: 'a' });
    toasts.show({ title: 'b' });
    toasts.show({ title: 'c' });
    toasts.show({ title: 'd' });
    expect(titles()).toEqual(['b', 'c', 'd']);
  });

  test('a toast that stays until dismissed is kept while a timed one can go', () => {
    toasts.show({ title: 'Not scrubbed', duration: 0 });
    toasts.show({ title: 'b' });
    toasts.show({ title: 'c' });
    toasts.show({ title: 'd' });
    expect(titles()).toEqual(['Not scrubbed', 'c', 'd']);
  });

  test('the new toast is never the one dropped', () => {
    toasts.show({ title: 'a', duration: 0 });
    toasts.show({ title: 'b', duration: 0 });
    toasts.show({ title: 'c', duration: 0 });
    toasts.show({ title: 'd' });
    expect(titles()).toEqual(['b', 'c', 'd']);
  });

  test('when all stay until dismissed, the oldest goes', () => {
    for (const t of ['a', 'b', 'c', 'd']) toasts.show({ title: t, duration: 0 });
    expect(titles()).toEqual(['b', 'c', 'd']);
  });

  test("a dropped toast's timer is cleared", () => {
    toasts.show({ title: 'a' });
    toasts.show({ title: 'b' });
    toasts.show({ title: 'c' });
    expect(vi.getTimerCount()).toBe(3);
    toasts.show({ title: 'd' });
    expect(vi.getTimerCount()).toBe(3);
  });
});

test('a timed toast leaves after its duration', () => {
  toasts.show({ title: 'a', duration: 1000 });
  vi.advanceTimersByTime(999);
  expect(titles()).toEqual(['a']);
  vi.advanceTimersByTime(1);
  expect(titles()).toEqual([]);
});
