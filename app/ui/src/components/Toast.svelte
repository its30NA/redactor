<!--
  Toast region, bottom centre above the dock. Renders the `toasts` queue; each toast carries a
  hairline that drains over its lifetime (a scaleX transform). Polite live region. Esc on a
  focused toast dismisses it (not the sheet behind it) and hands focus back to where it was.
-->
<script lang="ts">
  import { fly } from 'svelte/transition';
  import { flip } from 'svelte/animate';
  import { quintOut } from 'svelte/easing';
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import CircleX from '@lucide/svelte/icons/circle-x';
  import Info from '@lucide/svelte/icons/info';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import X from '@lucide/svelte/icons/x';
  import { appearance } from '../lib/appearance.svelte';
  import { toasts, type ToastTone } from '../lib/toast.svelte';

  const ICONS = { success: CircleCheck, info: Info, warning: TriangleAlert, error: CircleX } satisfies Record<
    ToastTone,
    unknown
  >;

  let region: HTMLDivElement | undefined = $state();
  /** Where focus was before it came into a toast. */
  let returnTo: HTMLElement | null = null;

  function onfocus(e: FocusEvent) {
    const from = e.relatedTarget;
    if (from instanceof HTMLElement && !region?.contains(from)) returnTo = from;
  }

  function onkeydown(e: KeyboardEvent, id: number) {
    if (e.key !== 'Escape' || e.shiftKey || e.ctrlKey || e.altKey || e.metaKey) return;
    e.preventDefault();
    e.stopPropagation();
    toasts.dismiss(id);
    // The toast stays in the page while it flies out: focus must not stay on it.
    if (returnTo?.isConnected) returnTo.focus();
    else (e.currentTarget as HTMLElement).blur();
    returnTo = null;
  }
</script>

<div class="region" role="status" aria-live="polite" aria-relevant="additions" bind:this={region}>
  {#each toasts.items as t (t.id)}
    {@const Icon = ICONS[t.tone]}
    <div
      class="toast {t.tone}"
      animate:flip={{ duration: appearance.ms(260), easing: quintOut }}
      transition:fly={{ y: 10, duration: appearance.ms(300), easing: quintOut }}
    >
      <Icon size={16} strokeWidth={2} aria-hidden="true" />
      <div class="copy">
        <span class="title">{t.title}</span>
        {#if t.detail}<span class="detail">{t.detail}</span>{/if}
      </div>
      {#if t.action}
        <button type="button" class="action" onclick={() => toasts.act(t.id)} {onfocus} onkeydown={(e) => onkeydown(e, t.id)}>
          {t.action.label}
        </button>
      {/if}
      <button
        type="button"
        class="dismiss"
        aria-label="Dismiss"
        onclick={() => toasts.dismiss(t.id)}
        {onfocus}
        onkeydown={(e) => onkeydown(e, t.id)}
      >
        <X size={14} aria-hidden="true" />
      </button>
      {#if t.duration > 0 && !appearance.reducedMotion}
        <span class="life" style:animation-duration="{t.duration}ms" aria-hidden="true"></span>
      {/if}
    </div>
  {/each}
</div>

<style>
  .region {
    position: fixed;
    z-index: var(--z-toast);
    left: 50%;
    bottom: 96px;
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    pointer-events: none;
  }
  .toast {
    --tone: var(--cyan);
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 260px;
    max-width: min(520px, calc(100vw - 48px));
    padding: 10px 8px 10px 14px;
    border-radius: 14px;
    background: var(--glass-strong);
    backdrop-filter: blur(20px) saturate(1.5);
    box-shadow:
      inset 0 0 0 1px var(--border),
      var(--shadow-float);
    overflow: hidden;
    pointer-events: auto;
  }
  .success {
    --tone: var(--mint);
  }
  .warning {
    --tone: var(--amber);
  }
  .error {
    --tone: var(--red);
  }
  .toast > :global(svg) {
    color: var(--tone);
    filter: drop-shadow(0 0 6px color-mix(in srgb, var(--tone) 50%, transparent));
  }
  .copy {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }
  .title {
    font-weight: 500;
    color: var(--text);
  }
  .detail {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .action {
    flex: none;
    padding: 4px 10px;
    border-radius: var(--radius-xs);
    font-weight: 500;
    color: var(--tone);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--tone) 45%, transparent);
  }
  .action:hover {
    background: var(--glass-hover);
  }
  .dismiss {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-xs);
    color: var(--muted);
  }
  .dismiss:hover {
    background: var(--glass-hover);
    color: var(--text);
  }
  .life {
    position: absolute;
    left: 14px;
    right: 14px;
    bottom: 0;
    height: 2px;
    border-radius: 2px;
    background: var(--tone);
    opacity: 0.55;
    transform-origin: left center;
    animation: shrink-x linear forwards;
  }
</style>
