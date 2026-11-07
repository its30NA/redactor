/**
 * Session secrets as the UI knows them: ids, labels and times only. The values live in the
 * shell's memory and never come back over IPC. The list follows `session-changed` events and
 * empties on `wipe` (the shell clears it too).
 */
import { sessionAdd, sessionClear, sessionForget, sessionList, sessionRemember } from './api';
import { toasts } from './toast.svelte';
import type { SessionSecret } from './types';

class Session {
  items = $state.raw<SessionSecret[]>([]);
  loaded = $state(false);

  async load(): Promise<void> {
    try {
      this.items = await sessionList();
    } catch {
      /* the list stays empty; nothing to remember yet */
    } finally {
      this.loaded = true;
    }
  }

  set(items: SessionSecret[]): void {
    this.items = items;
  }

  async remember(text: string, start: number, end: number, label: string): Promise<boolean> {
    return this.#run(() => sessionRemember(text, start, end, label), 'Could not remember it');
  }

  async add(secret: string, label: string): Promise<boolean> {
    return this.#run(() => sessionAdd(secret, label), 'Could not remember it');
  }

  async forget(id: string): Promise<boolean> {
    return this.#run(() => sessionForget(id), 'Could not forget it');
  }

  async clear(): Promise<boolean> {
    return this.#run(() => sessionClear(), 'Could not forget them');
  }

  async #run(call: () => Promise<SessionSecret[]>, failure: string): Promise<boolean> {
    try {
      this.items = await call();
      return true;
    } catch {
      toasts.show({ tone: 'error', title: failure });
      return false;
    }
  }
}

export const session = new Session();
