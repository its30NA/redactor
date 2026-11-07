<!--
  Shown over the empty Input editor. Clicks pass through to the editor (type or paste there);
  the two shortcuts are buttons of their own.
-->
<script lang="ts">
  import Kbd from './Kbd.svelte';
  import ShieldEmblem from './ShieldEmblem.svelte';

  let {
    compact = false,
    late = false,
    onpaste,
    onopen,
  }: { compact?: boolean; late?: boolean; onpaste?: () => void; onopen?: () => void } = $props();
</script>

<div class="empty" class:compact class:late data-testid="empty-state">
  <ShieldEmblem size={compact ? 128 : 168} />
  <p class="lead">Paste anything.</p>
  <p class="sub">Secrets never leave this machine.</p>
  <p class="keys">
    <button type="button" onclick={onpaste}><Kbd keys={['Ctrl', 'V']} /> <span>paste</span></button>
    <span class="dot" aria-hidden="true"></span>
    <button type="button" onclick={onopen}><Kbd keys={['Ctrl', 'O']} /> <span>open a file</span></button>
  </p>
</div>

<style>
  .empty {
    position: absolute;
    inset: 0;
    z-index: 4;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 24px 24px 56px;
    pointer-events: none;
    animation: rise-in var(--dur-4) var(--ease-out) both;
  }
  .lead {
    margin-top: 10px;
    font-size: 22px;
    font-weight: 650;
    letter-spacing: -0.025em;
    color: var(--text);
  }
  .sub {
    margin-top: 4px;
    font-size: var(--text-md);
    color: var(--muted);
  }
  .keys {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 20px;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .keys button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    border-radius: var(--radius-sm);
    color: inherit;
    pointer-events: auto;
    transition:
      background-color var(--dur-1) ease,
      color var(--dur-1) ease;
  }
  .keys button:hover {
    background: var(--glass-hover);
    color: var(--text);
  }
  .dot {
    width: 3px;
    height: 3px;
    border-radius: 50%;
    background: var(--faint);
  }
  /* After a Wipe, arrive once the shred has fallen away. */
  .late {
    animation-delay: 620ms;
  }
  .compact {
    padding-bottom: 36px;
  }
  .compact .lead {
    font-size: var(--text-xl);
  }
</style>
