<!--
  The bottom dock: sensitivity, PII, and the actions. A floating glass bar; the only large
  surface with a real backdrop blur (small, so cheap).
-->
<script lang="ts">
  import ClipboardPaste from '@lucide/svelte/icons/clipboard-paste';
  import Copy from '@lucide/svelte/icons/copy';
  import Eraser from '@lucide/svelte/icons/eraser';
  import Button from './Button.svelte';
  import Segmented, { type SegmentOption } from './Segmented.svelte';
  import Toggle from './Toggle.svelte';
  import type { Sensitivity } from '../lib/types';

  let {
    sensitivity = $bindable(),
    pii = $bindable(),
    onsensitivity,
    onpii,
    onpaste,
    onwipe,
    oncopy,
  }: {
    sensitivity: Sensitivity;
    pii: boolean;
    onsensitivity?: (s: Sensitivity) => void;
    onpii?: (on: boolean) => void;
    onpaste?: () => void;
    onwipe?: () => void;
    oncopy?: () => void;
  } = $props();

  const levels: SegmentOption<Sensitivity>[] = [
    { value: 'relaxed', label: 'Relaxed', hint: 'Only clear-cut secrets  (Ctrl+1)', tone: 'mint' },
    { value: 'balanced', label: 'Balanced', hint: 'Recommended  (Ctrl+2)', tone: 'cyan' },
    { value: 'paranoid', label: 'Paranoid', hint: 'Also anything that looks random  (Ctrl+3)', tone: 'violet' },
  ];
</script>

<div class="dock" role="toolbar" aria-label="Scrub controls">
  <div class="group">
    <Segmented options={levels} bind:value={sensitivity} label="Sensitivity" onchange={onsensitivity} />
    <Toggle
      bind:checked={pii}
      label="PII"
      hint="Also redact personal data: e-mail, phone, IBAN, card numbers, IP addresses"
      onchange={onpii}
    />
  </div>

  <div class="group actions">
    <Button
      variant="ghost"
      icon={ClipboardPaste}
      keys={['Ctrl', 'V']}
      onclick={onpaste}
      class="paste"
      title="Paste the clipboard and scrub it (Ctrl+V)"
    >
      Paste &amp; scrub
    </Button>
    <Button
      variant="danger"
      icon={Eraser}
      keys={['Ctrl', 'Shift', 'X']}
      onclick={onwipe}
      class="wipe"
      title="Clear both panes (Ctrl+Shift+X)"
    >
      Wipe
    </Button>
    <span class="divider" aria-hidden="true"></span>
    <Button
      variant="primary"
      size="lg"
      icon={Copy}
      keys={['Ctrl', 'Shift', 'C']}
      onclick={oncopy}
      title="Copy the safe text (Ctrl+Shift+C)"
      data-testid="copy-safe"
    >
      Copy safe text
    </Button>
  </div>
</div>

<style>
  .dock {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    height: 62px;
    padding: 0 10px 0 12px;
    border-radius: 18px;
    background: var(--glass-strong);
    backdrop-filter: blur(20px) saturate(1.5);
    box-shadow:
      inset 0 0 0 1px var(--border),
      var(--shadow-float);
  }
  .group {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .actions {
    gap: 4px;
  }
  .divider {
    width: 1px;
    height: 22px;
    margin: 0 8px 0 6px;
    background: var(--border);
  }

  /* Narrow windows: shortcuts move into tooltips on the secondary actions. */
  @media (max-width: 1120px) {
    .dock :global(.paste .kbd),
    .dock :global(.wipe .kbd) {
      display: none;
    }
  }
</style>
