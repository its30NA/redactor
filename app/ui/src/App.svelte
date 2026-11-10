<script lang="ts">
  import { onMount } from 'svelte';
  import { Tween } from 'svelte/motion';
  import { cubicOut } from 'svelte/easing';
  import Bookmark from '@lucide/svelte/icons/bookmark';
  import BookmarkPlus from '@lucide/svelte/icons/bookmark-plus';
  import ClipboardPaste from '@lucide/svelte/icons/clipboard-paste';
  import Copy from '@lucide/svelte/icons/copy';
  import Eraser from '@lucide/svelte/icons/eraser';
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import Gauge from '@lucide/svelte/icons/gauge';
  import Monitor from '@lucide/svelte/icons/monitor';
  import Moon from '@lucide/svelte/icons/moon';
  import Save from '@lucide/svelte/icons/save';
  import Settings2 from '@lucide/svelte/icons/settings-2';
  import ShieldCheck from '@lucide/svelte/icons/shield-check';
  import ShieldOff from '@lucide/svelte/icons/shield-off';
  import Sun from '@lucide/svelte/icons/sun';
  import UserRound from '@lucide/svelte/icons/user-round';
  import Aurora from './components/Aurora.svelte';
  import Button from './components/Button.svelte';
  import CommandPalette, { type Command } from './components/CommandPalette.svelte';
  import Dock from './components/Dock.svelte';
  import FirstRun from './components/FirstRun.svelte';
  import RememberedSheet from './components/RememberedSheet.svelte';
  import SettingsSheet from './components/SettingsSheet.svelte';
  import StatusPill, { type PillSegment } from './components/StatusPill.svelte';
  import TitleBar from './components/TitleBar.svelte';
  import Toast from './components/Toast.svelte';
  import Workspace from './components/Workspace.svelte';
  import { appInfo, onEvent } from './lib/api';
  import { appearance, applyAppearance } from './lib/appearance.svelte';
  import { actionFor, keysFor, type Action } from './lib/keys';
  import { session } from './lib/session.svelte';
  import { settings } from './lib/settings.svelte';
  import { toasts } from './lib/toast.svelte';
  import { workspace } from './lib/workspace.svelte';
  import type { AppInfo, Sensitivity, ThemeSetting } from './lib/types';

  let info = $state<AppInfo | null>(null);
  let settingsOpen = $state(false);
  let rememberedOpen = $state(false);
  let addingSecret = $state(false);
  let paletteOpen = $state(false);
  let firstRunOpen = $state(false);
  let firstRunChecked = false;

  // The first-run card, once settings are known.
  $effect(() => {
    if (!settings.loaded || firstRunChecked) return;
    firstRunChecked = true;
    if (!settings.value.firstRunDone) firstRunOpen = true;
  });

  $effect(() => applyAppearance());

  onMount(() => {
    void settings.load();
    void session.load();
    appInfo()
      .then((i) => (info = i))
      .catch(() => {});

    const unlisten = [
      onEvent('wipe', ({ reason }) => {
        // Like Ctrl+Shift+X: a sheet or dialog may hold a typed secret.
        closeOverlays();
        session.set([]);
        workspace.wipe(reason);
      }),
      onEvent('session-changed', (items) => session.set(items)),
      onEvent('settings-changed', (s) => (settings.value = s)),
      onEvent('clipboard-scrubbed', (s) => {
        if (s.status === 'cleaned')
          toasts.show({
            tone: 'success',
            title: 'Clipboard scrubbed',
            detail: `${s.removed} ${s.removed === 1 ? 'secret' : 'secrets'} removed${s.historyCleaned ? ' · history cleaned' : ''}`,
          });
        else if (s.status === 'unverified')
          toasts.show({
            tone: 'warning',
            title: 'Clipboard scrubbed',
            detail: `${s.removed} ${s.removed === 1 ? 'secret' : 'secrets'} removed · check before pasting${s.historyCleaned ? ' · history cleaned' : ''}`,
          });
        else if (s.status === 'clean') toasts.show({ tone: 'info', title: 'Clipboard is clean' });
        else if (s.status === 'protected')
          toasts.show({ tone: 'warning', title: 'Left alone', detail: 'A password manager marked that copy private.' });
        else if (s.status === 'tooLarge')
          toasts.show({ tone: 'warning', title: 'Left alone', detail: 'The clipboard text is larger than 20 MiB.' });
      }),
    ];

    // Decorative loops rest while the window is in the background.
    const root = document.documentElement;
    const set = (active: boolean) => (root.dataset.window = active ? 'active' : 'inactive');
    const onFocus = () => set(true);
    const onBlur = () => set(false);
    const onVisibility = () => set(!document.hidden && document.hasFocus());
    onVisibility();
    window.addEventListener('focus', onFocus);
    window.addEventListener('blur', onBlur);
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      for (const u of unlisten) void u.then((f) => f());
      window.removeEventListener('focus', onFocus);
      window.removeEventListener('blur', onBlur);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  });

  // The count in the pill rolls up once per result ("12 secrets neutralized · verified").
  const count = new Tween(0, { duration: 900, easing: cubicOut });
  $effect(() => {
    void workspace.resultEpoch;
    const n = workspace.groups.redacted;
    void count.set(n, { duration: appearance.reducedMotion || n === count.target ? 0 : 900 });
  });

  const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString('en')} ${n === 1 ? one : many}`;

  const pill = $derived.by<{ tone: 'ok' | 'busy' | 'warn' | 'alert'; segments: PillSegment[]; announce?: string }>(() => {
    const shield: PillSegment = settings.value.contentProtection
      ? { text: 'Shield on', icon: ShieldCheck }
      : { text: 'Shield off', icon: ShieldOff };
    const off = !settings.value.contentProtection;
    const res = workspace.response;
    if (workspace.tooLarge) return { tone: 'warn', segments: [{ text: 'Offline' }, { text: 'Too large to scan' }] };
    if (workspace.hasText && (workspace.busy || workspace.stale || !res))
      return { tone: 'busy', segments: [{ text: 'Offline' }, { text: 'Scanning…' }] };
    if (workspace.hasText && res) {
      const n = workspace.groups.redacted;
      const shown = Math.round(count.current);
      const verified = res.verified ? 'verified' : 'check before pasting';
      const what = n ? `${plural(shown, 'secret')} neutralized` : 'Nothing sensitive';
      return {
        tone: res.verified ? (off ? 'warn' : 'ok') : 'warn',
        segments: [{ text: 'Offline' }, { text: what }, { text: verified, icon: res.verified ? ShieldCheck : undefined }],
        announce: `Offline. ${n ? `${plural(n, 'secret')} neutralized` : 'Nothing sensitive'}, ${verified}.${off ? ' Shield off.' : ''}`,
      };
    }
    return { tone: off ? 'warn' : 'ok', segments: [{ text: 'Offline' }, shield] };
  });

  // ── Keyboard ──────────────────────────────────────────────────────
  const overlayOpen = () => settingsOpen || rememberedOpen || addingSecret || paletteOpen || firstRunOpen;

  function closeOverlays() {
    settingsOpen = false;
    rememberedOpen = false;
    addingSecret = false;
    paletteOpen = false;
  }

  function run(action: Action) {
    switch (action) {
      case 'paste': {
        const inInput = !!document.activeElement?.closest('[data-testid="input-editor"]');
        void workspace.paste(inInput ? 'insert' : 'replace');
        break;
      }
      case 'copySafe':
        void workspace.copySafe();
        break;
      case 'open':
        void workspace.open();
        break;
      case 'save':
        void workspace.save();
        break;
      case 'wipe':
        closeOverlays();
        workspace.wipe();
        break;
      case 'sensitivity:relaxed':
      case 'sensitivity:balanced':
      case 'sensitivity:paranoid':
        setSensitivity(action.slice('sensitivity:'.length) as Sensitivity);
        break;
      case 'palette':
        if (firstRunOpen) break;
        if (paletteOpen) paletteOpen = false;
        else {
          closeOverlays();
          paletteOpen = true;
        }
        break;
      case 'escape':
        break;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    const t = e.target instanceof HTMLElement ? e.target : null;
    const inTextField = !!t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || !!t.closest('.cm-content'));
    if (e.defaultPrevented) return;
    const action = actionFor(e, { inTextField, overlayOpen: overlayOpen() });
    if (!action || action === 'escape') return;
    e.preventDefault();
    run(action);
  }

  function setSensitivity(sensitivity: Sensitivity) {
    if (settings.value.sensitivity === sensitivity) return;
    workspace.rescanAfter(settings.update({ sensitivity }));
  }

  const THEME_ORDER: ThemeSetting[] = ['system', 'dark', 'light'];
  const THEME_ICON = { system: Monitor, dark: Moon, light: Sun };
  const THEME_NAME = { system: 'System', dark: 'Dark', light: 'Light' };
  const nextTheme = $derived(
    THEME_ORDER[(THEME_ORDER.indexOf(settings.value.theme) + 1) % THEME_ORDER.length] ?? 'system',
  );

  const setPii = (pii: boolean) => workspace.rescanAfter(settings.update({ pii }));

  const commands = $derived<Command[]>([
    { id: 'paste', label: 'Paste and scrub', icon: ClipboardPaste, keys: keysFor('paste'), hint: 'clipboard', run: () => run('paste') },
    { id: 'copy', label: 'Copy safe text', icon: Copy, keys: keysFor('copySafe'), disabled: !workspace.ready, run: () => run('copySafe') },
    { id: 'open', label: 'Open a text file', icon: FolderOpen, keys: keysFor('open'), hint: 'file load', run: () => run('open') },
    { id: 'save', label: 'Save a safe copy', icon: Save, keys: keysFor('save'), hint: 'file export', disabled: !workspace.ready, run: () => run('save') },
    { id: 'wipe', label: 'Wipe both panes', icon: Eraser, keys: keysFor('wipe'), hint: 'clear erase', run: () => run('wipe') },
    ...(['relaxed', 'balanced', 'paranoid'] as const).map((level) => ({
      id: `sens-${level}`,
      label: `Sensitivity: ${level[0]?.toUpperCase()}${level.slice(1)}`,
      icon: Gauge,
      keys: keysFor(`sensitivity:${level}`),
      hint: settings.value.sensitivity === level ? 'current' : undefined,
      run: () => setSensitivity(level),
    })),
    {
      id: 'pii',
      label: settings.value.pii ? 'Personal data: turn off' : 'Personal data: turn on',
      icon: UserRound,
      hint: 'pii email phone iban',
      run: () => setPii(!settings.value.pii),
    },
    { id: 'remembered', label: 'Remembered…', icon: Bookmark, hint: 'session secrets always redact', run: () => (rememberedOpen = true) },
    {
      id: 'add-secret',
      label: 'Add a secret for this session…',
      icon: BookmarkPlus,
      hint: 'remember password',
      run: () => {
        rememberedOpen = true;
        addingSecret = true;
      },
    },
    {
      id: 'theme',
      label: `Theme: ${THEME_NAME[nextTheme]}`,
      icon: THEME_ICON[nextTheme],
      hint: 'dark light appearance',
      run: () => void settings.update({ theme: nextTheme }),
    },
    { id: 'settings', label: 'Settings…', icon: Settings2, hint: 'preferences hotkey', run: () => (settingsOpen = true) },
  ]);
</script>

<svelte:window {onkeydown} />

<Aurora />

<div class="app" data-ready={settings.loaded ? '' : undefined}>
  <TitleBar>
    {#snippet status()}
      <StatusPill tone={pill.tone} segments={pill.segments} announce={pill.announce} />
    {/snippet}
    {#snippet actions()}
      {@const ThemeIcon = THEME_ICON[settings.value.theme]}
      <Button
        variant="ghost"
        size="sm"
        icon={ThemeIcon}
        label="Theme: {THEME_NAME[settings.value.theme]} (switch to {THEME_NAME[nextTheme]})"
        data-testid="theme-toggle"
        onclick={() => settings.update({ theme: nextTheme })}
      />
      <span class="remembered-btn">
        <Button
          variant="ghost"
          size="sm"
          icon={Bookmark}
          label="Remembered ({session.items.length} this session)"
          pressed={rememberedOpen}
          data-testid="remembered-open"
          onclick={() => (rememberedOpen = true)}
        />
        {#if session.items.length}<span class="badge mono num" aria-hidden="true">{session.items.length}</span>{/if}
      </span>
      <Button
        variant="ghost"
        size="sm"
        icon={Settings2}
        label="Settings"
        pressed={settingsOpen}
        data-testid="settings-open"
        onclick={() => (settingsOpen = true)}
      />
    {/snippet}
  </TitleBar>

  <main class="main">
    <Workspace {info} />
  </main>

  <div class="dock-wrap">
    <Dock
      bind:sensitivity={settings.value.sensitivity}
      bind:pii={settings.value.pii}
      onsensitivity={(sensitivity) => workspace.rescanAfter(settings.update({ sensitivity }))}
      onpii={setPii}
      onpaste={() => void workspace.paste()}
      onwipe={() => workspace.wipe()}
      oncopy={() => void workspace.copySafe()}
    />
  </div>
</div>

<Toast />

<SettingsSheet bind:open={settingsOpen} {info} />
<RememberedSheet bind:open={rememberedOpen} bind:adding={addingSecret} />
<CommandPalette bind:open={paletteOpen} {commands} />
<FirstRun bind:open={firstRunOpen} />

<style>
  .app {
    position: relative;
    z-index: var(--z-app);
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .main {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    animation: rise-in var(--dur-4) var(--ease-out) both;
  }
  .dock-wrap {
    padding: 12px var(--gutter) var(--gutter);
    animation: rise-in var(--dur-4) var(--ease-out) 90ms both;
  }

  .remembered-btn {
    position: relative;
    display: inline-flex;
  }
  .badge {
    position: absolute;
    top: 1px;
    right: 0;
    display: grid;
    place-items: center;
    min-width: 14px;
    height: 14px;
    padding: 0 3px;
    border-radius: 7px;
    font-size: 9.5px;
    font-weight: 600;
    color: var(--on-grad);
    background: var(--violet);
    pointer-events: none;
  }
</style>
