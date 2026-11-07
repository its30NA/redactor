<!-- Keyboard shortcut as keycaps, Windows style: ['Ctrl', 'Shift', 'C']. -->
<script lang="ts">
  let { keys, tone = 'default' }: { keys: string[]; tone?: 'default' | 'on-grad' } = $props();
  const glyph = (k: string) => (k === 'Enter' ? '↵' : k);
</script>

<span class="kbd {tone}" aria-label={keys.join('+')}>
  {#each keys as k (k)}
    <kbd class:wide={glyph(k).length > 1}>{glyph(k)}</kbd>
  {/each}
</span>

<style>
  .kbd {
    display: inline-flex;
    gap: 3px;
    align-items: center;
  }
  kbd {
    display: inline-grid;
    place-items: center;
    min-width: 18px;
    height: 18px;
    padding: 0 4px;
    border-radius: 5px;
    font-family: var(--font-mono);
    font-size: 10.5px;
    font-weight: 500;
    line-height: 1;
    color: var(--muted);
    background: var(--glass);
    box-shadow:
      inset 0 0 0 1px var(--border),
      inset 0 -1px 0 0 var(--border);
  }
  kbd.wide {
    padding: 0 5px;
  }
  .on-grad kbd {
    color: var(--on-grad);
    background: color-mix(in srgb, var(--on-grad) 10%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--on-grad) 22%, transparent);
  }
</style>
