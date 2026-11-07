<!--
  The status pill in the title bar: a live dot and short segments, e.g. "Offline · Shield on".
  Tones: ok (mint), busy (cyan, faster pulse), warn (amber), alert (red).
  aria-live, so a screen reader hears "3 secrets neutralized · verified" when it changes.
-->
<script lang="ts" module>
  import type { LucideIcon } from '@lucide/svelte';

  export interface PillSegment {
    text: string;
    icon?: LucideIcon;
  }
</script>

<script lang="ts">
  let {
    tone = 'ok',
    segments,
    announce,
  }: {
    tone?: 'ok' | 'busy' | 'warn' | 'alert';
    segments: PillSegment[];
    /** What a screen reader hears, when the visible text animates (a count rolling up). */
    announce?: string;
  } = $props();
</script>

<div class="pill {tone}" role="status" aria-live="polite" data-tauri-drag-region data-testid="status-pill">
  <span class="dot" aria-hidden="true"><span class="ring ambient"></span></span>
  {#each segments as seg, i (i)}
    {#if i > 0}<span class="sep" aria-hidden="true">·</span>{/if}
    <span class="seg" aria-hidden={announce ? 'true' : undefined}>
      {#if seg.icon}<seg.icon size={13} strokeWidth={2} aria-hidden="true" />{/if}
      {seg.text}
    </span>
  {/each}
  {#if announce}<span class="sr-only">{announce}</span>{/if}
</div>

<style>
  .pill {
    --tone: var(--mint);
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 26px;
    padding: 0 12px 0 11px;
    border-radius: 999px;
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--text-2);
    white-space: nowrap;
    background: light-dark(rgb(255 255 255 / 0.6), rgb(255 255 255 / 0.035));
    box-shadow:
      inset 0 0 0 1px var(--border),
      inset 0 1px 0 0 var(--highlight),
      0 4px 14px -6px light-dark(rgb(11 16 32 / 0.12), rgb(0 0 0 / 0.5));
    pointer-events: auto;
  }
  .busy {
    --tone: var(--cyan);
  }
  .warn {
    --tone: var(--amber);
  }
  .alert {
    --tone: var(--red);
  }

  .dot {
    position: relative;
    width: 7px;
    height: 7px;
    margin-right: 1px;
    border-radius: 50%;
    background: var(--tone);
    box-shadow: 0 0 8px 1px color-mix(in srgb, var(--tone) 60%, transparent);
  }
  .ring {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: var(--tone);
    animation: pulse-ring 2.8s var(--ease-out) infinite;
    will-change: transform, opacity;
  }
  .busy .ring {
    animation-duration: 1.1s;
  }

  .seg {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .seg :global(svg) {
    color: var(--tone);
  }
  .sep {
    color: var(--muted);
    margin: 0 -1px;
  }
</style>
