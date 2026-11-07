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
  // Query: ?clipboard=demo|harsh|large|huge|empty  ?session=empty  ?latency=ms  ?firstrun
  if (import.meta.env.MODE === 'mock' && !isTauri()) {
    const { installMockBackend } = await import('./lib/mock/ipc');
    const q = new URLSearchParams(location.search);
    const clipboard = q.get('clipboard');
    installMockBackend({
      scanLatencyMs: q.has('latency') ? Number(q.get('latency')) : undefined,
      session: q.get('session') === 'empty' ? 'empty' : 'demo',
      clipboard: clipboard === 'empty' ? null : (clipboard ?? undefined),
      firstRun: q.has('firstrun'),
    });
  }
  const target = document.getElementById('app');
  if (!target) throw new Error('#app missing from index.html');
  mount(App, { target });
}

void boot();
