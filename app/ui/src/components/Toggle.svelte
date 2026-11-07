<!-- Switch with an inline label. role="switch", Space/Enter toggle (native button). -->
<script lang="ts">
  let {
    checked = $bindable(false),
    label,
    hint,
    quiet = false,
    onchange,
  }: {
    checked: boolean;
    label: string;
    /** Label for screen readers only (the row around the switch shows it). */
    quiet?: boolean;
    /** Tooltip explaining what the switch does. */
    hint?: string;
    onchange?: (checked: boolean) => void;
  } = $props();

  function toggle() {
    checked = !checked;
    onchange?.(checked);
  }
</script>

<button type="button" class="toggle" role="switch" aria-checked={checked} title={hint} onclick={toggle}>
  <span class="label" class:sr-only={quiet}>{label}</span>
  <span class="track" aria-hidden="true">
    <span class="fill"></span>
    <span class="knob"></span>
  </span>
</button>

<style>
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    height: 34px;
    padding: 0 10px 0 12px;
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--text-2);
    transition: background-color var(--dur-1) ease, color var(--dur-1) ease;
  }
  .toggle:hover {
    background: var(--glass-hover);
    color: var(--text);
  }

  .track {
    position: relative;
    width: 32px;
    height: 18px;
    border-radius: 9px;
    background: light-dark(rgb(11 16 32 / 0.12), rgb(255 255 255 / 0.1));
    box-shadow: inset 0 1px 2px light-dark(rgb(11 16 32 / 0.12), rgb(0 0 0 / 0.4));
    overflow: hidden;
    isolation: isolate;
  }
  .fill {
    position: absolute;
    inset: 0;
    background: var(--grad);
    opacity: 0;
    transition: opacity var(--dur-2) ease;
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: #fff;
    box-shadow:
      0 1px 2px rgb(0 0 0 / 0.3),
      0 0 0 0.5px rgb(0 0 0 / 0.08);
    transition: transform var(--dur-3) var(--ease-spring);
  }
  [aria-checked='true'] .fill {
    opacity: 1;
  }
  [aria-checked='true'] .knob {
    transform: translateX(14px);
  }
  [aria-checked='true'] .label {
    color: var(--text);
  }
</style>
