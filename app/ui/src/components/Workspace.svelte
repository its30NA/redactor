<!--
  The main layout: Input pane | scan seam | Safe output pane | findings rail.
  Below 1180 px the rail collapses to its counts; the user can open or close it either way.
-->
<script lang="ts">
  import { MediaQuery } from 'svelte/reactivity';
  import FindingsRail from './FindingsRail.svelte';
  import InputPane from './InputPane.svelte';
  import OutputPane from './OutputPane.svelte';
  import ScanSeam from './ScanSeam.svelte';
  import { workspace } from '../lib/workspace.svelte';
  import type { AppInfo } from '../lib/types';

  let { info }: { info: AppInfo | null } = $props();

  const narrow = new MediaQuery('(max-width: 1179px)', false);
  const short = new MediaQuery('(max-height: 700px)', false);
  let userCollapsed = $state<boolean | null>(null);
  const collapsed = $derived(userCollapsed ?? narrow.current);
</script>

<div class="workspace" class:collapsed>
  <InputPane compact={short.current} />
  <ScanSeam busy={workspace.busy} active={workspace.ready} />
  <OutputPane />
  <FindingsRail {info} {collapsed} oncollapse={(c) => (userCollapsed = c)} />
</div>

<style>
  .workspace {
    --seam-w: 22px;
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) var(--seam-w) minmax(0, 1fr) var(--rail-w);
    padding: 2px var(--gutter) 0;
  }
  .workspace.collapsed {
    grid-template-columns: minmax(0, 1fr) var(--seam-w) minmax(0, 1fr) var(--rail-w-collapsed);
  }
  .workspace :global(.rail) {
    margin-left: var(--gutter);
    animation: rise-in var(--dur-3) var(--ease-out);
  }
</style>
