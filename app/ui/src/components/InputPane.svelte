<!--
  The Input pane: a CodeMirror editor that holds the text the user pasted, typed or opened.
  Findings glow in place and lock on with brackets; a scan line sweeps the pane on paste.
  Every effect is an overlay: the text is in the editor before any of them starts.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { EditorState } from '@codemirror/state';
  import { EditorView } from '@codemirror/view';
  import ClipboardPaste from '@lucide/svelte/icons/clipboard-paste';
  import FileText from '@lucide/svelte/icons/file-text';
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Button from './Button.svelte';
  import EmptyState from './EmptyState.svelte';
  import Panel from './Panel.svelte';
  import SelectionBar from './SelectionBar.svelte';
  import { appearance } from '../lib/appearance.svelte';
  import { workspace } from '../lib/workspace.svelte';
  import type { WarningKind } from '../lib/types';

  let { compact = false }: { compact?: boolean } = $props();

  let host: HTMLDivElement;
  let body: HTMLDivElement | undefined = $state();
  let view = $state.raw<EditorView | null>(null);

  onMount(() => {
    const v = new EditorView({
      parent: host,
      state: EditorState.create({ doc: '', extensions: workspace.inputExtensions() }),
    });
    view = v;
    workspace.attachInput(v);
    return () => {
      workspace.detachInput();
      v.destroy();
    };
  });

  const WARN: Partial<Record<WarningKind, [string, string]>> = {
    invisibleChars: ['invisible', 'Invisible characters (zero-width and similar) were stripped from the output.'],
    bidiControls: ['bidi', 'Bidirectional control characters can make text read differently than it runs.'],
    tagChars: ['tag chars', 'Unicode tag characters can hide text; they were stripped.'],
    decodeLimit: ['decode limit', 'Some encoded parts were too deep or too large to decode and check.'],
  };
  const warnings = $derived(
    (workspace.response?.warnings ?? []).flatMap((w) => {
      const meta = WARN[w.kind];
      return meta ? [{ kind: w.kind, count: w.count, text: meta[0], title: meta[1] }] : [];
    }),
  );
  const tone = $derived(
    workspace.groups.redacted > 0 && !workspace.stale ? 'red' : workspace.inputFocused ? 'cyan' : 'none',
  );
</script>

<Panel class="pane input-pane" aria-label="Input" tone={tone}>
  <header class="pane-head">
    <div class="pane-title">
      <FileText size={15} strokeWidth={1.75} aria-hidden="true" />
      <h2>Input</h2>
      {#if workspace.fileName}<span class="file mono" title={workspace.fileName}>{workspace.fileName}</span>{/if}
      <span class="meta mono num" data-testid="input-count">
        {workspace.length.toLocaleString('en')} chars{#if workspace.length}<span class="sep">·</span
          >{workspace.lines.toLocaleString('en')} {workspace.lines === 1 ? 'line' : 'lines'}{/if}
      </span>
      {#each warnings as w (w.kind)}
        <span class="warn" title={w.title}>
          <TriangleAlert size={11} strokeWidth={2.25} aria-hidden="true" />
          <span class="num">{w.count}</span>
          {w.text}
        </span>
      {/each}
    </div>
    <div class="tools">
      <Button variant="ghost" size="sm" icon={FolderOpen} label="Open a text file (Ctrl+O)" onclick={() => workspace.open()} />
      <Button variant="ghost" size="sm" icon={ClipboardPaste} label="Paste (Ctrl+V)" onclick={() => workspace.paste()} />
    </div>
  </header>
  <div class="pane-body" bind:this={body}>
    <div class="editor-host" bind:this={host} data-testid="input-editor"></div>
    {#if !workspace.hasText}
      <EmptyState {compact} late={workspace.shredding} onpaste={() => workspace.paste()} onopen={() => workspace.open()} />
    {/if}
    {#if workspace.pasteEpoch > 0 && !appearance.reducedMotion}
      {#key workspace.pasteEpoch}
        <div class="scanline" aria-hidden="true"><span class="beam"></span></div>
      {/key}
    {/if}
    {#if view && body}
      <SelectionBar {view} container={body} />
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
  h2 {
    font-size: var(--text-md);
    font-weight: 600;
    letter-spacing: -0.005em;
    color: var(--text);
  }
  .file {
    max-width: 160px;
    overflow: hidden;
    text-overflow: ellipsis;
    padding: 1px 7px;
    border-radius: 6px;
    font-size: var(--text-xs);
    color: var(--text-2);
    background: var(--glass);
    box-shadow: inset 0 0 0 1px var(--border);
  }
  .meta {
    margin-left: 2px;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .sep {
    margin: 0 5px;
    color: var(--faint);
  }
  .warn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 1px 7px 1px 6px;
    border-radius: 6px;
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--amber);
    background: var(--amber-tint);
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
  }
  .editor-host :global(.cm-editor) {
    position: absolute;
    inset: 0;
  }

  /* The scan line: a full-height layer whose bright bottom edge sweeps down once. */
  .scanline {
    position: absolute;
    inset: 0;
    z-index: 5;
    overflow: hidden;
    pointer-events: none;
    border-radius: 0 0 var(--radius-lg) var(--radius-lg);
  }
  .beam {
    position: absolute;
    inset: 0;
    background: linear-gradient(
      180deg,
      transparent 0%,
      transparent 72%,
      color-mix(in srgb, var(--cyan) 7%, transparent) 88%,
      color-mix(in srgb, var(--cyan) 22%, transparent) 98.6%,
      var(--cyan) 99.6%,
      transparent 100%
    );
    transform: translateY(-100%);
    animation: sweep 1050ms cubic-bezier(0.45, 0.05, 0.25, 1) forwards;
  }
  .beam::after {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    bottom: -1px;
    height: 14px;
    background: radial-gradient(60% 100% at 50% 0%, color-mix(in srgb, var(--cyan) 55%, transparent), transparent);
  }

  @keyframes sweep {
    0% {
      transform: translateY(-100%);
      opacity: 1;
    }
    85% {
      opacity: 1;
    }
    100% {
      transform: translateY(0%);
      opacity: 0;
    }
  }
</style>
