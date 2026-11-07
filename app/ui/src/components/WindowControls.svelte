<!--
  Windows 11 style caption buttons for the frameless window: minimize, maximize/restore, close.
  Thin 1px glyphs drawn here (crisper than an icon font at this size).
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { windowControls } from '../lib/api';

  let maximized = $state(false);

  async function refresh() {
    try {
      maximized = await windowControls.isMaximized();
    } catch {
      /* outside the shell */
    }
  }

  onMount(() => {
    void refresh();
    let unlisten: (() => void) | undefined;
    windowControls
      .onResized(refresh)
      .then((u) => (unlisten = u))
      .catch(() => {});
    return () => unlisten?.();
  });

  async function toggle() {
    await windowControls.toggleMaximize();
    await refresh();
  }
</script>

<div class="controls">
  <button type="button" aria-label="Minimize" title="Minimize" onclick={() => windowControls.minimize()}>
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5.5h10" /></svg>
  </button>
  <button
    type="button"
    aria-label={maximized ? 'Restore' : 'Maximize'}
    title={maximized ? 'Restore' : 'Maximize'}
    onclick={toggle}
  >
    {#if maximized}
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <rect x="0.5" y="2.5" width="7" height="7" rx="1" />
        <path d="M2.5 2.5V1.5a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-1" />
      </svg>
    {:else}
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <rect x="0.5" y="0.5" width="9" height="9" rx="1.5" />
      </svg>
    {/if}
  </button>
  <button type="button" class="close" aria-label="Close" title="Close" onclick={() => windowControls.close()}>
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M0.5 0.5l9 9M9.5 0.5l-9 9" /></svg>
  </button>
</div>

<style>
  .controls {
    display: flex;
    height: 100%;
    align-self: stretch;
  }
  button {
    display: grid;
    place-items: center;
    width: 46px;
    height: 100%;
    color: var(--text-2);
    transition: background-color var(--dur-1) ease, color var(--dur-1) ease;
  }
  button:hover {
    background: var(--glass-hover);
    color: var(--text);
  }
  button:active {
    background: var(--glass-active);
  }
  .close:hover {
    background: #c42b1c;
    color: #fff;
  }
  .close:active {
    background: #b0281a;
  }
  button:focus-visible {
    box-shadow: inset 0 0 0 2px var(--focus);
  }
  svg {
    fill: none;
    stroke: currentColor;
    stroke-width: 1;
    shape-rendering: geometricPrecision;
  }
</style>
