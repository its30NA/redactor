// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, test } from 'vitest';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import * as api from '../../src/lib/api';
import { buildScanRequest, definitelyTooLarge, MAX_INPUT_BYTES, utf8Length } from '../../src/lib/scan-request';
import type { ScanRequest } from '../../src/lib/types';

const settings = { sensitivity: 'paranoid', pii: true, stripInvisible: false } as const;

describe('buildScanRequest', () => {
  test('carries the settings, the kept values and the merged manual ranges', () => {
    const req = buildScanRequest('name: Ada Lovelace, pin 1234', settings, new Set(['bb', 'aa']), [
      { start: 11, end: 18 },
      { start: 6, end: 10 },
      { start: 9, end: 12 },
    ]);
    expect(req).toEqual({
      text: 'name: Ada Lovelace, pin 1234',
      sensitivity: 'paranoid',
      pii: true,
      stripInvisible: false,
      keep: ['aa', 'bb'],
      manual: [{ start: 6, end: 18 }],
    });
  });

  test('clips manual ranges to the text and drops empty ones', () => {
    const req = buildScanRequest('short', settings, new Set(), [
      { start: 3, end: 50 },
      { start: 9, end: 12 },
    ]);
    expect(req.manual).toEqual([{ start: 3, end: 5 }]);
  });
});

describe('the 20 MiB cap', () => {
  test('UTF-16 length above the cap is always too large (UTF-8 is never shorter)', () => {
    expect(MAX_INPUT_BYTES).toBe(20 * 1024 * 1024);
    expect(definitelyTooLarge(MAX_INPUT_BYTES + 1)).toBe(true);
    expect(definitelyTooLarge(MAX_INPUT_BYTES)).toBe(false);
  });

  test('utf8Length counts bytes like the engine does', () => {
    expect(utf8Length('abc')).toBe(3);
    expect(utf8Length('fürs')).toBe(5);
    expect(utf8Length('€')).toBe(3);
    expect(utf8Length('🔑')).toBe(4);
    expect(utf8Length('a\r\nb')).toBe(4);
    expect(utf8Length('🔑x')).toBe(new TextEncoder().encode('🔑x').length);
  });
});

describe('scan IPC (keep / manual)', () => {
  let sent: ScanRequest[] = [];
  beforeEach(() => {
    sent = [];
    mockIPC((cmd, payload) => {
      if (cmd === 'scan') sent.push((payload as { req: ScanRequest }).req);
      return null;
    });
  });
  afterEach(() => clearMocks());

  test('Keep and Redact travel as keep:[valueKey] and manual:[{start,end}] in UTF-16 units', async () => {
    const text = '👉🔑hunter2hunter2';
    const start = text.indexOf('hunter2');
    await api.scan(buildScanRequest(text, settings, new Set(['00ff']), [{ start, end: text.length }]));
    expect(sent).toEqual([
      {
        text,
        sensitivity: 'paranoid',
        pii: true,
        stripInvisible: false,
        keep: ['00ff'],
        manual: [{ start: 4, end: 18 }],
      },
    ]);
  });
});
