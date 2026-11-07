<!--
  First-run card: the offline promise, the global hotkey and the screen-capture shield.
  Shown once; "Get started" sets `firstRunDone`.
-->
<script lang="ts">
  import CloudOff from '@lucide/svelte/icons/cloud-off';
  import Keyboard from '@lucide/svelte/icons/keyboard';
  import ShieldCheck from '@lucide/svelte/icons/shield-check';
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';
  import Kbd from './Kbd.svelte';
  import Mark from './Mark.svelte';
  import { settings } from '../lib/settings.svelte';

  let { open = $bindable(false) }: { open: boolean } = $props();

  const seen = () => void settings.update({ firstRunDone: true });

  function done() {
    open = false;
    seen();
  }
</script>

<Dialog bind:open label="Welcome to Redactor" width={460} testid="first-run" onclose={seen}>
  <div class="card">
    <div class="brand">
      <span class="mark"><Mark size={56} /></span>
      <h2>Welcome to Redactor</h2>
      <p class="tag">Paste anything. Copy it back without the secrets.</p>
    </div>

    <ul class="promises">
      <li>
        <span class="icon mint" aria-hidden="true"><CloudOff size={16} strokeWidth={1.9} /></span>
        <div>
          <p class="title">Offline by design</p>
          <p class="desc">No network code. What you paste stays on this PC and is never written to disk.</p>
        </div>
      </li>
      <li>
        <span class="icon cyan" aria-hidden="true"><Keyboard size={16} strokeWidth={1.9} /></span>
        <div>
          <p class="title">Scrub from anywhere <Kbd keys={settings.value.hotkey.split('+')} /></p>
          <p class="desc">Cleans the clipboard in place, without opening this window.</p>
        </div>
      </li>
      <li>
        <span class="icon violet" aria-hidden="true"><ShieldCheck size={16} strokeWidth={1.9} /></span>
        <div>
          <p class="title">Shielded from screen capture</p>
          <p class="desc">Screenshots and screen sharing see a blank window. You can turn it off in Settings.</p>
        </div>
      </li>
    </ul>

    <Button variant="primary" size="lg" onclick={done} data-autofocus class="start">Get started</Button>
  </div>
</Dialog>

<style>
  .card {
    position: relative;
    display: flex;
    flex-direction: column;
    padding: 28px 26px 24px;
  }
  .brand {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
  }
  .mark {
    display: grid;
    place-items: center;
    width: 84px;
    height: 84px;
    border-radius: 24px;
    background: var(--glass);
    box-shadow:
      inset 0 0 0 1px var(--border-strong),
      inset 0 1px 0 0 var(--highlight),
      0 10px 40px -12px var(--grad-glow);
  }
  h2 {
    margin-top: 16px;
    font-size: 22px;
    font-weight: 650;
    letter-spacing: -0.02em;
  }
  .tag {
    margin-top: 4px;
    color: var(--muted);
  }
  .promises {
    list-style: none;
    margin: 22px 0 22px;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .promises li {
    display: flex;
    gap: 12px;
  }
  .icon {
    --c: var(--mint);
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    flex-shrink: 0;
    border-radius: 10px;
    color: var(--c);
    background: color-mix(in srgb, var(--c) 13%, transparent);
  }
  .icon.cyan {
    --c: var(--cyan);
  }
  .icon.violet {
    --c: var(--violet);
  }
  .title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
  }
  .desc {
    margin-top: 2px;
    font-size: var(--text-sm);
    line-height: 1.5;
    color: var(--muted);
  }
  .card :global(.start) {
    width: 100%;
  }
</style>
