<!--
  Settings, wired to `Settings` (stored by the shell, never any text): scanning, privacy,
  hotkey and tray, appearance, and About.
-->
<script lang="ts">
  import Keyboard from '@lucide/svelte/icons/keyboard';
  import Kbd from './Kbd.svelte';
  import Segmented, { type SegmentOption } from './Segmented.svelte';
  import Sheet from './Sheet.svelte';
  import Toggle from './Toggle.svelte';
  import { hotkeyFrom, hotkeyProblem } from '../lib/keys';
  import { settings } from '../lib/settings.svelte';
  import { workspace } from '../lib/workspace.svelte';
  import type { AppInfo, MotionSetting, Sensitivity, Settings, ThemeSetting } from '../lib/types';

  let { open = $bindable(false), info }: { open: boolean; info: AppInfo | null } = $props();

  const themeOptions: SegmentOption<ThemeSetting>[] = [
    { value: 'system', label: 'System' },
    { value: 'dark', label: 'Dark' },
    { value: 'light', label: 'Light' },
  ];
  const motionOptions: SegmentOption<MotionSetting>[] = [
    { value: 'system', label: 'System' },
    { value: 'on', label: 'Reduced' },
    { value: 'off', label: 'Full' },
  ];
  const sensitivityOptions: SegmentOption<Sensitivity>[] = [
    { value: 'relaxed', label: 'Relaxed', tone: 'mint' },
    { value: 'balanced', label: 'Balanced', tone: 'cyan' },
    { value: 'paranoid', label: 'Paranoid', tone: 'violet' },
  ];
  type Placeholder = Settings['placeholderStyle'];
  const placeholderOptions: SegmentOption<Placeholder>[] = [
    { value: 'labeled', label: 'Labeled', hint: '[REDACTED: Password]' },
    { value: 'generic', label: 'Generic', hint: '[REDACTED]' },
  ];
  type Idle = `${Settings['autoWipeMinutes']}`;
  const idleOptions: SegmentOption<Idle>[] = [
    { value: '0', label: 'Off' },
    { value: '5', label: '5' },
    { value: '10', label: '10' },
    { value: '30', label: '30' },
  ];

  /** Changes that alter the output scan again. */
  const rescanning = (patch: Partial<Settings>) => void settings.update(patch).then(() => workspace.rescan());
  const update = (patch: Partial<Settings>) => void settings.update(patch);

  // ── Hotkey capture ──
  let capturing = $state(false);
  let hotkeyError = $state<string | null>(null);
  let pending = $state<string | null>(null);

  function startCapture() {
    capturing = true;
    pending = null;
    hotkeyError = null;
  }

  function onHotkeyKey(e: KeyboardEvent) {
    if (!capturing) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') {
      capturing = false;
      pending = null;
      return;
    }
    const combo = hotkeyFrom(e);
    if (!combo) {
      pending = [e.ctrlKey && 'Ctrl', e.shiftKey && 'Shift', e.altKey && 'Alt'].filter(Boolean).join('+') || null;
      return;
    }
    const problem = hotkeyProblem(combo);
    if (problem) {
      hotkeyError = `${combo}: ${problem}`;
      pending = null;
      return;
    }
    capturing = false;
    pending = null;
    hotkeyError = null;
    update({ hotkey: combo });
  }

  const shownKeys = $derived((capturing ? (pending ?? '') : settings.value.hotkey).split('+').filter(Boolean));
</script>

<Sheet bind:open title="Settings" description="Stored on this PC only. Never any text." width={460}>
  <section class="group">
    <h3 class="eyebrow">Scanning</h3>
    <div class="row">
      <div class="row-copy">
        <p class="row-title">Sensitivity</p>
        <p class="row-desc">Paranoid also flags anything that looks random.</p>
      </div>
      <Segmented
        size="sm"
        label="Sensitivity"
        options={sensitivityOptions}
        value={settings.value.sensitivity}
        onchange={(sensitivity) => rescanning({ sensitivity })}
      />
    </div>
    <div class="row">
      <div class="row-copy">
        <p class="row-title">Personal data</p>
        <p class="row-desc">E-mail, phone, IBAN, card numbers and IP addresses.</p>
      </div>
      <Toggle quiet label="Personal data" checked={settings.value.pii} onchange={(pii) => rescanning({ pii })} />
    </div>
    <div class="row">
      <div class="row-copy">
        <p class="row-title">Strip invisible characters</p>
        <p class="row-desc">Zero-width and tag characters can hide text.</p>
      </div>
      <Toggle
        quiet
        label="Strip invisible characters"
        checked={settings.value.stripInvisible}
        onchange={(stripInvisible) => rescanning({ stripInvisible })}
      />
    </div>
    <div class="row">
      <div class="row-copy">
        <p class="row-title">Placeholders</p>
        <p class="row-desc mono">
          {settings.value.placeholderStyle === 'labeled' ? '[REDACTED: Password]' : '[REDACTED]'}
        </p>
      </div>
      <Segmented
        size="sm"
        label="Placeholders"
        options={placeholderOptions}
        value={settings.value.placeholderStyle}
        onchange={(placeholderStyle) => rescanning({ placeholderStyle })}
      />
    </div>
  </section>

  <section class="group">
    <h3 class="eyebrow">Privacy</h3>
    <div class="row">
      <div class="row-copy">
        <p class="row-title">Screen-capture shield</p>
        <p class="row-desc">Screenshots and screen sharing see a blank window.</p>
      </div>
      <Toggle
        quiet
        label="Screen-capture shield"
        checked={settings.value.contentProtection}
        onchange={(contentProtection) => update({ contentProtection })}
      />
    </div>
    <div class="row">
      <div class="row-copy">
        <p class="row-title">Wipe when idle</p>
        <p class="row-desc">Minutes without input. Clears both panes and the session list.</p>
      </div>
      <Segmented
        size="sm"
        label="Wipe when idle"
        options={idleOptions}
        value={String(settings.value.autoWipeMinutes) as Idle}
        onchange={(v) => update({ autoWipeMinutes: Number(v) as Settings['autoWipeMinutes'] })}
      />
    </div>
    <div class="row">
      <div class="row-copy">
        <p class="row-title">Wipe when Windows locks</p>
        <p class="row-desc">Win+L or the screen saver lock.</p>
      </div>
      <Toggle
        quiet
        label="Wipe when Windows locks"
        checked={settings.value.wipeOnLock}
        onchange={(wipeOnLock) => update({ wipeOnLock })}
      />
    </div>
  </section>

  <section class="group">
    <h3 class="eyebrow">Hotkey and tray</h3>
    <div class="row">
      <div class="row-copy">
        <p class="row-title">Scrub the clipboard</p>
        <p class="row-desc">From any app, without opening the window.</p>
      </div>
      <button
        type="button"
        class="hotkey"
        class:capturing
        onclick={startCapture}
        onkeydown={onHotkeyKey}
        onblur={() => {
          capturing = false;
          pending = null;
        }}
        aria-label={capturing ? 'Press the new shortcut, or Esc to cancel' : `Hotkey ${settings.value.hotkey}. Change`}
        data-testid="hotkey"
      >
        <Keyboard size={14} strokeWidth={1.9} aria-hidden="true" />
        {#if shownKeys.length}<Kbd keys={shownKeys} />{:else}<span class="press">Press keys…</span>{/if}
        {#if !capturing}<span class="change">Change</span>{/if}
      </button>
    </div>
    {#if hotkeyError}<p class="error" role="alert">{hotkeyError}</p>{/if}
    <div class="row">
      <div class="row-copy">
        <p class="row-title">Keep running in the tray</p>
        <p class="row-desc">Closing the window keeps the hotkey working.</p>
      </div>
      <Toggle quiet label="Keep running in the tray" checked={settings.value.trayMode} onchange={(trayMode) => update({ trayMode })} />
    </div>
  </section>

  <section class="group">
    <h3 class="eyebrow">Appearance</h3>
    <div class="row">
      <div class="row-copy">
        <p class="row-title">Theme</p>
        <p class="row-desc">Follows Windows by default.</p>
      </div>
      <Segmented size="sm" label="Theme" options={themeOptions} value={settings.value.theme} onchange={(theme) => update({ theme })} />
    </div>
    <div class="row">
      <div class="row-copy">
        <p class="row-title">Motion</p>
        <p class="row-desc">Reduced turns animation off.</p>
      </div>
      <Segmented
        size="sm"
        label="Motion"
        options={motionOptions}
        value={settings.value.reducedMotion}
        onchange={(reducedMotion) => update({ reducedMotion })}
      />
    </div>
  </section>

  <section class="group">
    <h3 class="eyebrow">About</h3>
    <dl class="about">
      <dt>Version</dt>
      <dd class="mono">{info?.version ?? '–'}</dd>
      <dt>Engine</dt>
      <dd class="mono">{info ? `${info.engineVersion} · ${info.ruleCount} rules` : '–'}</dd>
      <dt>Platform</dt>
      <dd class="mono">{info?.platform ?? '–'}</dd>
    </dl>
    <p class="row-desc promise">No network code. Nothing you paste is written to disk.</p>
  </section>
</Sheet>

<style>
  .group + .group {
    margin-top: 26px;
  }
  .group .eyebrow {
    margin-bottom: 6px;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 0;
    border-bottom: 1px solid var(--hairline);
  }
  .row-copy {
    flex: 1;
    min-width: 0;
  }
  .row :global(.seg) {
    width: 216px;
    flex-shrink: 0;
  }
  .row :global(.toggle) {
    flex-shrink: 0;
    padding: 0 6px;
  }
  .row-title {
    font-weight: 500;
  }
  .row-desc {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .row-desc.mono {
    font-size: var(--text-xs);
  }
  .hotkey {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
    height: 32px;
    padding: 0 10px;
    border-radius: var(--radius-sm);
    color: var(--muted);
    background: var(--glass);
    box-shadow: inset 0 0 0 1px var(--border-strong);
    transition: box-shadow var(--dur-1) ease;
  }
  .hotkey:hover {
    color: var(--text);
  }
  .hotkey.capturing {
    color: var(--cyan);
    box-shadow:
      inset 0 0 0 1px var(--cyan),
      0 0 0 3px color-mix(in srgb, var(--cyan) 20%, transparent);
  }
  .press {
    font-size: var(--text-sm);
  }
  .change {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--cyan);
  }
  .error {
    margin-top: 6px;
    font-size: var(--text-xs);
    color: var(--red);
  }
  .about {
    display: grid;
    grid-template-columns: 96px 1fr;
    gap: 8px 12px;
    margin: 8px 0 0;
    font-size: var(--text-sm);
  }
  .about dt {
    color: var(--muted);
  }
  .about dd {
    margin: 0;
    color: var(--text-2);
  }
  .promise {
    margin-top: 16px;
  }
</style>
