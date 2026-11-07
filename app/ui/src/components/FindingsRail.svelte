<!--
  The findings rail: one item per distinct value, grouped by tier (Certain, Likely, Possible),
  kept values in their own section. Long lists render the first 50 of a tier and offer more.
  Collapsed (narrow windows or by choice) it shows the counts only.
-->
<script lang="ts">
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import PanelRightClose from '@lucide/svelte/icons/panel-right-close';
  import PanelRightOpen from '@lucide/svelte/icons/panel-right-open';
  import Radar from '@lucide/svelte/icons/radar';
  import Button from './Button.svelte';
  import FindingItem from './FindingItem.svelte';
  import Panel from './Panel.svelte';
  import { TIERS, type FindingGroup } from '../lib/findings';
  import { workspace } from '../lib/workspace.svelte';
  import type { AppInfo, Tier } from '../lib/types';

  let {
    info,
    collapsed,
    oncollapse,
  }: { info: AppInfo | null; collapsed: boolean; oncollapse: (collapsed: boolean) => void } = $props();

  const PAGE = 50;
  /** Only the first items of a tier play the arrival moment (a 5 MiB paste has thousands). */
  const MOMENT_ITEMS = 12;

  const META: Record<Tier, { name: string; desc: string }> = {
    certain: { name: 'Certain', desc: 'Known formats and verified shapes' },
    likely: { name: 'Likely', desc: 'Strong context, like password=…' },
    possible: { name: 'Possible', desc: 'Random-looking, worth a look' },
  };

  const g = $derived(workspace.groups);
  const active = $derived(Object.fromEntries(TIERS.map((t) => [t, g[t].filter((x) => !x.kept)])) as Record<Tier, FindingGroup[]>);
  const kept = $derived(TIERS.flatMap((t) => g[t].filter((x) => x.kept)));
  const counts = $derived(Object.fromEntries(TIERS.map((t) => [t, active[t].length])) as Record<Tier, number>);

  let limit = $state<Record<Tier | 'kept', number>>({ certain: PAGE, likely: PAGE, possible: PAGE, kept: PAGE });
  $effect(() => {
    void workspace.pasteEpoch;
    limit = { certain: PAGE, likely: PAGE, possible: PAGE, kept: PAGE };
  });

  const scanning = $derived(workspace.busy && !workspace.response);
  const clean = $derived(workspace.hasText && !!workspace.response && !workspace.stale && g.total === 0);

  const handlers = {
    onkeep: (x: FindingGroup) => (x.manual ? workspace.unredactManual(x) : workspace.keepValue(x.key)),
    onredact: (x: FindingGroup) => workspace.redactValue(x.key),
    onremember: (x: FindingGroup) => workspace.rememberFinding(x),
    onreveal: (x: FindingGroup) => workspace.reveal(x),
    onhover: (key: string | null) => workspace.hover(key),
  };
</script>

<Panel class="rail" aria-label="Findings">
  {#if collapsed}
    <div class="rail-mini">
      <Button variant="ghost" size="sm" icon={PanelRightOpen} label="Show findings" onclick={() => oncollapse(false)} />
      <div class="mini-tiers">
        {#each TIERS as t (t)}
          <span class="mini-tier" title="{META[t].name}: {counts[t]}">
            <span class="tier-dot {t}" aria-hidden="true"></span>
            <span class="mono num">{counts[t]}</span>
          </span>
        {/each}
      </div>
    </div>
  {:else}
    <header class="pane-head rail-head">
      <div class="pane-title">
        <Radar size={15} strokeWidth={1.75} aria-hidden="true" />
        <h2>Findings</h2>
        <span class="count mono num" data-testid="findings-count">{g.total}</span>
      </div>
      <Button variant="ghost" size="sm" icon={PanelRightClose} label="Hide findings" onclick={() => oncollapse(true)} />
    </header>

    <div class="summary" aria-label="Findings by tier">
      {#each TIERS as t (t)}
        <div class="sum {t}" class:zero={counts[t] === 0} title={META[t].desc}>
          <span class="sum-n mono num">{counts[t]}</span>
          <span class="sum-name"><span class="tier-dot {t}" aria-hidden="true"></span>{META[t].name}</span>
        </div>
      {/each}
    </div>

    <div class="rail-body">
      {#if !workspace.hasText}
        <ul class="legend">
          {#each TIERS as t (t)}
            <li class="legend-row">
              <span class="tier-dot {t}" aria-hidden="true"></span>
              <div class="legend-copy">
                <span class="legend-name">{META[t].name}</span>
                <span class="legend-desc">{META[t].desc}</span>
              </div>
            </li>
          {/each}
        </ul>
        <div class="rail-empty">
          <p>Nothing found yet.</p>
          <p class="muted">Each finding shows why it was flagged. Keep it, or redact more with a selection.</p>
        </div>
      {:else if scanning}
        <div class="rail-skeleton" aria-hidden="true">
          {#each [0, 1, 2] as i (i)}<span class="sk" style:--i={i}></span>{/each}
        </div>
      {:else if clean}
        <div class="rail-clean" data-testid="rail-clean">
          <span class="clean-icon"><CircleCheck size={18} strokeWidth={1.9} aria-hidden="true" /></span>
          <p>Nothing sensitive found.</p>
          <p class="muted">Select text in the Input to redact something by hand.</p>
        </div>
      {:else}
        {#each TIERS as t (t)}
          {#if active[t].length}
            <section class="tier" data-testid="tier-{t}" aria-label="{META[t].name} findings">
              <h3 class="tier-head">
                <span class="tier-dot {t}" aria-hidden="true"></span>
                {META[t].name}
                <span class="tier-n mono num">{active[t].length}</span>
              </h3>
              <ul class="items">
                {#each active[t].slice(0, limit[t]) as x, i (x.key)}
                  <FindingItem group={x} fresh={i < MOMENT_ITEMS && workspace.fresh.has(x.key)} index={i} {...handlers} />
                {/each}
              </ul>
              {#if active[t].length > limit[t]}
                <button type="button" class="more" onclick={() => (limit[t] += PAGE)}>
                  Show {Math.min(PAGE, active[t].length - limit[t]).toLocaleString('en')} more of {(
                    active[t].length - limit[t]
                  ).toLocaleString('en')}
                </button>
              {/if}
            </section>
          {/if}
        {/each}
        {#if kept.length}
          <section class="tier kept" data-testid="tier-kept" aria-label="Kept values">
            <h3 class="tier-head">
              <span class="tier-dot kept" aria-hidden="true"></span>
              Kept in the output
              <span class="tier-n mono num">{kept.length}</span>
            </h3>
            <ul class="items">
              {#each kept.slice(0, limit.kept) as x, i (x.key)}
                <FindingItem group={x} index={i} {...handlers} />
              {/each}
            </ul>
            {#if kept.length > limit.kept}
              <button type="button" class="more" onclick={() => (limit.kept += PAGE)}>Show more</button>
            {/if}
          </section>
        {/if}
      {/if}
    </div>

    <footer class="engine">
      <span class="engine-dot" aria-hidden="true"></span>
      <span class="engine-text">
        {#if info}Engine {info.engineVersion} · {info.ruleCount} rules{:else}Engine{/if}
      </span>
      <span class="engine-tag">on-device</span>
    </footer>
  {/if}
</Panel>

<style>
  .pane-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    height: 46px;
    flex-shrink: 0;
    padding: 0 6px 0 16px;
    border-bottom: 1px solid var(--hairline);
  }
  .pane-title {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    color: var(--muted);
  }
  h2 {
    font-size: var(--text-md);
    font-weight: 600;
    letter-spacing: -0.005em;
    color: var(--text);
  }
  .count {
    display: inline-grid;
    place-items: center;
    min-width: 20px;
    height: 18px;
    padding: 0 6px;
    border-radius: 9px;
    font-size: var(--text-xs);
    color: var(--text-2);
    background: var(--glass);
    box-shadow: inset 0 0 0 1px var(--border);
  }

  .summary {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 6px;
    padding: 10px 10px 0;
    flex-shrink: 0;
  }
  .sum {
    --c: var(--tier-certain);
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 10px 7px;
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--c) 7%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--c) 16%, transparent);
    transition:
      background-color var(--dur-2) ease,
      box-shadow var(--dur-2) ease;
  }
  .sum.likely {
    --c: var(--tier-likely);
  }
  .sum.possible {
    --c: var(--tier-possible);
  }
  .sum.zero {
    background: var(--glass);
    box-shadow: inset 0 0 0 1px var(--border);
  }
  .sum-n {
    font-size: 20px;
    font-weight: 600;
    line-height: 1.1;
    letter-spacing: -0.02em;
    color: var(--c);
  }
  .sum.zero .sum-n {
    color: var(--faint);
  }
  .sum-name {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--muted);
  }
  .sum-name .tier-dot {
    width: 6px;
    height: 6px;
  }

  .rail-body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    overscroll-behavior: contain;
    padding: 6px 8px 10px;
  }

  .tier-dot {
    --c: var(--tier-certain);
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
    background: var(--c);
    box-shadow: 0 0 10px 0 color-mix(in srgb, var(--c) 70%, transparent);
  }
  .tier-dot.likely {
    --c: var(--tier-likely);
  }
  .tier-dot.possible {
    --c: var(--tier-possible);
  }
  .tier-dot.kept {
    --c: var(--mint);
    box-shadow: none;
  }

  .tier {
    margin-top: 8px;
  }
  .tier-head {
    position: sticky;
    top: -6px;
    z-index: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px 6px;
    font-size: var(--text-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
    background: var(--panel-fill);
  }
  .tier-head .tier-dot {
    width: 6px;
    height: 6px;
  }
  .tier-n {
    margin-left: auto;
    font-weight: 500;
    letter-spacing: 0;
  }
  .items {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .more {
    width: 100%;
    margin-top: 4px;
    padding: 8px;
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--cyan);
  }
  .more:hover {
    background: var(--glass);
  }

  .legend {
    list-style: none;
    margin: 6px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .legend-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 9px 10px;
    border-radius: var(--radius-md);
  }
  .legend-copy {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .legend-name {
    font-weight: 500;
    color: var(--text);
  }
  .legend-desc {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .rail-empty {
    margin: 12px 10px 0;
    padding: 14px 0 0;
    border-top: 1px dashed var(--border);
    font-size: var(--text-sm);
    color: var(--text-2);
  }
  .muted {
    margin-top: 4px;
    color: var(--muted);
    line-height: 1.5;
  }
  .rail-clean {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    padding: 36px 16px;
    font-size: var(--text-sm);
    color: var(--text-2);
    animation: rise-in var(--dur-3) var(--ease-out) both;
  }
  .clean-icon {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    margin-bottom: 10px;
    border-radius: 11px;
    color: var(--mint);
    background: var(--mint-tint);
  }
  .rail-skeleton {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 4px;
  }
  .sk {
    height: 64px;
    border-radius: var(--radius-md);
    background: var(--glass);
    animation: sk-pulse 1.2s ease-in-out infinite;
    animation-delay: calc(var(--i) * 120ms);
  }
  @keyframes sk-pulse {
    50% {
      opacity: 0.45;
    }
  }

  .engine {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 44px;
    flex-shrink: 0;
    padding: 0 12px 0 16px;
    font-size: var(--text-sm);
    color: var(--muted);
    border-top: 1px solid var(--hairline);
  }
  .engine-dot {
    width: 6px;
    height: 6px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--mint);
    box-shadow: 0 0 6px 0 color-mix(in srgb, var(--mint) 70%, transparent);
  }
  .engine-text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .engine-tag {
    padding: 2px 7px;
    border-radius: 6px;
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--mint);
    background: var(--mint-tint);
  }

  .rail-mini {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    padding: 8px 0;
  }
  .mini-tiers {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding-top: 10px;
    border-top: 1px solid var(--hairline);
  }
  .mini-tier {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 5px;
    font-size: var(--text-xs);
    color: var(--muted);
  }
</style>
