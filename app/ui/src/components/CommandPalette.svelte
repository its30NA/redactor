<!--
  Command palette (Ctrl+K): every action by name, filtered as you type. Arrows move, Enter
  runs, Esc closes. A combobox over a listbox, so a screen reader follows the active row.
-->
<script lang="ts" module>
  import type { LucideIcon } from '@lucide/svelte';

  export interface Command {
    id: string;
    label: string;
    icon: LucideIcon;
    keys?: string[];
    /** Extra words that should find it. */
    hint?: string;
    disabled?: boolean;
    run: () => void;
  }
</script>

<script lang="ts">
  import Search from '@lucide/svelte/icons/search';
  import Dialog from './Dialog.svelte';
  import Kbd from './Kbd.svelte';

  let { open = $bindable(false), commands }: { open: boolean; commands: Command[] } = $props();

  const uid = $props.id();
  let query = $state('');
  let active = $state(0);
  let list: HTMLUListElement | undefined = $state();

  const shown = $derived.by(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return commands;
    return commands.filter((c) => {
      const hay = `${c.label} ${c.hint ?? ''}`.toLowerCase();
      return words.every((w) => hay.includes(w));
    });
  });

  $effect(() => {
    if (open) {
      query = '';
      active = 0;
    }
  });

  $effect(() => {
    void shown;
    if (active >= shown.length) active = Math.max(0, shown.length - 1);
  });

  function move(delta: number) {
    if (!shown.length) return;
    active = (active + delta + shown.length) % shown.length;
    queueMicrotask(() => list?.querySelector(`#${uid}-o${active}`)?.scrollIntoView({ block: 'nearest' }));
  }

  function run(c: Command | undefined) {
    if (!c || c.disabled) return;
    open = false;
    // Let the dialog hand focus back before the command moves it (e.g. into a sheet).
    queueMicrotask(() => c.run());
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      move(1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      move(-1);
    } else if (e.key === 'Home' && !query) {
      e.preventDefault();
      active = 0;
    } else if (e.key === 'End' && !query) {
      e.preventDefault();
      active = Math.max(0, shown.length - 1);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      run(shown[active]);
    }
  }
</script>

<Dialog bind:open label="Command palette" width={540} placement="top" testid="palette">
  <div class="search">
    <Search size={16} strokeWidth={1.9} aria-hidden="true" />
    <input
      type="text"
      bind:value={query}
      {onkeydown}
      placeholder="Type a command…"
      role="combobox"
      aria-expanded="true"
      aria-controls="{uid}-list"
      aria-activedescendant={shown.length ? `${uid}-o${active}` : undefined}
      aria-autocomplete="list"
      spellcheck="false"
      autocomplete="off"
      data-autofocus
    />
    <Kbd keys={['Esc']} />
  </div>
  <ul class="list" id="{uid}-list" role="listbox" aria-label="Commands" bind:this={list}>
    {#each shown as c, i (c.id)}
      <!-- Keys go to the combobox input (aria-activedescendant); the options only take the mouse. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <li
        id="{uid}-o{i}"
        role="option"
        aria-selected={i === active}
        aria-disabled={c.disabled}
        class="opt"
        class:active={i === active}
        class:disabled={c.disabled}
        onmousemove={() => (active = i)}
        onmousedown={(e) => e.preventDefault()}
        onclick={() => run(c)}
      >
        <span class="icon" aria-hidden="true"><c.icon size={15} strokeWidth={1.9} /></span>
        <span class="label">{c.label}</span>
        {#if c.keys}<Kbd keys={c.keys} />{/if}
      </li>
    {:else}
      <li class="none" role="presentation">No command matches “{query}”.</li>
    {/each}
  </ul>
</Dialog>

<style>
  .search {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 52px;
    padding: 0 14px 0 16px;
    color: var(--muted);
    border-bottom: 1px solid var(--hairline);
  }
  input {
    border: 0;
    flex: 1;
    min-width: 0;
    height: 100%;
    font-size: var(--text-md);
    color: var(--text);
    background: transparent;
  }
  input:focus-visible {
    outline: none;
    box-shadow: none;
  }
  .list {
    position: relative;
    list-style: none;
    margin: 0;
    padding: 6px;
    max-height: min(420px, 60vh);
    overflow: auto;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 38px;
    padding: 0 10px;
    border-radius: var(--radius-sm);
    color: var(--text-2);
    cursor: default;
  }
  .opt.active {
    color: var(--text);
    background: var(--glass-hover);
    box-shadow: inset 2px 0 0 0 var(--cyan);
  }
  .opt.disabled {
    opacity: 0.45;
  }
  .icon {
    display: grid;
    place-items: center;
    width: 24px;
    color: var(--muted);
  }
  .opt.active .icon {
    color: var(--cyan);
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .none {
    padding: 14px 10px;
    font-size: var(--text-sm);
    color: var(--muted);
  }
</style>
