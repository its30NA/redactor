/** Toast queue. Components call `toasts.show(...)`; `<Toaster>` renders the queue. */

export type ToastTone = 'success' | 'info' | 'warning' | 'error';

export interface ToastInput {
  tone?: ToastTone;
  title: string;
  detail?: string;
  /** ms before it leaves on its own; 0 keeps it until dismissed. */
  duration?: number;
  /** A button on the toast; clicking it runs `run` and closes the toast (`toasts.act`). */
  action?: ToastAction;
}

export interface ToastAction {
  label: string;
  run: () => void;
}

export interface ToastItem extends Required<Omit<ToastInput, 'detail' | 'action'>> {
  id: number;
  detail?: string;
  action?: ToastAction;
}

const MAX_VISIBLE = 3;

class Toasts {
  items = $state<ToastItem[]>([]);
  #next = 1;
  #timers = new Map<number, ReturnType<typeof setTimeout>>();

  show(input: ToastInput): number {
    const item: ToastItem = {
      id: this.#next++,
      tone: input.tone ?? 'info',
      title: input.title,
      detail: input.detail,
      duration: input.duration ?? 3600,
      action: input.action,
    };
    // Over the limit, the oldest earlier toast that leaves on its own makes room; one that stays
    // until dismissed (a clipboard left raw) goes only when every earlier one does.
    const earlier = [...this.items];
    while (earlier.length >= MAX_VISIBLE) {
      const drop = earlier.find((t) => t.duration > 0) ?? earlier[0]!;
      earlier.splice(earlier.indexOf(drop), 1);
      clearTimeout(this.#timers.get(drop.id));
      this.#timers.delete(drop.id);
    }
    this.items = [...earlier, item];
    if (item.duration > 0) {
      this.#timers.set(
        item.id,
        setTimeout(() => this.dismiss(item.id), item.duration),
      );
    }
    return item.id;
  }

  /** The toast's button was clicked: closes the toast and runs its action. */
  act(id: number): void {
    const action = this.items.find((t) => t.id === id)?.action;
    this.dismiss(id);
    action?.run();
  }

  dismiss(id: number): void {
    clearTimeout(this.#timers.get(id));
    this.#timers.delete(id);
    this.items = this.items.filter((t) => t.id !== id);
  }

  clear(): void {
    for (const t of this.items) this.dismiss(t.id);
  }
}

export const toasts = new Toasts();
