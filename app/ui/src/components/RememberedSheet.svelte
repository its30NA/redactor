<!--
  "Remembered": what Redactor redacts beyond its detectors.
  1. This session: secrets held in memory only (label shown), wiped on quit, lock
     and idle. 2. Always redact: plain words like names and hostnames, stored in plain text.
-->
<script lang="ts">
  import Clock from '@lucide/svelte/icons/clock';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Plus from '@lucide/svelte/icons/plus';
  import Tag from '@lucide/svelte/icons/tag';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import X from '@lucide/svelte/icons/x';
  import AddSecretDialog from './AddSecretDialog.svelte';
  import Button from './Button.svelte';
  import Sheet from './Sheet.svelte';
  import { checkWord } from '../lib/always-redact';
  import { addToList, keepForSession } from '../lib/remembered.svelte';
  import { session } from '../lib/session.svelte';
  import { settings } from '../lib/settings.svelte';
  import { MAX_ALWAYS_REDACT } from '../lib/types';
  import { workspace } from '../lib/workspace.svelte';

  let { open = $bindable(false), adding = $bindable(false) }: { open: boolean; adding?: boolean } = $props();

  let word = $state('');
  let wordError = $state<string | null>(null);
  /** The typed entry looks like a password: waiting for "Keep for this session" or "Add anyway". */
  let pending = $state(false);
  let busy = $state(false);

  const time = (iso: string) => {
    const d = new Date(iso);
    return Number.isNaN(d.getTime())
      ? ''
      : d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', hour12: false });
  };

  const idle = $derived(settings.value.autoWipeMinutes);
  const forgetNote = $derived(
    [
      'when you quit',
      settings.value.wipeOnLock ? 'when Windows locks' : null,
      idle ? `after ${idle} idle minutes` : null,
    ]
      .filter(Boolean)
      .join(', '),
  );

  async function forget(id: string) {
    if (await session.forget(id)) workspace.rescan();
  }

  async function forgetAll() {
    if (await session.clear()) workspace.rescan();
  }

  async function add(e: SubmitEvent) {
    e.preventDefault();
    const res = checkWord(settings.value.alwaysRedact, word);
    if (res.kind === 'error') {
      wordError = res.message;
      pending = false;
      return;
    }
    wordError = null;
    if (res.kind === 'warn') {
      pending = true;
      return;
    }
    pending = false;
    word = '';
    await settings.update({ alwaysRedact: res.list });
    workspace.rescan();
  }

  async function keepSession() {
    if (busy) return;
    busy = true;
    const ok = await keepForSession(word);
    busy = false;
    if (!ok) return;
    // Nothing stays in the field or on the list: the value lives in the shell's memory only.
    word = '';
    pending = false;
    workspace.rescan();
  }

  async function addAnyway() {
    if (busy) return;
    busy = true;
    const problem = await addToList(word);
    busy = false;
    pending = false;
    if (problem) {
      wordError = problem;
      return;
    }
    word = '';
    workspace.rescan();
  }

  async function remove(w: string) {
    await settings.update({ alwaysRedact: settings.value.alwaysRedact.filter((x) => x !== w) });
    workspace.rescan();
  }
</script>

<Sheet bind:open title="Remembered" description="What Redactor removes besides what it detects." width={460}>
  <section class="group" data-testid="session-section" aria-labelledby="rem-session">
    <div class="group-head">
      <h3 class="eyebrow" id="rem-session">This session</h3>
      {#if session.items.length}<span class="n mono num">{session.items.length}</span>{/if}
    </div>
    <p class="note">
      Kept in memory only, never written to disk. Forgotten {forgetNote}. Only the label is shown.
    </p>

    {#if session.items.length}
      <ul class="secrets">
        {#each session.items as s (s.id)}
          <li class="secret" data-testid="session-secret">
            <span class="glyph" aria-hidden="true"><KeyRound size={14} strokeWidth={1.9} /></span>
            <div class="secret-copy">
              <span class="secret-label">{s.label}</span>
              <span class="secret-meta">
                <span class="mask mono" role="img" aria-label="hidden value">••••••••</span>
                {#if time(s.addedAt)}<span class="at"><Clock size={11} strokeWidth={2} aria-hidden="true" /> {time(s.addedAt)}</span>{/if}
              </span>
            </div>
            <Button variant="ghost" size="sm" onclick={() => forget(s.id)} aria-label="Forget {s.label}">Forget</Button>
          </li>
        {/each}
      </ul>
    {:else}
      <div class="empty">
        Nothing remembered yet. Select text in the Input and choose <strong>Remember</strong>, or add one here.
      </div>
    {/if}

    <div class="row-actions">
      <Button variant="secondary" size="sm" icon={Plus} onclick={() => (adding = true)} data-testid="add-secret-open">
        Add a secret
      </Button>
      <Button variant="danger" size="sm" onclick={forgetAll} disabled={!session.items.length}>Forget all</Button>
    </div>
  </section>

  <section class="group" data-testid="always-section" aria-labelledby="rem-always">
    <div class="group-head">
      <h3 class="eyebrow" id="rem-always">Always redact</h3>
      <span class="n mono num">{settings.value.alwaysRedact.length} / {MAX_ALWAYS_REDACT}</span>
    </div>
    <p class="note">
      For names, company names and internal hostnames. Whole words, any case.
    </p>
    <p class="caution">
      <TriangleAlert size={13} strokeWidth={2} aria-hidden="true" />
      <span>Not for passwords: this list is stored on this PC in plain text. Use <em>This session</em> for secrets.</span>
    </p>

    <form class="add-word" onsubmit={add}>
      <input
        type="text"
        bind:value={word}
        oninput={() => {
          wordError = null;
          pending = false;
        }}
        placeholder="Name or hostname"
        aria-label="Add a word to always redact"
        aria-invalid={!!wordError}
        spellcheck="false"
        autocomplete="off"
        maxlength="120"
        data-testid="always-input"
      />
      <Button variant="secondary" size="sm" type="submit" icon={Plus} disabled={!word.trim()}>Add</Button>
    </form>
    {#if wordError}<p class="error" role="alert">{wordError}</p>{/if}
    {#if pending}
      <div class="warn" role="alert" data-testid="password-warning">
        <TriangleAlert size={14} strokeWidth={2} aria-hidden="true" />
        <div class="warn-copy">
          <p class="warn-title">This looks like a password.</p>
          <p class="warn-text">
            This list is stored on this PC in plain text. This session keeps it in memory only, and it is forgotten
            when you quit, lock or wipe.
          </p>
          <div class="warn-actions">
            <Button variant="primary" size="sm" onclick={keepSession} disabled={busy} data-testid="keep-session">
              Keep for this session
            </Button>
            <Button variant="secondary" size="sm" onclick={addAnyway} disabled={busy} data-testid="add-anyway">
              Add anyway
            </Button>
          </div>
        </div>
      </div>
    {/if}

    {#if settings.value.alwaysRedact.length}
      <ul class="chips">
        {#each settings.value.alwaysRedact as w (w)}
          <li class="chip" data-testid="always-chip">
            <Tag size={12} strokeWidth={2} aria-hidden="true" />
            <span class="chip-text">{w}</span>
            <button type="button" class="chip-x" onclick={() => remove(w)} aria-label="Remove {w}" title="Remove">
              <X size={12} strokeWidth={2.25} />
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</Sheet>

<AddSecretDialog bind:open={adding} onadded={() => workspace.rescan()} />

<style>
  .group + .group {
    margin-top: 26px;
  }
  .group-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 6px;
  }
  .n {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .note {
    font-size: var(--text-sm);
    line-height: 1.5;
    color: var(--muted);
  }
  .caution {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    margin-top: 10px;
    padding: 9px 11px;
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    line-height: 1.45;
    color: var(--text-2);
    background: var(--amber-tint);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--amber) 22%, transparent);
  }
  .caution :global(svg) {
    flex-shrink: 0;
    margin-top: 2px;
    color: var(--amber);
  }
  .caution em {
    font-style: normal;
    font-weight: 600;
    color: var(--text);
  }

  .secrets {
    list-style: none;
    margin: 12px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .secret {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 9px 8px 9px 10px;
    border-radius: var(--radius-md);
    background: var(--glass);
    box-shadow: inset 0 0 0 1px var(--border);
    animation: rise-in var(--dur-3) var(--ease-out) both;
  }
  .glyph {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    flex-shrink: 0;
    border-radius: 8px;
    color: var(--violet);
    background: var(--violet-tint);
  }
  .secret-copy {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .secret-label {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .secret-meta {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .mask {
    letter-spacing: 0.08em;
    color: var(--text-2);
  }
  .at {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .empty {
    margin-top: 12px;
    padding: 14px;
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
    line-height: 1.5;
    color: var(--muted);
    border: 1px dashed var(--border-strong);
  }
  .empty strong {
    color: var(--text-2);
    font-weight: 600;
  }
  .row-actions {
    display: flex;
    gap: 8px;
    margin-top: 12px;
  }

  .add-word {
    display: flex;
    gap: 8px;
    margin-top: 12px;
  }
  .add-word input {
    border: 0;
    flex: 1;
    min-width: 0;
    height: 32px;
    padding: 0 11px;
    border-radius: var(--radius-sm);
    color: var(--text);
    background: var(--glass);
    box-shadow: inset 0 0 0 1px var(--border-strong);
  }
  .add-word input:focus-visible {
    outline: none;
    box-shadow:
      inset 0 0 0 1px var(--focus),
      0 0 0 3px color-mix(in srgb, var(--focus) 22%, transparent);
  }
  .add-word input[aria-invalid='true'] {
    box-shadow: inset 0 0 0 1px var(--red);
  }
  .error {
    margin-top: 6px;
    font-size: var(--text-xs);
    color: var(--red);
  }
  .warn {
    display: flex;
    gap: 9px;
    align-items: flex-start;
    margin-top: 10px;
    padding: 11px 12px;
    border-radius: var(--radius-md);
    background: var(--amber-tint);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--amber) 28%, transparent);
  }
  .warn > :global(svg) {
    flex-shrink: 0;
    margin-top: 2px;
    color: var(--amber);
  }
  .warn-copy {
    flex: 1;
    min-width: 0;
  }
  .warn-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text);
  }
  .warn-text {
    margin-top: 2px;
    font-size: var(--text-sm);
    line-height: 1.45;
    color: var(--text-2);
  }
  .warn-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 10px;
  }
  .chips {
    list-style: none;
    margin: 12px 0 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 100%;
    height: 26px;
    padding: 0 3px 0 9px;
    border-radius: 13px;
    font-size: var(--text-sm);
    color: var(--text-2);
    background: var(--glass);
    box-shadow: inset 0 0 0 1px var(--border-strong);
  }
  .chip :global(svg) {
    flex-shrink: 0;
    color: var(--muted);
  }
  .chip-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chip-x {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    color: var(--muted);
  }
  .chip-x:hover {
    color: var(--red);
    background: var(--red-tint);
  }
</style>
