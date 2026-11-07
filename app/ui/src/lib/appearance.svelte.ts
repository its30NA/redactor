/**
 * Resolved theme and motion. Settings say 'system' | 'dark' | 'light' and
 * 'system' | 'on' | 'off' (reduced motion on/off); this module resolves them against the OS
 * and mirrors the result onto <html> (data-theme, data-motion) for the CSS.
 */
import { MediaQuery } from 'svelte/reactivity';
import { settings } from './settings.svelte';

const prefersDark = new MediaQuery('(prefers-color-scheme: dark)', true);
const prefersReduced = new MediaQuery('(prefers-reduced-motion: reduce)', false);

export const appearance = {
  get scheme(): 'dark' | 'light' {
    const t = settings.value.theme;
    return t === 'system' ? (prefersDark.current ? 'dark' : 'light') : t;
  },
  get reducedMotion(): boolean {
    const m = settings.value.reducedMotion;
    return m === 'on' || (m === 'system' && prefersReduced.current);
  },
  /** A transition duration that collapses to 0 under reduced motion. */
  ms(duration: number): number {
    return this.reducedMotion ? 0 : duration;
  },
};

/** Mirror the settings onto the root element. Call from an $effect. */
export function applyAppearance(root: HTMLElement = document.documentElement): void {
  const { theme, reducedMotion } = settings.value;
  if (theme === 'system') delete root.dataset.theme;
  else root.dataset.theme = theme;
  if (reducedMotion === 'system') delete root.dataset.motion;
  else root.dataset.motion = reducedMotion === 'on' ? 'reduce' : 'full';
}
