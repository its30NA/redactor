import { readFileSync } from 'node:fs';
import type { Page } from '@playwright/test';
import { demoInput, demoSecrets, harshInput } from '../../src/lib/mock/demo';
import { groupFindings } from '../../src/lib/findings';
import type { ScanRequest, ScanResponse } from '../../src/lib/types';
import { expect, mockCalls, openApp, test } from './fixtures';

const fixture = (name: string): ScanResponse =>
  JSON.parse(readFileSync(new URL(`../../src/lib/mock/fixtures/${name}.json`, import.meta.url), 'utf8')) as ScanResponse;

const input = (page: Page) => page.getByTestId('input-editor');
const output = (page: Page) => page.getByTestId('output-editor');

async function pasteAndWait(page: Page) {
  await page.getByRole('button', { name: /Paste & scrub/ }).click();
  await expect(page.getByTestId('verified')).toBeVisible();
}

/** The text each finding mark covers, joined per finding id (a mark may be split in spans). */
async function markedText(page: Page): Promise<Record<string, string>> {
  return page.evaluate(() => {
    const out: Record<string, string> = {};
    for (const el of document.querySelectorAll<HTMLElement>('[data-testid=input-editor] [data-fid]')) {
      const id = el.dataset.fid ?? '';
      out[id] = (out[id] ?? '') + (el.textContent ?? '');
    }
    return out;
  });
}

async function scanRequests(page: Page): Promise<ScanRequest[]> {
  return (await mockCalls(page)).filter((c) => c.cmd === 'scan').map((c) => c.args.req as ScanRequest);
}

/** Drag-select the nth occurrence of `needle` in the Input with the mouse. */
async function dragSelect(page: Page, needle: string, nth = 0) {
  const box = await page.evaluate(
    ([needle, nth]) => {
      const root = document.querySelector('[data-testid=input-editor] .cm-content');
      if (!root) return null;
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
      let seen = 0;
      for (let n = walker.nextNode(); n; n = walker.nextNode()) {
        const at = n.textContent?.indexOf(needle) ?? -1;
        if (at < 0 || seen++ < nth) continue;
        const r = document.createRange();
        r.setStart(n, at);
        r.setEnd(n, at + needle.length);
        const rect = r.getBoundingClientRect();
        return { left: rect.left, right: rect.right, y: rect.top + rect.height / 2 };
      }
      return null;
    },
    [needle, nth] as const,
  );
  if (!box) throw new Error(`"${needle}" not in the Input`);
  await page.mouse.move(box.left + 1, box.y);
  await page.mouse.down();
  await page.mouse.move(box.right - 1, box.y, { steps: 6 });
  await page.mouse.up();
}

test.describe('editor', () => {
  test('harsh_paste_highlights_exact_utf16_ranges', async ({ page, problems: _ }) => {
    await openApp(page, '?clipboard=harsh');
    await pasteAndWait(page);
    const text = harshInput();
    const res = fixture('scan-harsh');
    const marked = await markedText(page);
    expect(res.findings.length).toBeGreaterThan(0);
    for (const f of res.findings) expect(marked[String(f.id)], `finding ${f.id} (${f.label})`).toBe(text.slice(f.start, f.end));
  });

  test('emoji_and_crlf_do_not_shift_highlights', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasteAndWait(page);
    const text = demoInput();
    const s = demoSecrets();
    const res = fixture('scan-demo');
    const marked = await markedText(page);
    for (const f of res.findings) expect(marked[String(f.id)]).toBe(text.slice(f.start, f.end));

    // The signing secret sits right after an emoji (a surrogate pair).
    const signing = res.findings.find((f) => text.slice(f.start, f.end) === s.signingSecret);
    expect(signing, 'signing secret found').toBeDefined();
    expect(text.codePointAt((signing?.start ?? 0) - 2)).toBeGreaterThan(0xffff);
    // The repeated password in the CRLF block is marked at both places.
    const curl = res.findings.filter((f) => text.slice(f.start, f.end) === s.curlPassword);
    expect(curl.length).toBe(2);
    expect(text.slice(0, curl[1]?.end).includes('\r\n')).toBe(true);
    await expect(page.getByTestId('input-count')).toHaveText(`${text.length.toLocaleString('en')} chars·${text.split('\n').length} lines`);
  });

  test('output_shows_a_chip_per_placeholder', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasteAndWait(page);
    const res = fixture('scan-demo');
    const chips = output(page).locator('.cm-chip');
    await expect(chips).toHaveCount(res.outputSpans.length);
    const expected = res.outputSpans.map((sp) => res.output.slice(sp.start, sp.end));
    expect(await chips.evaluateAll((els) => els.map((e) => e.getAttribute('title')))).toEqual(expected);
    // Copy hands out exactly the verified output.
    await page.getByTestId('copy-safe').click();
    await expect(page.locator('.toast', { hasText: 'Safe text copied' })).toBeVisible();
    const copy = (await mockCalls(page)).filter((c) => c.cmd === 'copy_text').at(-1);
    expect(copy?.args.text).toBe(res.output);
  });

  test('rail_groups_values_by_tier', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasteAndWait(page);
    const g = groupFindings(fixture('scan-demo').findings);
    for (const tier of ['certain', 'likely', 'possible'] as const) {
      await expect(page.getByTestId(`tier-${tier}`).getByTestId('finding')).toHaveCount(g[tier].length);
    }
    await expect(page.getByTestId('findings-count')).toHaveText(String(g.total));
    await expect(page.getByTestId('status-pill')).toContainText(`${g.redacted} secrets neutralized`);
  });

  test('keep_sends_the_value_key_on_the_next_scan', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasteAndWait(page);
    const item = page.getByTestId('tier-certain').getByTestId('finding').first();
    const key = await item.getAttribute('data-key');
    expect(key).toBeTruthy();
    await item.getByRole('button', { name: 'Keep' }).click();
    await expect(page.getByTestId('tier-kept').locator(`[data-key="${key}"]`)).toBeVisible();
    expect((await scanRequests(page)).at(-1)?.keep).toEqual([key]);

    // Redact again drops it from keep.
    await page.getByTestId('tier-kept').getByRole('button', { name: 'Redact' }).click();
    await expect(page.getByTestId('tier-kept')).toHaveCount(0);
    expect((await scanRequests(page)).at(-1)?.keep).toEqual([]);
  });

  test('selection_redact_sends_a_manual_range', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasteAndWait(page);
    const needle = 'mail.example.test';
    const start = demoInput().indexOf(needle);
    await dragSelect(page, needle);
    const bar = page.getByTestId('selection-bar');
    await expect(bar).toBeVisible();
    await bar.getByRole('button', { name: 'Redact' }).click();
    await expect(output(page).locator('.cm-chip[title="[REDACTED: Manual]"]')).toHaveCount(1);
    expect((await scanRequests(page)).at(-1)?.manual).toEqual([{ start, end: start + needle.length }]);
    await expect(bar).toBeHidden();
  });

  test('selection_remember_calls_session_remember', async ({ page, problems: _ }) => {
    await openApp(page, '?session=empty');
    await pasteAndWait(page);
    // Appears twice (lines 8 and 14), both outside any engine finding.
    const needle = 'example.test';
    const start = demoInput().indexOf(needle);
    await dragSelect(page, needle);
    await page.getByTestId('selection-bar').getByRole('button', { name: 'Remember' }).click();
    await expect(page.locator('.toast', { hasText: 'Remembered for this session' })).toBeVisible();
    const call = (await mockCalls(page)).find((c) => c.cmd === 'session_remember');
    expect(call?.args).toMatchObject({ start, end: start + needle.length, label: 'Selection · line 8' });
    expect(call?.args.text).toBe(demoInput());
    // The rescan finds it everywhere it appears.
    const item = page.getByTestId('finding').filter({ hasText: 'Selection · line 8' });
    await expect(item).toContainText('×2');
    await expect(item).toContainText('Remembered');
  });

  test('remember_from_the_rail_calls_session_remember', async ({ page, problems: _ }) => {
    await openApp(page, '?session=empty');
    await pasteAndWait(page);
    const res = fixture('scan-demo');
    const item = page.getByTestId('tier-certain').getByTestId('finding').first();
    const key = await item.getAttribute('data-key');
    const first = res.findings.find((f) => f.valueKey === key);
    await item.getByRole('button', { name: 'Remember' }).click();
    await expect(page.locator('.toast', { hasText: 'Remembered for this session' })).toBeVisible();
    const call = (await mockCalls(page)).find((c) => c.cmd === 'session_remember');
    expect(call?.args).toMatchObject({ start: first?.start, end: first?.end });
  });

  test('short_selection_cannot_be_remembered', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasteAndWait(page);
    await dragSelect(page, 'host');
    await expect(page.getByTestId('selection-bar').getByRole('button', { name: 'Remember' })).toBeDisabled();
  });

  test('typing_rescans_after_the_debounce', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasteAndWait(page);
    const before = (await scanRequests(page)).length;
    await input(page).locator('.cm-content').click();
    await page.keyboard.press('Control+End');
    await page.keyboard.type(' more', { delay: 20 });
    await expect.poll(async () => (await scanRequests(page)).length).toBe(before + 1);
    expect((await scanRequests(page)).at(-1)?.text.endsWith(' more')).toBe(true);
  });

  test('wipe_clears_both_panes_and_the_undo_history', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasteAndWait(page);
    await page.getByRole('button', { name: /^Wipe/ }).click();
    await expect(page.getByTestId('input-count')).toHaveText('0 chars');
    await expect(page.getByTestId('empty-state')).toBeVisible();
    await expect(output(page).locator('.cm-chip')).toHaveCount(0);
    await input(page).locator('.cm-content').click();
    await page.keyboard.press('Control+z');
    await expect(page.getByTestId('input-count')).toHaveText('0 chars');
  });

  test('too_large_input_shows_the_cap_and_copies_nothing', async ({ page, problems: _ }) => {
    test.setTimeout(60_000);
    await openApp(page, '?clipboard=huge');
    await page.getByRole('button', { name: /Paste & scrub/ }).click();
    await expect(page.getByTestId('too-large')).toBeVisible({ timeout: 30_000 });
    await expect(page.getByTestId('too-large')).toContainText('Redactor scans up to 20 MB');
    await page.getByTestId('copy-safe').click();
    await expect(page.locator('.toast', { hasText: 'Too large to scan' })).toBeVisible();
    expect((await mockCalls(page)).some((c) => c.cmd === 'copy_text')).toBe(false);
  });
});
