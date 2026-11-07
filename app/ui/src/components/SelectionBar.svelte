<!--
  The floating "Redact · Remember" bar over a selection in the Input. It sits above the first
  line of the selection (below it near the top edge) and follows scrolling. The buttons keep
  the editor's focus and selection on mouse down.
-->
<script lang="ts">
  import { fly } from 'svelte/transition';
  import { quintOut } from 'svelte/easing';
  import type { EditorView } from '@codemirror/view';
  import BookmarkPlus from '@lucide/svelte/icons/bookmark-plus';
  import EyeOff from '@lucide/svelte/icons/eye-off';
  import { appearance } from '../lib/appearance.svelte';
  import { MIN_SESSION_SECRET } from '../lib/types';
  import { workspace } from '../lib/workspace.svelte';

  let { view, container }: { view: EditorView; container: HTMLElement } = $props();

  let pos = $state<{ x: number; y: number; below: boolean } | null>(null);
  let bar: HTMLDivElement | undefined = $state();
  let frame = 0;

  const sel = $derived(workspace.selection);
  const length = $derived(sel ? sel.end - sel.start : 0);

  function measure() {
    frame = 0;
    const s = workspace.selection;
    if (!s) {
      pos = null;
      return;
    }
    const from = view.coordsAtPos(s.start, 1);
    const to = view.coordsAtPos(s.end, -1);
    if (!from || !to) {
      pos = null;
      return;
    }
    const box = container.getBoundingClientRect();
    const width = bar?.offsetWidth ?? 220;
    const sameLine = Math.abs(from.top - to.top) < 4;
    const center = sameLine ? (from.left + to.right) / 2 : from.left + 60;
    const x = Math.min(Math.max(center - box.left - width / 2, 8), box.width - width - 8);
    const above = from.top - box.top - 44;
    const below = above < 6;
    const y = below ? to.bottom - box.top + 8 : above;
    if (y < 0 || y > box.height - 30) {
      pos = null; // scrolled out of view
      return;
    }
    pos = { x, y, below };
  }

  const schedule = () => {
    if (!frame) frame = requestAnimationFrame(measure);
  };

  $effect(() => {
    void workspace.selection;
    schedule();
  });

  $effect(() => {
    const scroller = view.scrollDOM;
    scroller.addEventListener('scroll', schedule, { passive: true });
    window.addEventListener('resize', schedule);
    return () => {
      scroller.removeEventListener('scroll', schedule);
      window.removeEventListener('resize', schedule);
      cancelAnimationFrame(frame);
    };
  });

  const keep = (e: MouseEvent) => e.preventDefault();

  const redactTitle = $derived(
    sel?.keptKey ? 'Redact this value again' : sel?.covered ? 'Already redacted' : 'Redact the selection everywhere it appears',
  );
  const rememberTitle = $derived(
    length < MIN_SESSION_SECRET
      ? `Select at least ${MIN_SESSION_SECRET} characters to remember`
      : 'Remember for this session: redacted wherever it shows up until you quit',
  );
</script>

{#if sel && pos}
  <div
    bind:this={bar}
    class="selbar"
    class:below={pos.below}
    role="toolbar"
    aria-label="Selection"
    data-selbar
    data-testid="selection-bar"
    style:transform="translate({pos.x}px, {pos.y}px)"
    transition:fly={{ y: pos.below ? -4 : 4, duration: appearance.ms(180), easing: quintOut }}
  >
    <button
      type="button"
      class="act redact"
      onmousedown={keep}
      onclick={() => workspace.redactSelection()}
      disabled={sel.covered && !sel.keptKey}
      title={redactTitle}
    >
      <EyeOff size={14} strokeWidth={2} aria-hidden="true" />
      {sel.covered && !sel.keptKey ? 'Redacted' : 'Redact'}
    </button>
    <span class="dot" aria-hidden="true">·</span>
    <button
      type="button"
      class="act"
      onmousedown={keep}
      onclick={() => workspace.rememberSelection()}
      disabled={length < MIN_SESSION_SECRET}
      title={rememberTitle}
    >
      <BookmarkPlus size={14} strokeWidth={2} aria-hidden="true" />
      Remember
    </button>
    <span class="len mono num" aria-label="{length} characters selected">{length}</span>
  </div>
{/if}

<style>
  .selbar {
    position: absolute;
    left: 0;
    top: 0;
    z-index: 8;
    display: flex;
    align-items: center;
    gap: 2px;
    height: 34px;
    padding: 0 4px;
    border-radius: 11px;
    background: var(--glass-strong);
    backdrop-filter: blur(16px) saturate(1.4);
    box-shadow:
      inset 0 0 0 1px var(--border-strong),
      var(--shadow-float),
      0 0 24px -8px var(--grad-glow);
    white-space: nowrap;
  }
  .selbar::after {
    content: '';
    position: absolute;
    left: 50%;
    bottom: -4px;
    width: 8px;
    height: 8px;
    margin-left: -4px;
    background: var(--glass-strong);
    box-shadow: inset -1px -1px 0 0 var(--border-strong);
    transform: rotate(45deg);
  }
  .selbar.below::after {
    top: -4px;
    bottom: auto;
    box-shadow: inset 1px 1px 0 0 var(--border-strong);
  }
  .act {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 9px;
    border-radius: 8px;
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text);
    transition:
      background-color var(--dur-1) ease,
      color var(--dur-1) ease;
  }
  .act:hover {
    background: var(--glass-hover);
  }
  .redact {
    color: var(--red);
  }
  .redact:hover {
    background: var(--red-tint);
  }
  .act:disabled {
    opacity: 0.45;
    pointer-events: none;
  }
  .dot {
    color: var(--faint);
  }
  .len {
    margin: 0 6px 0 4px;
    font-size: var(--text-xs);
    color: var(--muted);
  }
</style>
