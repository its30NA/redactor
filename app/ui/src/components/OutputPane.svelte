<!--
  The Safe output pane: the redacted, re-verified text in a read-only CodeMirror editor, each
  placeholder a chip. Selecting and copying gives the exact output text.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { EditorState } from '@codemirror/state';
  import { EditorView } from '@codemirror/view';
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import Copy from '@lucide/svelte/icons/copy';
  import Lock from '@lucide/svelte/icons/lock';
  import Save from '@lucide/svelte/icons/save';
  import ShieldCheck from '@lucide/svelte/icons/shield-check';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Button from './Button.svelte';
  import Panel from './Panel.svelte';
  import { MAX_INPUT_BYTES } from '../lib/scan-request';
  import { workspace } from '../lib/workspace.svelte';

  let host: HTMLDivElement;

  onMount(() => {
    const v = new EditorView({
      parent: host,
      state: EditorState.create({ doc: '', extensions: workspace.outputExtensions() }),
    });
    workspace.attachOutput(v);
    return () => {
      workspace.detachOutput();
      v.destroy();
    };
  });

  const res = $derived(workspace.response);
  const hasOutput = $derived(!!res && res.output.length > 0);
  const status = $derived.by(() => {
    if (!workspace.hasText || workspace.tooLarge) return null;
    if (workspace.busy || workspace.stale || !res) return 'scanning';
    return res.verified ? 'verified' : 'unverified';
  });
  const mb = (n: number) => (n / 1024 / 1024).toLocaleString('en', { maximumFractionDigits: 1 });
  const ms = $derived(res ? (res.stats.ms < 10 ? res.stats.ms.toFixed(1) : Math.round(res.stats.ms).toLocaleString('en')) : '');
</script>

<Panel class="pane output-pane" aria-label="Safe output" tone={status === 'verified' && hasOutput ? 'mint' : 'none'}>
  <header class="pane-head">
    <div class="pane-title">
      <ShieldCheck size={15} strokeWidth={1.75} aria-hidden="true" class="safe-icon" />
      <h2>Safe output</h2>
      {#if status === 'verified'}
        <span class="badge ok" data-testid="verified" title="Scanned again after redaction: nothing detectable is left">
          <CircleCheck size={12} strokeWidth={2.25} aria-hidden="true" /> Verified
        </span>
      {:else if status === 'scanning'}
        <span class="badge busy"><span class="spin" aria-hidden="true"></span> Scanning</span>
      {:else if status === 'unverified'}
        <span class="badge warn" title="The verify pass hit its limit. Read the output before you paste it.">
          <TriangleAlert size={12} strokeWidth={2.25} aria-hidden="true" /> Check before pasting
        </span>
      {/if}
      {#if res && status !== 'scanning'}
        <span class="meta mono num">
          {res.outputSpans.length.toLocaleString('en')}
          {res.outputSpans.length === 1 ? 'redaction' : 'redactions'}<span class="sep">·</span>{ms} ms
        </span>
      {/if}
    </div>
    <div class="tools">
      <Button
        variant="ghost"
        size="sm"
        icon={Save}
        label="Save a safe copy (Ctrl+S)"
        disabled={!workspace.ready}
        onclick={() => workspace.save()}
      />
      <Button
        variant="ghost"
        size="sm"
        icon={Copy}
        label="Copy safe text (Ctrl+Shift+C)"
        disabled={!workspace.ready}
        onclick={() => workspace.copySafe()}
      />
    </div>
  </header>
  <div class="pane-body">
    <div class="editor-host" class:dim={workspace.stale} bind:this={host} data-testid="output-editor"></div>

    {#if workspace.tooLarge}
      <div class="notice" role="alert" data-testid="too-large">
        <span class="notice-icon"><TriangleAlert size={20} strokeWidth={1.75} aria-hidden="true" /></span>
        <p class="notice-title">Too large to scan</p>
        <p class="notice-text">
          This text is {workspace.tooLarge.bytes
            ? `${mb(workspace.tooLarge.bytes)} MB`
            : `over ${mb(workspace.tooLarge.chars)} MB`}. Redactor scans up to {mb(MAX_INPUT_BYTES)} MB at
          a time, so nothing was redacted and there is nothing to copy.
        </p>
        <p class="notice-text muted">Split it into smaller parts, or open the part you need.</p>
      </div>
    {:else if !workspace.hasText}
      <div class="placeholder" class:late={workspace.shredding}>
        <span class="lock" aria-hidden="true"><Lock size={18} strokeWidth={1.75} /></span>
        <p class="strong">Safe text appears here</p>
        <p>Scanned again and verified before you copy it.</p>
      </div>
    {:else if !hasOutput && workspace.busy}
      <div class="skeleton" aria-hidden="true">
        {#each [78, 92, 54, 86, 66, 40, 72] as w, i (i)}
          <span class="bar" style:width="{w}%"></span>
        {/each}
      </div>
    {/if}
  </div>
</Panel>

<style>
  .pane-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    height: 46px;
    flex-shrink: 0;
    padding: 0 8px 0 16px;
    border-bottom: 1px solid var(--hairline);
  }
  .pane-title {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
  }
  .pane-title :global(.safe-icon) {
    color: var(--mint);
  }
  h2 {
    font-size: var(--text-md);
    font-weight: 600;
    letter-spacing: -0.005em;
    color: var(--text);
  }
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 20px;
    padding: 0 8px 0 6px;
    border-radius: 10px;
    font-size: var(--text-xs);
    font-weight: 600;
    animation: rise-in var(--dur-3) var(--ease-out) both;
  }
  .badge.ok {
    color: var(--mint);
    background: var(--mint-tint);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--mint) 25%, transparent);
  }
  .badge.busy {
    color: var(--cyan);
    background: var(--cyan-tint);
  }
  .badge.warn {
    color: var(--amber);
    background: var(--amber-tint);
  }
  .spin {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    border: 1.5px solid color-mix(in srgb, var(--cyan) 30%, transparent);
    border-top-color: var(--cyan);
    animation: spin 700ms linear infinite;
  }
  .meta {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .sep {
    margin: 0 5px;
    color: var(--faint);
  }
  .tools {
    display: flex;
    gap: 2px;
    flex-shrink: 0;
  }
  .pane-body {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .editor-host {
    position: relative;
    flex: 1;
    min-height: 0;
    transition: opacity var(--dur-2) ease;
  }
  .editor-host.dim {
    opacity: 0.55;
  }
  .editor-host :global(.cm-editor) {
    position: absolute;
    inset: 0;
  }

  .placeholder,
  .notice {
    position: absolute;
    inset: 0;
    z-index: 3;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 24px 32px 56px;
    color: var(--muted);
    pointer-events: none;
  }
  .placeholder {
    animation: rise-in var(--dur-4) var(--ease-out) both;
  }
  .placeholder.late {
    animation-delay: 620ms;
  }
  .placeholder .strong {
    color: var(--text-2);
    font-weight: 500;
  }
  .lock {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    margin-bottom: 14px;
    border-radius: 12px;
    color: var(--muted);
    background: var(--glass);
    box-shadow:
      inset 0 0 0 1px var(--border),
      inset 0 1px 0 0 var(--highlight);
  }
  .notice {
    pointer-events: auto;
    background: var(--panel-fill);
  }
  .notice-icon {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    margin-bottom: 14px;
    border-radius: 13px;
    color: var(--amber);
    background: var(--amber-tint);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--amber) 30%, transparent);
  }
  .notice-title {
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--text);
  }
  .notice-text {
    max-width: 360px;
    margin-top: 6px;
    line-height: 1.55;
    color: var(--text-2);
  }
  .notice-text.muted {
    color: var(--muted);
  }

  .skeleton {
    position: absolute;
    inset: 14px 18px auto 58px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    pointer-events: none;
  }
  .bar {
    position: relative;
    height: 10px;
    border-radius: 5px;
    background: var(--glass-hover);
    overflow: hidden;
  }
  .bar::after {
    content: '';
    position: absolute;
    inset: 0;
    background: linear-gradient(90deg, transparent, color-mix(in srgb, var(--cyan) 22%, transparent), transparent);
    transform: translateX(-100%);
    animation: skeleton-sheen 1.1s ease-in-out infinite;
  }
  @keyframes skeleton-sheen {
    to {
      transform: translateX(100%);
    }
  }
</style>
