import './lib/harden-now';
import '@fontsource-variable/inter';
import '@fontsource-variable/jetbrains-mono';
import './styles/tokens.css';
import './styles/base.css';
import './styles/motion.css';
import './styles/editor.css';
import { mount } from 'svelte';
import { isTauri } from '@tauri-apps/api/core';
import App from './App.svelte';

async function boot(): Promise<void> {
  // `--mode mock` (npm run dev, the Playwright build) runs against the fixture backend.
  // The condition is static, so production bundles contain no mock code at all.
  // Query: ?clipboard=demo|harsh|large|huge|empty  ?session=empty  ?latency=ms  ?firstrun  ?os=macos|linux  ?hotkey=portal|command  ?focus=1 (the last two with os)  ?scrub (started with --scrub-clipboard)  ?appinfo=slow|fail  ?exe=path (with os)  ?hotkeyrefused  ?settingsbad  ?savefail  ?nolock (with os)
  if (import.meta.env.MODE === 'mock' && !isTauri()) {
    const { installMockBackend } = await import('./lib/mock/ipc');
    const q = new URLSearchParams(location.search);
    const clipboard = q.get('clipboard');
    const os = q.get('os');
    const hotkey = q.get('hotkey');
    const info = q.get('appinfo');
    installMockBackend({
      scanLatencyMs: q.has('latency') ? Number(q.get('latency')) : undefined,
      session: q.get('session') === 'empty' ? 'empty' : 'demo',
      clipboard: clipboard === 'empty' ? null : (clipboard ?? undefined),
      firstRun: q.has('firstrun'),
      os: os === 'macos' || os === 'linux' ? os : undefined,
      hotkey: hotkey === 'portal' || hotkey === 'command' ? hotkey : undefined,
      clipboardNeedsFocus: q.get('focus') === '1',
      scrubOnStart: q.has('scrub'),
      appInfo: info === 'slow' || info === 'fail' ? info : undefined,
      executable: q.get('exe') ?? undefined,
      hotkeyRefused: q.has('hotkeyrefused'),
      settingsUnreadable: q.has('settingsbad'),
      saveFails: q.has('savefail'),
      noLockSignal: q.has('nolock'),
    });
  }
  const target = document.getElementById('app');
  if (!target) throw new Error('#app missing from index.html');
  mount(App, { target });
}

void boot();
