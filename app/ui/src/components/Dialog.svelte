<!--
  Modal dialog, centered (or near the top for the command palette). Esc and a click on the
  scrim close it; Esc stops there, so a sheet underneath stays open. Focus moves in on open,
  Tab stays inside, and focus returns to the opener on close.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import { fade, scale } from 'svelte/transition';
  import { quintOut } from 'svelte/easing';
  import { appearance } from '../lib/appearance.svelte';

  let {
    open = $bindable(false),
    label,
    width = 420,
    placement = 'center',
    children,
    onclose,
    testid,
  }: {
    open: boolean;
    /** Accessible name (the visible title is part of `children`). */
    label: string;
    width?: number;
    placement?: 'center' | 'top';
    children: Snippet;
    onclose?: () => void;
    testid?: string;
  } = $props();

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
    return () => {
      if (opener?.isConnected) opener.focus();
    };
  });

  function close() {
    open = false;
    onclose?.();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
      return;
    }
    if (e.key === 'Tab' && panel) {
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

{#if open}
  <div class="layer {placement}" role="presentation" {onkeydown}>
    <div class="scrim" role="presentation" onclick={close} transition:fade={{ duration: appearance.ms(180) }}></div>
    <div
      bind:this={panel}
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-label={label}
      tabindex="-1"
      data-testid={testid}
      style:width="{width}px"
      transition:scale={{ start: 0.96, duration: appearance.ms(260), easing: quintOut, opacity: 0 }}
    >
      {@render children()}
    </div>
  </div>
{/if}

<style>
  .layer {
    position: fixed;
    inset: var(--titlebar-h) 0 0 0;
    z-index: calc(var(--z-sheet) + 2);
    display: flex;
    justify-content: center;
    align-items: center;
    padding: 16px;
  }
  .layer.top {
    align-items: flex-start;
    padding-top: 10vh;
  }
  .scrim {
    position: absolute;
    inset: 0;
    background: var(--scrim);
    backdrop-filter: blur(3px);
  }
  .dialog {
    position: relative;
    max-width: 100%;
    max-height: 100%;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-xl);
    background: var(--glass-strong);
    backdrop-filter: blur(28px) saturate(1.5);
    box-shadow:
      inset 0 0 0 1px var(--border-strong),
      var(--shadow-float);
    overflow: hidden;
  }
  .dialog:focus-visible {
    outline: none;
  }
  .dialog::before {
    content: '';
    position: absolute;
    inset: 0 0 auto 0;
    height: 120px;
    background: radial-gradient(80% 100% at 20% 0%, var(--cyan-tint), transparent 70%),
      radial-gradient(70% 100% at 90% 0%, var(--violet-tint), transparent 70%);
    pointer-events: none;
  }
</style>
