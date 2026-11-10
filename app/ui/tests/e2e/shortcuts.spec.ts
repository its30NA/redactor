import type { Page } from '@playwright/test';
import { demoInput } from '../../src/lib/mock/demo';
import type { ScanRequest } from '../../src/lib/types';
import { expect, lastSettings, mockCalls, openApp, test } from './fixtures';

const scans = async (page: Page): Promise<ScanRequest[]> =>
  (await mockCalls(page)).filter((c) => c.cmd === 'scan').map((c) => c.args.req as ScanRequest);
const called = async (page: Page, cmd: string) => (await mockCalls(page)).filter((c) => c.cmd === cmd);

/** Focus the page body, so shortcuts are not typed into the editor. */
const blur = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());

async function pasted(page: Page) {
  await blur(page);
  await page.keyboard.press('Control+v');
  await expect(page.getByTestId('verified')).toBeVisible();
}

test.describe('shortcuts', () => {
  test('ctrl_v_pastes_and_scans', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasted(page);
    await expect(page.getByTestId('input-count')).toContainText(`${demoInput().length.toLocaleString('en')} chars`);
    expect((await scans(page)).at(-1)?.text).toBe(demoInput());
  });

  test('ctrl_v_inside_the_input_pastes_at_the_cursor', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasted(page);
    await page.getByTestId('input-editor').locator('.cm-content').click();
    await page.keyboard.press('Control+End');
    await page.keyboard.press('Control+v');
    await expect(page.getByTestId('input-count')).toContainText(`${(demoInput().length * 2).toLocaleString('en')} chars`);
  });

  test('ctrl_shift_c_copies_the_safe_text', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasted(page);
    await page.keyboard.press('Control+Shift+C');
    await expect(page.locator('.toast', { hasText: 'Safe text copied' })).toBeVisible();
    expect((await called(page, 'copy_text')).length).toBe(1);
  });

  test('ctrl_o_opens_a_file_and_ctrl_s_saves_a_safe_copy', async ({ page, problems: _ }) => {
    await openApp(page);
    await page.keyboard.press('Control+o');
    await expect(page.getByTestId('verified')).toBeVisible();
    await expect(page.locator('.file')).toHaveText('staging-notes.txt');
    await blur(page);
    await page.keyboard.press('Control+s');
    await expect(page.locator('.toast', { hasText: 'Safe copy saved' })).toBeVisible();
    const save = (await called(page, 'save_text_file')).at(-1);
    expect(save?.args.suggestedName).toBe('staging-notes.safe.txt');
  });

  test('ctrl_shift_x_wipes', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasted(page);
    await page.keyboard.press('Control+Shift+X');
    await expect(page.getByTestId('input-count')).toHaveText('0 chars');
    await expect(page.locator('.toast', { hasText: 'Wiped' })).toBeVisible();
  });

  test('ctrl_1_2_3_set_the_sensitivity_and_rescan', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasted(page);
    for (const [key, level] of [
      ['1', 'relaxed'],
      ['3', 'paranoid'],
      ['2', 'balanced'],
    ] as const) {
      const before = (await scans(page)).length;
      await page.keyboard.press(`Control+${key}`);
      await expect(page.getByRole('radio', { name: new RegExp(level, 'i') }).first()).toHaveAttribute('aria-checked', 'true');
      await expect.poll(async () => (await scans(page)).at(-1)?.sensitivity).toBe(level);
      expect((await scans(page)).length).toBe(before + 1);
    }
    expect((await lastSettings(page))?.sensitivity).toBe('balanced');
  });

  test('ctrl_k_opens_the_palette_and_runs_a_command', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasted(page);
    await page.keyboard.press('Control+k');
    const palette = page.getByTestId('palette');
    await expect(palette).toBeVisible();
    await page.keyboard.type('wipe');
    await expect(palette.getByRole('option')).toHaveCount(1);
    await page.keyboard.press('Enter');
    await expect(palette).toBeHidden();
    await expect(page.getByTestId('input-count')).toHaveText('0 chars');

    await page.keyboard.press('Control+k');
    await expect(palette).toBeVisible();
    await page.keyboard.press('ArrowDown');
    await expect(palette.getByRole('option').nth(1)).toHaveAttribute('aria-selected', 'true');
    await page.keyboard.press('Escape');
    await expect(palette).toBeHidden();
  });

  test('esc_closes_sheets', async ({ page, problems: _ }) => {
    await openApp(page);
    await page.getByTestId('remembered-open').click();
    await expect(page.getByRole('dialog', { name: 'Remembered' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog', { name: 'Remembered' })).toBeHidden();
    await page.getByTestId('settings-open').click();
    await expect(page.getByRole('dialog', { name: 'Settings' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog', { name: 'Settings' })).toBeHidden();
  });

  test('shortcuts_wait_while_a_sheet_is_open_but_wipe_works', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasted(page);
    await page.getByTestId('settings-open').click();
    await page.keyboard.press('Control+o');
    expect((await called(page, 'open_text_file')).length).toBe(0);
    await page.keyboard.press('Control+Shift+X');
    await expect(page.getByRole('dialog', { name: 'Settings' })).toBeHidden();
    await expect(page.getByTestId('input-count')).toHaveText('0 chars');
  });
});

test.describe('scheduling and size', () => {
  test('stale_responses_dropped', async ({ page, problems: _ }) => {
    await openApp(page, '?latency=500');
    await blur(page);
    await page.keyboard.press('Control+v');
    await expect.poll(async () => (await scans(page)).length).toBe(1);
    // Type while the first scan is still running: its response no longer matches the text.
    await page.getByTestId('input-editor').locator('.cm-content').click();
    await page.keyboard.press('Control+End');
    await page.keyboard.type('!');
    // The second scan starts only when the first has come back; by then it must be dropped.
    await expect.poll(async () => (await scans(page)).length, { timeout: 5000 }).toBe(2);
    await expect(page.getByTestId('verified')).toHaveCount(0);
    await expect(page.getByTestId('output-editor').locator('.cm-chip')).toHaveCount(0);
    await expect(page.getByTestId('verified')).toBeVisible();
    expect((await scans(page)).at(-1)?.text).toBe(`${demoInput()}!`);
  });
});

test.describe('copy after a failed or pending scan', () => {
  test('a failed rescan makes copy and save say so instead of doing nothing', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasted(page);
    await page.evaluate(() => window.__REDACTOR_MOCK__?.failScans(true));
    await page.getByTestId('input-editor').locator('.cm-content').press('End');
    await page.keyboard.type('!');
    await expect(page.locator('.toast', { hasText: 'The safe output was not updated' })).toBeVisible();
    await blur(page);
    await page.keyboard.press('Control+Shift+C');
    await expect(page.locator('.toast', { hasText: 'Scan failed, nothing copied' })).toBeVisible();
    await page.keyboard.press('Control+s');
    await expect(page.locator('.toast', { hasText: 'Scan failed, nothing saved' })).toBeVisible();
    expect(await called(page, 'copy_text')).toHaveLength(0);
    expect(await called(page, 'save_text_file')).toHaveLength(0);
  });

  test('a copy during a sensitivity change waits for the rescan under the new setting', async ({ page, problems: _ }) => {
    await openApp(page);
    await pasted(page);
    await page.evaluate(() => window.__REDACTOR_MOCK__?.delaySettings(800));
    await page.keyboard.press('Control+3');
    await page.keyboard.press('Control+Shift+C');
    await expect(page.locator('.toast', { hasText: 'Safe text copied' })).toBeVisible();
    const calls = await mockCalls(page);
    const copy = calls.findIndex((c) => c.cmd === 'copy_text');
    const lastScan = calls.slice(0, copy).findLastIndex((c) => c.cmd === 'scan');
    const settingsSet = calls.findIndex((c) => c.cmd === 'set_settings');
    expect(copy).toBeGreaterThan(-1);
    expect(lastScan).toBeGreaterThan(settingsSet);
    expect((calls[lastScan]?.args.req as ScanRequest).sensitivity).toBe('paranoid');
  });
});
