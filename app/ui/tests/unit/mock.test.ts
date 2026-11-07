// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, test } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import * as api from '../../src/lib/api';
import { demoInput, demoSecrets, tiledDemo } from '../../src/lib/mock/demo';
import { installMockBackend, type MockBackend } from '../../src/lib/mock/ipc';
import * as schema from '../../src/lib/schema';
import type { ScanRequest } from '../../src/lib/types';

let backend: MockBackend;

beforeEach(() => {
  backend = installMockBackend({ scanLatencyMs: 0 });
});
afterEach(() => backend.uninstall());

const req = (text: string, extra: Partial<ScanRequest> = {}): ScanRequest => ({
  text,
  sensitivity: 'balanced',
  pii: false,
  stripInvisible: true,
  keep: [],
  manual: [],
  ...extra,
});

describe('mock backend', () => {
  test('scanning the demo input returns the demo fixture', async () => {
    const res = await api.scan(req(demoInput()));
    expect(schema.scanResponse(res, 'res')).toEqual([]);
    expect(res.findings.length).toBe(11);
    for (const s of Object.values(demoSecrets())) expect(res.output).not.toContain(s);
  });

  test('unknown text passes through with no findings', async () => {
    const res = await api.scan(req('just a note'));
    expect(res.output).toBe('just a note');
    expect(res.findings).toEqual([]);
  });

  test('keep restores every occurrence of a value and drops its placeholders', async () => {
    const text = demoInput();
    const first = await api.scan(req(text));
    // The curl password appears twice (the shell line and the CRLF block): one value, one key.
    const curl = first.findings.find((f) => text.slice(f.start, f.end) === demoSecrets().curlPassword);
    expect(curl).toBeDefined();
    const res = await api.scan(req(text, { keep: [curl?.valueKey ?? ''] }));
    const kept = res.findings.filter((f) => f.kept);
    expect(kept.length).toBe(2);
    expect(res.output).toContain(demoSecrets().curlPassword);
    expect(res.output).not.toContain(demoSecrets().dbPassword);
    expect(res.outputSpans.length).toBe(res.findings.length - 2);
    expect(schema.scanResponse(res, 'res')).toEqual([]);
  });

  test('manual ranges become Manual findings', async () => {
    const text = 'name: Ada Lovelace';
    const res = await api.scan(req(text, { manual: [{ start: 6, end: 18 }] }));
    expect(res.output).toBe('name: [REDACTED: Manual]');
    expect(res.findings[0]?.source).toBe('manual');
    expect(schema.scanResponse(res, 'res')).toEqual([]);
  });

  test('a manual value is redacted everywhere, with one key and one placeholder', async () => {
    const text = 'pin 4711 and later pin 4711 again';
    const res = await api.scan(req(text, { manual: [{ start: 4, end: 8 }] }));
    expect(res.output).toBe('pin [REDACTED: Manual] and later pin [REDACTED: Manual] again');
    expect(res.findings.map((f) => f.source)).toEqual(['manual', 'propagated']);
    expect(new Set(res.findings.map((f) => f.valueKey)).size).toBe(1);
  });

  test('a session secret is caught until it is forgotten', async () => {
    const text = 'router login: hunter2-hunter2';
    const list = await api.sessionRemember(text, 14, text.length, 'Router');
    const added = list.at(-1);
    expect(added).toMatchObject({ label: 'Router' });
    expect(Object.keys(added ?? {}).sort()).toEqual(['addedAt', 'id', 'label']); // no length either
    expect(schema.sessionList(list, 'list')).toEqual([]);
    const hit = await api.scan(req(text));
    expect(hit.output).toBe('router login: [REDACTED: Router]');
    expect(hit.findings[0]?.source).toBe('watchlist');
    await api.sessionForget(added?.id ?? '');
    expect((await api.scan(req(text))).findings).toEqual([]);
  });

  test('session secrets never come back or get logged, and need 6 characters', async () => {
    const list = await api.sessionAdd('abcdefgh-long', 'NAS');
    expect(JSON.stringify(list)).not.toContain('abcdefgh-long');
    expect(JSON.stringify(backend.calls)).not.toContain('abcdefgh-long');
    await expect(api.sessionAdd('short', 'x')).rejects.toThrow(/too short/);
    expect(await api.sessionClear()).toEqual([]);
  });

  test('always-redact words match whole words, any case', async () => {
    const s = await api.getSettings();
    await api.setSettings({ ...s, alwaysRedact: ['Nightjar'] });
    const res = await api.scan(req('project NIGHTJAR ships; nightjars sing'));
    expect(res.output).toBe('project [REDACTED: Always redact] ships; nightjars sing');
    expect(res.findings[0]?.source).toBe('custom');
  });

  test('a wipe event clears the session', async () => {
    expect((await api.sessionList()).length).toBeGreaterThan(0);
    await backend.emit('wipe', { reason: 'idle' });
    expect(await api.sessionList()).toEqual([]);
  });

  test('a tiled 5 MiB demo gets the tiled response, built in slices', async () => {
    const text = tiledDemo(5 * 1024 * 1024);
    const res = await api.scan(req(text));
    const tiles = text.length / demoInput().length;
    expect(res.findings.length).toBe(11 * tiles);
    expect(res.outputSpans.length).toBe(11 * tiles);
    const last = res.findings.at(-1);
    expect(text.slice(last?.start, last?.end)).toBe(demoSecrets().curlPassword);
    const span = res.outputSpans.at(-1);
    expect(res.output.slice(span?.start, span?.end)).toBe('[REDACTED: Password #1]');
  });

  test('over 20 MiB of UTF-8 comes back unscanned with an inputTooLarge warning', async () => {
    const text = 'ä'.repeat(10 * 1024 * 1024 + 1); // 10 Mi UTF-16 units, 20 MiB + 2 bytes
    const res = await api.scan(req(text));
    expect(res.warnings).toEqual([{ kind: 'inputTooLarge', count: 20 * 1024 * 1024 + 2 }]);
    expect(res.output).toBe('');
    expect(res.findings).toEqual([]);
  });

  test('settings round-trip and calls are recorded', async () => {
    const s = await api.getSettings();
    await api.setSettings({ ...s, sensitivity: 'paranoid' });
    expect((await api.getSettings()).sensitivity).toBe('paranoid');
    expect(backend.calls.map((c) => c.cmd)).toEqual(['get_settings', 'set_settings', 'get_settings']);
  });

  test('unhandled commands reject', async () => {
    await expect(invoke('format_disk')).rejects.toThrow(/unhandled command/);
  });

  test('events reach api.onEvent', async () => {
    const got: string[] = [];
    await api.onEvent('wipe', (p) => got.push(p.reason));
    await backend.emit('wipe', { reason: 'tray' });
    expect(got).toEqual(['tray']);
  });
});
