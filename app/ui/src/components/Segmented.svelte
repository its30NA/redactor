<!--
  Segmented control (a radio group). Equal-width segments, so the sliding thumb is a pure
  transform. Each option may carry a tone that tints the thumb's glow line.
  Keyboard: arrows / Home / End move and select, as in a native radio group.
-->
<script lang="ts" module>
  export interface SegmentOption<V extends string> {
    value: V;
    label: string;
    /** Tooltip, e.g. the shortcut. */
    hint?: string;
    tone?: 'mint' | 'cyan' | 'violet' | 'red' | 'neutral';
  }
</script>

<script lang="ts" generics="T extends string">
  let {
    options,
    value = $bindable(),
    label,
    size = 'md',
    onchange,
  }: {
    options: SegmentOption<T>[];
    value: T;
    label: string;
    size?: 'sm' | 'md';
    onchange?: (value: T) => void;
  } = $props();

  let buttons: HTMLButtonElement[] = $state([]);
  const index = $derived(Math.max(0, options.findIndex((o) => o.value === value)));
  const tone = $derived(options[index]?.tone ?? 'neutral');

  function select(i: number, focus = false) {
    const opt = options[i];
    if (!opt) return;
    if (opt.value !== value) {
      value = opt.value;
      onchange?.(opt.value);
    }
    if (focus) buttons[i]?.focus();
  }

  function onkeydown(e: KeyboardEvent) {
    const n = options.length;
    const moves: Record<string, number> = {
      ArrowRight: index + 1,
      ArrowDown: index + 1,
      ArrowLeft: index - 1,
      ArrowUp: index - 1,
      Home: 0,
      End: n - 1,
    };
    const target = moves[e.key];
    if (target === undefined) return;
    e.preventDefault();
    select((target + n) % n, true);
  }
</script>

<div
  class="seg {size}"
  role="radiogroup"
  aria-label={label}
  data-tone={tone}
  style:--n={options.length}
  style:--i={index}
>
  <span class="thumb" aria-hidden="true"></span>
  {#each options as opt, i (opt.value)}
    <button
      bind:this={buttons[i]}
      type="button"
      role="radio"
      aria-checked={i === index}
      tabindex={i === index ? 0 : -1}
      title={opt.hint}
      class:active={i === index}
      onclick={() => select(i)}
      {onkeydown}
    >
      {opt.label}
    </button>
  {/each}
</div>

<style>
  .seg {
    --pad: 3px;
    --tone-color: var(--muted);
    position: relative;
    display: grid;
    grid-template-columns: repeat(var(--n), minmax(0, 1fr));
    padding: var(--pad);
    border-radius: var(--radius-md);
    background: light-dark(rgb(11 16 32 / 0.045), rgb(0 0 0 / 0.22));
    box-shadow:
      inset 0 0 0 1px var(--border),
      inset 0 1px 2px light-dark(rgb(11 16 32 / 0.06), rgb(0 0 0 / 0.35));
    isolation: isolate;
  }
  .seg[data-tone='mint'] {
    --tone-color: var(--mint);
  }
  .seg[data-tone='cyan'] {
    --tone-color: var(--cyan);
  }
  .seg[data-tone='violet'] {
    --tone-color: var(--violet-soft);
  }
  .seg[data-tone='red'] {
    --tone-color: var(--red);
  }

  .thumb {
    position: absolute;
    z-index: -1;
    top: var(--pad);
    bottom: var(--pad);
    left: var(--pad);
    width: calc((100% - 2 * var(--pad)) / var(--n));
    border-radius: calc(var(--radius-md) - 3px);
    background: light-dark(#ffffff, rgb(255 255 255 / 0.09));
    box-shadow:
      inset 0 1px 0 0 light-dark(rgb(255 255 255 / 1), rgb(255 255 255 / 0.12)),
      0 0 0 1px light-dark(rgb(11 16 32 / 0.08), rgb(255 255 255 / 0.04)),
      0 1px 3px light-dark(rgb(11 16 32 / 0.1), rgb(0 0 0 / 0.4));
    transform: translateX(calc(var(--i) * 100%));
    transition: transform var(--dur-3) var(--ease-spring);
  }
  /* The tone: a short glowing line on the thumb's bottom edge. */
  .thumb::after {
    content: '';
    position: absolute;
    left: 30%;
    right: 30%;
    bottom: 0;
    height: 2px;
    border-radius: 2px;
    background: var(--tone-color);
    box-shadow: 0 0 8px 0 var(--tone-color);
    opacity: 0.9;
    transition: background-color var(--dur-3) ease, box-shadow var(--dur-3) ease;
  }

  button {
    position: relative;
    height: 28px;
    padding: 0 12px;
    border-radius: calc(var(--radius-md) - 3px);
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--muted);
    transition: color var(--dur-2) ease;
  }
  .sm button {
    height: 24px;
    padding: 0 10px;
  }
  button:hover {
    color: var(--text-2);
  }
  button.active {
    color: var(--text);
  }
</style>
