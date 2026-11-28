<!--
  Custom title bar for the frameless window. The bar and its non-interactive parts carry
  data-tauri-drag-region (drag to move, double-click to maximize); buttons do not.
  On macOS the window keeps the system's own buttons (the traffic lights) over the left end of
  this bar: no caption buttons here, and 78 px of free space on the left for them.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import Mark from './Mark.svelte';
  import WindowControls from './WindowControls.svelte';
  import type { KeyOs } from '../lib/keys';

  let { status, actions, os = 'windows' }: { status: Snippet; actions?: Snippet; os?: KeyOs } = $props();
  const mac = $derived(os === 'macos');
</script>

<header class="titlebar" class:mac data-tauri-drag-region>
  <div class="brand" data-tauri-drag-region>
    <Mark size={20} />
    <span class="name" data-tauri-drag-region>Redactor</span>
  </div>

  <div class="center" data-tauri-drag-region>
    {@render status()}
  </div>

  <div class="right">
    {#if actions}
      <div class="actions">{@render actions()}</div>
      {#if !mac}<span class="divider" aria-hidden="true"></span>{/if}
    {/if}
    {#if !mac}<WindowControls />{/if}
  </div>
</header>

<style>
  .titlebar {
    position: relative;
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    height: var(--titlebar-h);
    padding-left: 16px;
  }
  /* The traffic lights sit in the first 78 px; the right end keeps the bar's own margin. */
  .titlebar.mac {
    padding-left: 78px;
    padding-right: 10px;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    min-width: 0;
  }
  .name {
    font-size: var(--text-md);
    font-weight: 600;
    letter-spacing: -0.01em;
    color: var(--text);
  }
  .center {
    display: flex;
    justify-content: center;
  }
  .right {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    height: 100%;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 2px;
    padding-right: 6px;
  }
  .divider {
    width: 1px;
    height: 16px;
    margin-right: 4px;
    background: var(--border);
  }
</style>
