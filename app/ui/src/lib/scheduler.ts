/**
 * Scan scheduler: debounce, generation counter, stale-response drop.
 *
 * - `schedule()` (typing) waits 250 ms after the last call; `now()` (paste, Keep, Redact,
 *   sensitivity) runs at once. Both bump the generation.
 * - At most one scan is in flight. A scan due while another runs waits for it, then runs once
 *   with the latest request (`build()` is read when the scan starts, never earlier).
 * - A response is applied only if no newer generation exists; everything else is dropped.
 *   `cancel()` (Wipe) bumps the generation, so whatever is in flight lands nowhere.
 */

export const DEBOUNCE_MS = 250;

export interface SchedulerOptions<Req, Res> {
  /** Builds the request from the current state; null means there is nothing to scan. */
  build: () => Req | null;
  run: (req: Req) => Promise<Res>;
  onResult: (res: Res, generation: number) => void;
  onError?: (err: unknown, generation: number) => void;
  /** Called when busy (a scan pending or in flight) flips. */
  onBusy?: (busy: boolean) => void;
  debounceMs?: number;
}

export interface Scheduler {
  schedule(): void;
  now(): void;
  cancel(): void;
  /** Resolves when nothing is pending or in flight. */
  idle(): Promise<void>;
  readonly generation: number;
  readonly busy: boolean;
}

export function createScheduler<Req, Res>(opts: SchedulerOptions<Req, Res>): Scheduler {
  const wait = opts.debounceMs ?? DEBOUNCE_MS;
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let inFlight = false;
  let queued = false;
  let busy = false;
  let waiters: (() => void)[] = [];

  const setBusy = (next: boolean) => {
    if (next === busy) return;
    busy = next;
    opts.onBusy?.(busy);
    if (!busy) {
      const done = waiters;
      waiters = [];
      for (const w of done) w();
    }
  };

  const settleIfIdle = () => {
    if (!timer && !inFlight && !queued) setBusy(false);
  };

  const fire = () => {
    timer = null;
    if (inFlight) {
      queued = true;
      return;
    }
    const gen = generation;
    const req = opts.build();
    if (req === null) {
      settleIfIdle();
      return;
    }
    inFlight = true;
    opts.run(req).then(
      (res) => {
        if (gen === generation) opts.onResult(res, gen);
      },
      (err: unknown) => {
        if (gen === generation) opts.onError?.(err, gen);
      },
    ).finally(() => {
      inFlight = false;
      if (queued) {
        queued = false;
        fire();
      } else {
        settleIfIdle();
      }
    });
  };

  const clearTimer = () => {
    if (timer) clearTimeout(timer);
    timer = null;
  };

  return {
    schedule() {
      generation++;
      clearTimer();
      setBusy(true);
      timer = setTimeout(fire, wait);
    },
    now() {
      generation++;
      clearTimer();
      setBusy(true);
      fire();
    },
    cancel() {
      generation++;
      clearTimer();
      queued = false;
      // An in-flight scan still settles, but its generation is gone, so nothing is applied.
      if (inFlight) setBusy(false);
      settleIfIdle();
    },
    idle() {
      return busy ? new Promise<void>((r) => waiters.push(r)) : Promise.resolve();
    },
    get generation() {
      return generation;
    },
    get busy() {
      return busy;
    },
  };
}
