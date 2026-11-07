<!--
  Add a secret for this session: a label and a masked field. The value goes to the shell once
  and the field is cleared right after; the UI keeps only the label.
-->
<script lang="ts">
  import Eye from '@lucide/svelte/icons/eye';
  import EyeOff from '@lucide/svelte/icons/eye-off';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';
  import { session } from '../lib/session.svelte';
  import { toasts } from '../lib/toast.svelte';
  import { MIN_SESSION_SECRET } from '../lib/types';

  let { open = $bindable(false), onadded }: { open: boolean; onadded?: () => void } = $props();

  let label = $state('');
  let secret = $state('');
  let show = $state(false);
  let busy = $state(false);
  let touched = $state(false);

  const tooShort = $derived(secret.length > 0 && secret.length < MIN_SESSION_SECRET);
  const multiline = $derived(/[\r\n]/.test(secret));
  const valid = $derived(secret.length >= MIN_SESSION_SECRET && !multiline);

  function reset() {
    label = '';
    secret = '';
    show = false;
    touched = false;
  }

  $effect(() => {
    if (!open) reset();
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    touched = true;
    if (!valid || busy) return;
    busy = true;
    const name = label.trim() || 'Secret';
    const ok = await session.add(secret, name);
    busy = false;
    if (!ok) return;
    secret = '';
    open = false;
    toasts.show({ tone: 'success', title: `Remembered “${name}”`, detail: 'For this session only. Never written to disk.' });
    onadded?.();
  }
</script>

<Dialog bind:open label="Add a secret" width={420} testid="add-secret">
  <form class="form" onsubmit={submit} autocomplete="off">
    <div class="head">
      <span class="icon" aria-hidden="true"><KeyRound size={18} strokeWidth={1.8} /></span>
      <div>
        <h2>Add a secret</h2>
        <p>Redacted wherever it shows up until you quit, lock or wipe. Kept in memory only.</p>
      </div>
    </div>

    <label class="field">
      <span class="field-label">Label</span>
      <input
        type="text"
        bind:value={label}
        placeholder="e.g. Staging database"
        maxlength="60"
        spellcheck="false"
        data-autofocus
      />
    </label>

    <label class="field">
      <span class="field-label">Secret</span>
      <span class="secret">
        <input
          type={show ? 'text' : 'password'}
          bind:value={secret}
          class="mono"
          autocomplete="off"
          spellcheck="false"
          autocapitalize="off"
          data-gramm="false"
          data-testid="secret-input"
          aria-invalid={touched && !valid}
          aria-describedby="secret-hint"
        />
        <button
          type="button"
          class="reveal"
          onclick={() => (show = !show)}
          aria-label={show ? 'Hide the secret' : 'Show the secret'}
          title={show ? 'Hide' : 'Show'}
          aria-pressed={show}
        >
          {#if show}<EyeOff size={15} strokeWidth={1.9} />{:else}<Eye size={15} strokeWidth={1.9} />{/if}
        </button>
      </span>
      <span id="secret-hint" class="hint" class:bad={(touched || tooShort) && !valid && secret.length > 0}>
        {#if multiline}
          One line only.
        {:else if tooShort}
          At least {MIN_SESSION_SECRET} characters ({secret.length} so far).
        {:else}
          At least {MIN_SESSION_SECRET} characters. Only its label is shown afterwards.
        {/if}
      </span>
    </label>

    <div class="actions">
      <Button variant="ghost" onclick={() => (open = false)}>Cancel</Button>
      <Button variant="primary" type="submit" disabled={!valid || busy}>Remember</Button>
    </div>
  </form>
</Dialog>

<style>
  .form {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 22px;
  }
  .head {
    display: flex;
    gap: 14px;
  }
  .icon {
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    flex-shrink: 0;
    border-radius: 11px;
    color: var(--violet);
    background: var(--violet-tint);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--violet) 25%, transparent);
  }
  h2 {
    font-size: var(--text-lg);
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .head p {
    margin-top: 3px;
    font-size: var(--text-sm);
    line-height: 1.5;
    color: var(--muted);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .field-label {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--text-2);
  }
  input {
    border: 0;
    width: 100%;
    height: 38px;
    padding: 0 12px;
    border-radius: var(--radius-sm);
    color: var(--text);
    background: var(--glass);
    box-shadow: inset 0 0 0 1px var(--border-strong);
    transition: box-shadow var(--dur-1) ease;
  }
  input:focus-visible {
    outline: none;
    box-shadow:
      inset 0 0 0 1px var(--focus),
      0 0 0 3px color-mix(in srgb, var(--focus) 22%, transparent);
  }
  input[aria-invalid='true'] {
    box-shadow: inset 0 0 0 1px var(--red);
  }
  .secret {
    position: relative;
    display: block;
  }
  .secret input {
    padding-right: 40px;
    letter-spacing: 0.04em;
  }
  .reveal {
    position: absolute;
    top: 4px;
    right: 4px;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 7px;
    color: var(--muted);
  }
  .reveal:hover {
    color: var(--text);
    background: var(--glass-hover);
  }
  .hint {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .hint.bad {
    color: var(--red);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }
</style>
