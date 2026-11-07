import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import { createScheduler, DEBOUNCE_MS } from '../../src/lib/scheduler';

/** A run() whose promises the test settles by hand, in any order. */
function controllable() {
  const pending: { req: string; resolve: (v: string) => void; reject: (e: unknown) => void }[] = [];
  const run = vi.fn(
    (req: string) =>
      new Promise<string>((resolve, reject) => {
        pending.push({ req, resolve, reject });
      }),
  );
  return { run, pending };
}

let text = 'a';
beforeEach(() => {
  vi.useFakeTimers();
  text = 'a';
});
afterEach(() => vi.useRealTimers());

const flush = () => vi.advanceTimersByTimeAsync(0);

describe('scheduler', () => {
  test('debounce is 250 ms', () => {
    expect(DEBOUNCE_MS).toBe(250);
  });

  test('a burst of edits becomes one scan of the latest text', async () => {
    const { run, pending } = controllable();
    const results: string[] = [];
    const s = createScheduler({ build: () => text, run, onResult: (r) => results.push(r) });
    for (const t of ['ab', 'abc', 'abcd']) {
      text = t;
      s.schedule();
      await vi.advanceTimersByTimeAsync(100);
    }
    expect(run).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    expect(run).toHaveBeenCalledTimes(1);
    expect(run).toHaveBeenCalledWith('abcd');
    pending[0]?.resolve('R(abcd)');
    await flush();
    expect(results).toEqual(['R(abcd)']);
  });

  test('now() skips the debounce', async () => {
    const { run } = controllable();
    const s = createScheduler({ build: () => text, run, onResult: () => {} });
    s.now();
    await flush();
    expect(run).toHaveBeenCalledTimes(1);
  });

  test('stale_responses_dropped', async () => {
    const { run, pending } = controllable();
    const results: string[] = [];
    const s = createScheduler({ build: () => text, run, onResult: (r) => results.push(r) });

    text = 'one';
    s.now();
    await flush();
    expect(run).toHaveBeenCalledTimes(1);

    // The text changes while the first scan is in flight: the new scan waits for it,
    // and the first response is dropped because a newer generation exists.
    text = 'two';
    s.schedule();
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    expect(run).toHaveBeenCalledTimes(1); // at most one scan in flight

    pending[0]?.resolve('R(one)');
    await flush();
    expect(results).toEqual([]); // stale, dropped
    expect(run).toHaveBeenCalledTimes(2);
    expect(run).toHaveBeenLastCalledWith('two');

    pending[1]?.resolve('R(two)');
    await flush();
    expect(results).toEqual(['R(two)']);
  });

  test('a response that arrives after a newer one was scheduled is dropped even if it resolves last', async () => {
    const { run, pending } = controllable();
    const results: string[] = [];
    const s = createScheduler({ build: () => text, run, onResult: (r) => results.push(r) });
    text = 'one';
    s.now();
    await flush();
    text = 'two';
    s.schedule(); // pending timer, generation bumped
    pending[0]?.resolve('R(one)');
    await flush();
    expect(results).toEqual([]);
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    pending[1]?.resolve('R(two)');
    await flush();
    expect(results).toEqual(['R(two)']);
  });

  test('cancel drops the pending and the in-flight scan', async () => {
    const { run, pending } = controllable();
    const results: string[] = [];
    const s = createScheduler({ build: () => text, run, onResult: (r) => results.push(r) });
    s.now();
    await flush();
    s.schedule();
    s.cancel();
    pending[0]?.resolve('R(a)');
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS * 2);
    expect(results).toEqual([]);
    expect(run).toHaveBeenCalledTimes(1);
    expect(s.busy).toBe(false);
  });

  test('errors are reported only for the latest generation', async () => {
    const { run, pending } = controllable();
    const errors: unknown[] = [];
    const results: string[] = [];
    const s = createScheduler({
      build: () => text,
      run,
      onResult: (r) => results.push(r),
      onError: (e) => errors.push(e),
    });
    s.now();
    await flush();
    s.schedule();
    pending[0]?.reject(new Error('old'));
    await flush();
    expect(errors).toEqual([]);
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    pending[1]?.reject(new Error('new'));
    await flush();
    expect(errors.map((e) => (e as Error).message)).toEqual(['new']);
    expect(results).toEqual([]);
  });

  test('a null request runs nothing and leaves the scheduler idle', async () => {
    const { run } = controllable();
    const busy: boolean[] = [];
    const s = createScheduler({ build: () => null, run, onResult: () => {}, onBusy: (b) => busy.push(b) });
    s.now();
    await flush();
    expect(run).not.toHaveBeenCalled();
    expect(s.busy).toBe(false);
    expect(busy.at(-1)).toBe(false);
  });

  test('busy is true from schedule until the latest response lands', async () => {
    const { run, pending } = controllable();
    const busy: boolean[] = [];
    const s = createScheduler({ build: () => text, run, onResult: () => {}, onBusy: (b) => busy.push(b) });
    s.schedule();
    expect(s.busy).toBe(true);
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    expect(s.busy).toBe(true);
    pending[0]?.resolve('R');
    await flush();
    expect(s.busy).toBe(false);
    expect(busy).toEqual([true, false]);
  });

  test('idle() resolves once nothing is pending or in flight', async () => {
    const { run, pending } = controllable();
    const s = createScheduler({ build: () => text, run, onResult: () => {} });
    await expect(s.idle()).resolves.toBeUndefined(); // already idle
    s.now();
    let settled = false;
    void s.idle().then(() => (settled = true));
    await flush();
    expect(settled).toBe(false);
    pending[0]?.resolve('R');
    await flush();
    expect(settled).toBe(true);
  });
});
