// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, test } from 'vitest';
import { addAnyway, checkWord, looksLikePassword, TYPED_SECRET_LABEL } from '../../src/lib/always-redact';
import { installMockBackend, type MockBackend } from '../../src/lib/mock/ipc';
import { addToList, keepForSession } from '../../src/lib/remembered.svelte';
import { session } from '../../src/lib/session.svelte';
import { settings } from '../../src/lib/settings.svelte';
import { MAX_ALWAYS_REDACT } from '../../src/lib/types';

/** A password-shaped single token, assembled at test time (the repo takes no secret-shaped literals). */
function passwordShaped(seed = 7): string {
  const sets = ['abcdefghijkmnpqrstuvwxyz', 'ABCDEFGHJKLMNPQRSTUVWXYZ', '23456789', '!#%+=?@'];
  let a = seed;
  const pick = (chars: string) => {
    a = (Math.imul(a, 1103515245) + 12345) >>> 0;
    return chars[a % chars.length] ?? '';
  };
  return Array.from({ length: 12 }, (_, i) => pick(sets[i % sets.length] ?? '')).join('');
}

const message = (res: ReturnType<typeof checkWord>) => (res.kind === 'error' ? res.message : '');

describe('always-redact words', () => {
  test('accepts names, company names and hostnames with no warning', () => {
    for (const w of ['Nightjar', 'Robin Example', 'build-07.corp.example', 'ACME GmbH', 'Zoë', 'Müller GmbH & Co. KG'])
      expect(checkWord([], w), w).toEqual({ kind: 'ok', list: [w] });
    expect(checkWord(['Nightjar'], '  Kestrel Labs 2 & Co. ')).toEqual({
      kind: 'ok',
      list: ['Nightjar', 'Kestrel Labs 2 & Co.'],
    });
  });

  test('needs at least three characters and one line', () => {
    expect(message(checkWord([], 'ab'))).toMatch(/3 characters/);
    expect(message(checkWord([], '  ab  '))).toMatch(/3 characters/);
    expect(message(checkWord([], 'two\nlines'))).toMatch(/one line/i);
  });

  test('a single password-shaped token is a warning, not an error', () => {
    const pw = passwordShaped();
    expect(pw).toHaveLength(12);
    expect(looksLikePassword(pw)).toBe(true);
    expect(checkWord([], pw)).toEqual({ kind: 'warn' });
    expect(checkWord([], `  ${pw}  `)).toEqual({ kind: 'warn' });
  });

  test('the password shape is one token of 8+ characters with 3 of 4 classes', () => {
    expect(looksLikePassword(['Winter', '2026', '#'].join(''))).toBe(true);
    expect(looksLikePassword('build-07.corp.example')).toBe(false); // . - _ are not symbols
    expect(looksLikePassword('Nightjar')).toBe(false);
    expect(looksLikePassword('Robin Example')).toBe(false);
    expect(looksLikePassword('Müller GmbH & Co. KG')).toBe(false); // whitespace: not a single token
    expect(looksLikePassword('Ab1!xyz')).toBe(false); // 7 characters
    expect(looksLikePassword('abcdefgh1')).toBe(false); // 2 classes
  });

  test('duplicates are case-insensitive', () => {
    expect(message(checkWord(['Nightjar'], 'nightjar'))).toMatch(/already/i);
  });

  test('caps the list', () => {
    const full = Array.from({ length: MAX_ALWAYS_REDACT }, (_, i) => `name${String(i).padStart(3, '0')}`);
    expect(message(checkWord(full, 'one more'))).toMatch(/500/);
    // Errors win over the password warning.
    expect(checkWord(full, passwordShaped()).kind).toBe('error');
  });

  test('addAnyway appends a password-shaped entry, and still refuses real errors', () => {
    const pw = passwordShaped();
    expect(addAnyway(['Nightjar'], pw)).toEqual({ list: ['Nightjar', pw] });
    expect(addAnyway(['Nightjar'], 'NIGHTJAR')).toEqual({ error: expect.stringMatching(/already/i) as string });
    expect(addAnyway([], 'ab')).toEqual({ error: expect.stringMatching(/3 characters/) as string });
  });
});

describe('a password-shaped entry', () => {
  let backend: MockBackend;
  const calls = (cmd: string) => backend.calls.filter((c) => c.cmd === cmd);

  beforeEach(async () => {
    backend = installMockBackend({ scanLatencyMs: 0, session: 'empty' });
    await session.load();
    await settings.load();
  });
  afterEach(() => backend.uninstall());

  test('"Keep for this session" sends it to memory and leaves the settings alone', async () => {
    const pw = passwordShaped(11);
    expect(await keepForSession(`  ${pw}  `)).toBe(true);

    const adds = calls('session_add');
    expect(adds).toHaveLength(1);
    expect(adds[0]?.args.label).toBe(TYPED_SECRET_LABEL);
    expect(adds[0]?.args.secret).toBe('•'.repeat(pw.length)); // the mock logs it masked
    expect(session.items.map((s) => s.label)).toEqual([TYPED_SECRET_LABEL]);
    expect(Object.keys(session.items[0] ?? {}).sort()).toEqual(['addedAt', 'id', 'label']);

    expect(calls('set_settings')).toHaveLength(0);
    expect(settings.value.alwaysRedact).toEqual(['Nightjar', 'build-07.corp.example']);
    expect(JSON.stringify(backend.settings)).not.toContain(pw);
  });

  test('"Add anyway" puts it on the always-redact list, not in the session', async () => {
    const pw = passwordShaped(13);
    expect(await addToList(pw)).toBeNull();

    expect(calls('session_add')).toHaveLength(0);
    expect(settings.value.alwaysRedact).toEqual(['Nightjar', 'build-07.corp.example', pw]);
    const saved = calls('set_settings').at(-1)?.args.settings as { alwaysRedact: string[] };
    expect(saved.alwaysRedact.at(-1)).toBe(pw);
  });

  test('"Add anyway" still reports a duplicate', async () => {
    expect(await addToList('nightjar')).toMatch(/already/i);
    expect(calls('set_settings')).toHaveLength(0);
  });
});
