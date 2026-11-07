<!--
  Button. Variants: primary (gradient, the one hero action per screen), secondary (glass),
  ghost (quiet), danger (quiet until hovered). `icon` takes a Lucide component; `keys` shows a
  shortcut as keycaps. Icon-only buttons need `label` (used as aria-label and tooltip).
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import type { LucideIcon } from '@lucide/svelte';
  import Kbd from './Kbd.svelte';

  type Props = Omit<HTMLButtonAttributes, 'children'> & {
    variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
    size?: 'sm' | 'md' | 'lg';
    icon?: LucideIcon;
    keys?: string[];
    /** Accessible name for icon-only buttons; also the tooltip. */
    label?: string;
    pressed?: boolean;
    children?: Snippet;
  };

  let {
    variant = 'secondary',
    size = 'md',
    icon: Icon,
    keys,
    label,
    pressed,
    children,
    type = 'button',
    class: klass = '',
    ...rest
  }: Props = $props();

  const iconSize = $derived(size === 'lg' ? 18 : size === 'sm' ? 14 : 16);
</script>

<button
  {type}
  class="btn {variant} {size} {klass}"
  class:icon-only={!children}
  aria-label={children ? undefined : label}
  aria-pressed={pressed}
  title={label}
  {...rest}
>
  {#if variant === 'primary'}<span class="sheen" aria-hidden="true"></span>{/if}
  {#if Icon}<Icon size={iconSize} strokeWidth={size === 'lg' ? 2 : 1.75} aria-hidden="true" />{/if}
  {#if children}<span class="text">{@render children()}</span>{/if}
  {#if keys}<Kbd {keys} tone={variant === 'primary' ? 'on-grad' : 'default'} />{/if}
</button>

<style>
  .btn {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    flex-shrink: 0;
    white-space: nowrap;
    font-weight: 500;
    border-radius: var(--radius-md);
    color: var(--text-2);
    isolation: isolate;
    overflow: hidden;
    transition:
      background-color var(--dur-1) ease,
      color var(--dur-1) ease,
      box-shadow var(--dur-2) ease,
      transform var(--dur-2) var(--ease-spring),
      filter var(--dur-2) ease;
  }
  .btn:active:not(:disabled) {
    transform: scale(0.97);
  }
  .btn:disabled {
    opacity: 0.45;
    pointer-events: none;
  }

  .sm {
    height: 28px;
    padding: 0 10px;
    font-size: var(--text-sm);
    border-radius: var(--radius-sm);
    gap: 6px;
  }
  .md {
    height: 34px;
    padding: 0 12px;
    font-size: var(--text-md);
  }
  .lg {
    height: 42px;
    padding: 0 16px 0 18px;
    font-size: 14px;
    font-weight: 600;
    border-radius: 12px;
    gap: 10px;
  }
  .icon-only.sm {
    width: 28px;
    padding: 0;
  }
  .icon-only.md {
    width: 32px;
    height: 32px;
    padding: 0;
  }
  .icon-only.lg {
    width: 42px;
    padding: 0;
  }

  /* Secondary: glass */
  .secondary {
    background: var(--glass);
    box-shadow:
      inset 0 0 0 1px var(--border),
      inset 0 1px 0 0 var(--highlight);
    color: var(--text);
  }
  .secondary:hover {
    background: var(--glass-hover);
    box-shadow:
      inset 0 0 0 1px var(--border-strong),
      inset 0 1px 0 0 var(--highlight);
  }

  /* Ghost */
  .ghost {
    color: var(--text-2);
  }
  .ghost:hover,
  .ghost[aria-pressed='true'] {
    background: var(--glass-hover);
    color: var(--text);
  }

  /* Danger: quiet until it matters */
  .danger {
    color: var(--text-2);
  }
  .danger:hover {
    background: var(--red-tint);
    color: var(--red);
  }

  /* Primary: the gradient hero */
  .primary {
    color: var(--on-grad);
    background: var(--grad);
    box-shadow:
      inset 0 1px 0 0 rgb(255 255 255 / 0.35),
      inset 0 0 0 1px rgb(255 255 255 / 0.12),
      0 10px 28px -10px var(--grad-glow),
      0 0 0 1px light-dark(rgb(79 70 229 / 0.25), rgb(0 0 0 / 0.25));
  }
  .primary::before {
    content: '';
    position: absolute;
    inset: 0;
    z-index: -1;
    background: radial-gradient(120% 140% at 50% -40%, rgb(255 255 255 / 0.32), transparent 60%);
    opacity: 0.7;
    transition: opacity var(--dur-2) ease;
  }
  .primary:hover {
    filter: brightness(1.06) saturate(1.05);
    box-shadow:
      inset 0 1px 0 0 rgb(255 255 255 / 0.4),
      inset 0 0 0 1px rgb(255 255 255 / 0.16),
      0 14px 36px -10px var(--grad-glow),
      0 0 0 1px light-dark(rgb(79 70 229 / 0.3), rgb(0 0 0 / 0.25));
  }
  .primary:hover::before {
    opacity: 1;
  }
  .sheen {
    position: absolute;
    inset: -2px auto -2px 0;
    width: 38%;
    z-index: -1;
    background: linear-gradient(90deg, transparent, rgb(255 255 255 / 0.38), transparent);
    transform: translateX(-120%) skewX(-18deg);
    pointer-events: none;
  }
  .primary:hover .sheen {
    animation: sheen 900ms var(--ease-out) 1;
  }
  .text {
    position: relative;
  }
  .lg :global(.lucide-icon) {
    margin-left: -2px;
  }
</style>
