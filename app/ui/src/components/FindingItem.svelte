<!--
  One value in the findings rail: what it is, where, why it was flagged and how sure the
  engine is, with Keep (or Redact again) and Remember. Clicking the item shows it in the text.
-->
<script lang="ts">
  import BookmarkCheck from '@lucide/svelte/icons/bookmark-check';
  import BookmarkPlus from '@lucide/svelte/icons/bookmark-plus';
  import Eye from '@lucide/svelte/icons/eye';
  import EyeOff from '@lucide/svelte/icons/eye-off';
  import Scramble from './Scramble.svelte';
  import { glyphFor } from '../lib/glyphs';
  import type { FindingGroup } from '../lib/findings';

  let {
    group,
    fresh = false,
    index = 0,
    onkeep,
    onredact,
    onremember,
    onreveal,
    onhover,
  }: {
    group: FindingGroup;
    fresh?: boolean;
    index?: number;
    onkeep: (g: FindingGroup) => void;
    onredact: (g: FindingGroup) => void;
    onremember: (g: FindingGroup) => void;
    onreveal: (g: FindingGroup) => void;
    onhover: (key: string | null) => void;
  } = $props();

  const Glyph = $derived(glyphFor(group.kind, group.source));
  const where = $derived.by(() => {
    const ls = group.lines;
    const head = ls.length > 3 ? `${ls.slice(0, 3).join(', ')}…` : ls.join(', ');
    return `${ls.length === 1 ? 'line' : 'lines'} ${head}`;
  });
  const pct = $derived(Math.round(group.confidence * 100));
  const remembered = $derived(group.source === 'watchlist');
  const always = $derived(group.source === 'custom');
  const SOURCE: Record<string, string> = {
    pattern: 'format',
    context: 'context',
    classifier: 'classifier',
    decoded: 'decoded',
    watchlist: 'remembered',
    propagated: 'repeat',
    manual: 'your selection',
    custom: 'always redact',
    pii: 'personal data',
  };
</script>

<li
  class="item t-{group.tier}"
  class:kept={group.kept}
  class:fresh
  style:--i={index}
  data-testid="finding"
  data-key={group.key}
  data-tier={group.tier}
  onmouseenter={() => onhover(group.key)}
  onmouseleave={() => onhover(null)}
>
  <button type="button" class="main" onclick={() => onreveal(group)} title="Show it in the text">
    <span class="glyph" aria-hidden="true"><Glyph size={14} strokeWidth={1.9} /></span>
    <span class="body">
      <span class="top">
        <span class="label"><Scramble text={group.label} play={fresh} delay={index * 40} /></span>
        {#if group.count > 1}<span class="times mono num" title="{group.count} occurrences">×{group.count}</span>{/if}
        <span class="where mono num">{where}</span>
      </span>
      <span class="reason">{group.reason}</span>
      <span class="conf">
        <span class="bar" aria-hidden="true"><span class="fill" style:transform="scaleX({group.confidence})"></span></span>
        <span class="pct mono num" aria-label="{pct} percent sure">{pct}%</span>
        <span class="src">{SOURCE[group.source] ?? group.source}</span>
        {#if group.kept}<span class="kept-tag">kept</span>{/if}
      </span>
    </span>
  </button>
  <div class="actions">
    {#if group.kept}
      <button type="button" class="act redact" onclick={() => onredact(group)} title="Redact this value again">
        <EyeOff size={13} strokeWidth={2} aria-hidden="true" /> Redact
      </button>
    {:else}
      <button
        type="button"
        class="act"
        onclick={() => onkeep(group)}
        title={group.manual ? 'Stop redacting your selection' : 'Not a secret: keep it in the output'}
      >
        <Eye size={13} strokeWidth={2} aria-hidden="true" /> Keep
      </button>
    {/if}
    {#if remembered}
      <span class="act done" title="Remembered for this session">
        <BookmarkCheck size={13} strokeWidth={2} aria-hidden="true" /> Remembered
      </span>
    {:else if !always}
      <button
        type="button"
        class="act"
        onclick={() => onremember(group)}
        title="Remember for this session: redacted wherever it shows up until you quit"
      >
        <BookmarkPlus size={13} strokeWidth={2} aria-hidden="true" /> Remember
      </button>
    {/if}
  </div>
</li>

<style>
  .item {
    --c: var(--tier-certain);
    position: relative;
    border-radius: var(--radius-md);
    background: transparent;
    transition: background-color var(--dur-1) ease;
  }
  .t-likely {
    --c: var(--tier-likely);
  }
  .t-possible {
    --c: var(--tier-possible);
  }
  .item::before {
    content: '';
    position: absolute;
    left: 0;
    top: 10px;
    bottom: 10px;
    width: 2px;
    border-radius: 1px;
    background: var(--c);
    opacity: 0.85;
    box-shadow: 0 0 8px 0 color-mix(in srgb, var(--c) 60%, transparent);
  }
  .item:hover,
  .item:focus-within {
    background: var(--glass);
  }
  .kept::before {
    background: var(--mint);
    box-shadow: none;
    opacity: 0.6;
  }
  .kept .label,
  .kept .reason {
    opacity: 0.7;
  }

  .fresh {
    animation: item-in 520ms var(--ease-out) both;
    animation-delay: calc(var(--i) * 40ms);
  }
  @keyframes item-in {
    from {
      opacity: 0;
      transform: translateX(10px);
    }
  }

  .main {
    display: flex;
    gap: 10px;
    width: 100%;
    padding: 10px 10px 4px 12px;
    text-align: left;
    color: inherit;
    border-radius: var(--radius-md);
  }
  .glyph {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    flex-shrink: 0;
    border-radius: 8px;
    color: var(--c);
    background: color-mix(in srgb, var(--c) 12%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--c) 22%, transparent);
  }
  .kept .glyph {
    color: var(--mint);
    background: var(--mint-tint);
    box-shadow: none;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 3px;
    flex: 1;
    min-width: 0;
  }
  .top {
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-width: 0;
  }
  .label {
    min-width: 0;
    flex-shrink: 1;
    overflow: hidden;
    font-weight: 600;
    font-size: var(--text-md);
    color: var(--text);
  }
  .times {
    flex-shrink: 0;
    padding: 0 5px;
    border-radius: 5px;
    font-size: var(--text-xs);
    color: var(--text-2);
    background: var(--glass-hover);
  }
  .where {
    margin-left: auto;
    flex-shrink: 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .reason {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    font-size: var(--text-sm);
    line-height: 1.45;
    color: var(--text-2);
  }
  .conf {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-top: 2px;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .bar {
    position: relative;
    width: 44px;
    height: 3px;
    border-radius: 2px;
    background: var(--glass-hover);
    overflow: hidden;
  }
  .fill {
    position: absolute;
    inset: 0;
    border-radius: 2px;
    background: var(--c);
    transform-origin: left center;
  }
  .kept .fill {
    background: var(--mint);
  }
  .src {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .kept-tag {
    padding: 0 6px;
    border-radius: 5px;
    font-weight: 600;
    color: var(--mint);
    background: var(--mint-tint);
  }

  .actions {
    display: flex;
    gap: 2px;
    padding: 0 8px 8px 46px;
  }
  .act {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 8px;
    border-radius: 7px;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--muted);
    transition:
      background-color var(--dur-1) ease,
      color var(--dur-1) ease;
  }
  button.act:hover {
    color: var(--text);
    background: var(--glass-hover);
  }
  .act.redact {
    color: var(--red);
  }
  .act.redact:hover {
    background: var(--red-tint);
    color: var(--red);
  }
  .act.done {
    color: var(--violet);
  }
</style>
