<!--
  Side sheet (modal dialog) sliding in from the right, below the title bar so the window can
  still be moved and closed. Esc or a click on the scrim closes it; focus moves in on open,
  stays inside while open (Tab wraps), and returns to the opener on close.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import { fade, fly } from 'svelte/transition';
  import { quintOut } from 'svelte/easing';
  import X from '@lucide/svelte/icons/x';
  import Button from './Button.svelte';
  import { appearance } from '../lib/appearance.svelte';

  let {
    open = $bindable(false),
    title,
    description,
    width = 440,
    children,
    footer,
    onclose,
  }: {
    open: boolean;
    title: string;
    description?: string;
    width?: number;
    children: Snippet;
    footer?: Snippet;
    onclose?: () => void;
  } = $props();

  const uid = $props.id();
  let panel: HTMLElement | undefined = $state();

  const FOCUSABLE =
    'button:not([disabled]), [href], input:not([disabled]), select, textarea, [tabindex]:not([tabindex="-1"])';

  $effect(() => {
    if (!open) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    queueMicrotask(() => {
      const first = panel?.querySelector<HTMLElement>('[data-autofocus]') ?? panel;
      first?.focus();
    });
    return () => opener?.focus();
  });

  function close() {
    open = false;
    onclose?.();
  }

  function onkeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
      return;
    }
    if (e.key === 'Tab' && panel) {
      // Roving-tabindex groups (segmented controls) expose only their checked item to Tab.
      const items = [...panel.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
        (el) => el.tabIndex >= 0 && el.getClientRects().length > 0,
      );
      const first = items[0];
      const last = items.at(-1);
      if (!first || !last) return;
      if (e.shiftKey && (document.activeElement === first || document.activeElement === panel)) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }
</script>

<svelte:window {onkeydown} />

{#if open}
  <div class="scrim" role="presentation" onclick={close} transition:fade={{ duration: appearance.ms(220) }}></div>
  <div
    bind:this={panel}
    class="sheet"
    role="dialog"
    aria-modal="true"
    aria-labelledby="{uid}-title"
    aria-describedby={description ? `${uid}-desc` : undefined}
    tabindex="-1"
    style:width="{width}px"
    transition:fly={{ x: 32, duration: appearance.ms(420), easing: quintOut, opacity: 0 }}
  >
    <header>
      <div class="heading">
        <h2 id="{uid}-title">{title}</h2>
        {#if description}<p id="{uid}-desc">{description}</p>{/if}
      </div>
      <Button variant="ghost" size="sm" icon={X} label="Close" onclick={close} />
    </header>
    <div class="body">
      {@render children()}
    </div>
    {#if footer}
      <footer>{@render footer()}</footer>
    {/if}
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: var(--titlebar-h) 0 0 0;
    z-index: var(--z-sheet);
    background: var(--scrim);
    backdrop-filter: blur(3px);
  }
  .sheet {
    position: fixed;
    z-index: var(--z-sheet);
    top: calc(var(--titlebar-h) + 6px);
    right: var(--gutter);
    bottom: var(--gutter);
    max-width: calc(100vw - 2 * var(--gutter));
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-xl);
    background: var(--glass-strong);
    backdrop-filter: blur(28px) saturate(1.5);
    box-shadow:
      inset 0 0 0 1px var(--border),
      var(--shadow-float);
    overflow: hidden;
  }
  .sheet:focus-visible {
    box-shadow:
      inset 0 0 0 1px var(--focus),
      var(--shadow-float);
  }
  /* A soft gradient bloom along the top edge. */
  .sheet::before {
    content: '';
    position: absolute;
    inset: 0 0 auto 0;
    height: 140px;
    background: radial-gradient(80% 100% at 20% 0%, var(--cyan-tint), transparent 70%),
      radial-gradient(70% 100% at 90% 0%, var(--violet-tint), transparent 70%);
    pointer-events: none;
  }
  header {
    position: relative;
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    padding: 20px 16px 16px 22px;
  }
  h2 {
    font-size: var(--text-lg);
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .heading p {
    margin-top: 4px;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .body {
    position: relative;
    flex: 1;
    overflow: auto;
    padding: 4px 22px 22px;
  }
  footer {
    position: relative;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 14px 16px;
    border-top: 1px solid var(--hairline);
  }
</style>
