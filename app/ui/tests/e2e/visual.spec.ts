import { readFileSync } from 'node:fs';
import type { Page } from '@playwright/test';
import { demoInput, demoSecrets } from '../../src/lib/mock/demo';
import { expect, openApp, test } from './fixtures';

/**
 * Ten baselines (the repo is public; each update adds megabytes): the empty state with the
 * emblem, the results in all four combinations (dark/light × 1280×800/960×640), scanning, the
 * emoji-before-secret highlight, the Remembered and Settings sheets, and the app mark sizes.
 * Motion is reduced so the final frame is deterministic; it is the same frame motion ends on.
 */
const SIZES = [
  { width: 1280, height: 800 },
  { width: 960, height: 640 },
] as const;

test.use({ timezoneId: 'UTC' });

async function paste(page: Page) {
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.keyboard.press('Control+v');
}

async function results(page: Page) {
  await paste(page);
  await expect(page.getByTestId('verified')).toBeVisible();
  await expect(page.locator('.cm-chip-veil, .scramble.running')).toHaveCount(0);
}

test('empty_dark_1280', async ({ page, problems: _ }) => {
  await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
  await openApp(page);
  await expect(page).toHaveScreenshot('empty_dark_1280.png');
});

for (const scheme of ['dark', 'light'] as const) {
  for (const size of SIZES) {
    test(`results_${scheme}_${size.width}`, async ({ page, problems: _ }) => {
      await page.setViewportSize(size);
      await page.emulateMedia({ colorScheme: scheme, reducedMotion: 'reduce' });
      await openApp(page);
      await results(page);
      await expect(page).toHaveScreenshot(`results_${scheme}_${size.width}.png`);
    });
  }
}

test('scanning_dark_1280', async ({ page, problems: _ }) => {
  await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
  await openApp(page, '?latency=600000');
  await paste(page);
  await expect(page.locator('.badge.busy')).toBeVisible();
  await expect(page).toHaveScreenshot('scanning_dark_1280.png');
});

test('emoji_before_secret_highlight', async ({ page, problems: _ }) => {
  await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
  await openApp(page);
  await results(page);
  const secret = demoSecrets().signingSecret;
  expect(demoInput()).toContain(`🔑${secret}`);
  const mark = page.getByTestId('input-editor').locator('[data-fid]', { hasText: secret });
  await expect(mark).toHaveText(secret);
  const line = page.getByTestId('input-editor').locator('.cm-line', { has: page.locator('[data-fid]', { hasText: secret }) });
  await expect(line).toHaveScreenshot('emoji_before_secret.png');
});

test('remembered_dark_1280', async ({ page, problems: _ }) => {
  await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
  await openApp(page);
  await page.getByTestId('remembered-open').click();
  await expect(page.getByRole('dialog', { name: 'Remembered' })).toBeVisible();
  await expect(page).toHaveScreenshot('remembered_dark_1280.png');
});

test('settings_sheet_dark_1280', async ({ page, problems: _ }) => {
  await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
  await openApp(page);
  await page.getByTestId('settings-open').click();
  await expect(page.getByRole('dialog', { name: 'Settings' })).toBeVisible();
  await expect(page).toHaveScreenshot('settings_sheet_dark_1280.png');
});

test('mark_sizes', async ({ page, problems: _ }) => {
  await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
  await openApp(page);
  // The icon source (src/assets/mark.svg, fixed colours, exported to app/icon-source.png) at
  // the sizes Windows uses (taskbar 16-32, Alt+Tab 48) on a dark and a light taskbar.
  const markup = readFileSync(new URL('../../src/assets/mark.svg', import.meta.url), 'utf8');
  await page.evaluate((markup) => {
    const board = document.createElement('div');
    board.id = 'mark-board';
    // CSSOM only: the CSP refuses inline <style> elements and style attributes.
    board.style.cssText = 'position:fixed;left:0;bottom:0;z-index:9999;display:flex;flex-direction:column';
    for (const bg of ['#1c1c1c', '#f3f3f3']) {
      const row = document.createElement('div');
      row.style.cssText = `display:flex;align-items:center;gap:18px;padding:14px 18px;background:${bg}`;
      for (const size of [16, 20, 24, 32, 48]) {
        const svg = document.importNode(new DOMParser().parseFromString(markup, 'image/svg+xml').documentElement, true);
        svg.setAttribute('width', String(size));
        svg.setAttribute('height', String(size));
        row.append(svg);
      }
      board.append(row);
    }
    document.body.append(board);
  }, markup);
  await expect(page.locator('#mark-board')).toHaveScreenshot('mark_sizes.png');
});
