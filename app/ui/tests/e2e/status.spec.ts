import type { Page } from '@playwright/test';
import { expect, lastSettings, openApp, test } from './fixtures';

/**
 * What the shell tells the window through `app-status`: a hotkey that could not be set at start
 * (`?hotkeyrefused`) or that the desktop dropped, settings that could not be written
 * (`?savefail`) or read (`?settingsbad`). The texts are the ones in lib/app-status.svelte.ts and
 * lib/platform.ts.
 */
const NOT_ACTIVE = 'Not active: pick another key.';

const toast = (page: Page, title: string) => page.locator('.toast', { hasText: title });

async function openSettings(page: Page) {
  await page.getByTestId('settings-open').click();
  const sheet = page.getByRole('dialog', { name: 'Settings' });
  await expect(sheet).toBeVisible();
  return sheet;
}

test.describe('app status', () => {
  test('a_hotkey_refused_at_start_is_told_once_and_settings_says_it_is_not_active', async ({ page, problems: _ }) => {
    await openApp(page, '?hotkeyrefused');
    const notice = toast(page, 'Hotkey not active');
    await expect(notice).toBeVisible();
    await expect(notice).toContainText('Another app holds it, or the desktop refused it. Pick another key in Settings.');
    await expect(notice).toHaveCount(1);
    // It stays until dismissed; its button opens Settings, where the key says it does not work.
    await notice.getByRole('button', { name: 'Open Settings' }).click();
    const sheet = page.getByRole('dialog', { name: 'Settings' });
    await expect(sheet).toBeVisible();
    await expect(sheet.getByTestId('hotkey-inactive')).toHaveText(NOT_ACTIVE);
    await expect(sheet.getByTestId('hotkey')).toHaveAccessibleName('Hotkey Ctrl+Alt+R, not active. Change');
    // A key the shell takes works: the note goes.
    await sheet.getByTestId('hotkey').click();
    await page.keyboard.press('Control+Alt+K');
    await expect.poll(async () => (await lastSettings(page))?.hotkey).toBe('Ctrl+Alt+K');
    await expect(sheet.getByTestId('hotkey-inactive')).toHaveCount(0);
  });

  test('a_key_picked_through_the_gear_retires_the_hotkey_notice', async ({ page, problems: _ }) => {
    await openApp(page, '?hotkeyrefused');
    const notice = toast(page, 'Hotkey not active');
    await expect(notice).toBeVisible();
    // Settings opened from the gear, not the notice's button: the notice stays meanwhile.
    const sheet = await openSettings(page);
    await expect(notice).toBeVisible();
    await sheet.getByTestId('hotkey').click();
    await page.keyboard.press('Control+Alt+K');
    await expect.poll(async () => (await lastSettings(page))?.hotkey).toBe('Ctrl+Alt+K');
    await expect(sheet.getByTestId('hotkey-inactive')).toHaveCount(0);
    await expect(notice).toHaveCount(0);
  });

  test('a_working_hotkey_has_no_note_and_no_notice', async ({ page, problems: _ }) => {
    await openApp(page);
    const sheet = await openSettings(page);
    await expect(sheet.getByTestId('hotkey')).toBeVisible();
    await expect(sheet.getByTestId('hotkey-inactive')).toHaveCount(0);
    await expect(toast(page, 'Hotkey not active')).toHaveCount(0);
  });

  test('a_hotkey_the_desktop_dropped_is_told_and_shown_in_settings', async ({ page, problems: _ }) => {
    await openApp(page, '?os=linux&hotkey=portal');
    const sheet = await openSettings(page);
    await expect(sheet.getByTestId('hotkey-inactive')).toHaveCount(0);
    await page.evaluate(() => window.__REDACTOR_MOCK__?.emit('app-status', { hotkeyActive: false, notice: 'hotkeyLost' }));
    await expect(toast(page, 'Hotkey stopped working')).toContainText(
      'The desktop closed its shortcut. Set it again in Settings.',
    );
    await expect(sheet.getByTestId('hotkey-inactive')).toHaveText(NOT_ACTIVE);
  });

  test('where_a_desktop_shortcut_runs_the_command_there_is_no_key_to_pick', async ({ page, problems: _ }) => {
    await openApp(page, '?os=linux&hotkey=command&hotkeyrefused');
    const sheet = await openSettings(page);
    await expect(sheet.getByTestId('copy-command')).toBeVisible();
    await expect(sheet.getByTestId('hotkey-inactive')).toHaveCount(0);
  });

  test('settings_that_could_not_be_read_at_start_are_told_once', async ({ page, problems: _ }) => {
    await openApp(page, '?settingsbad');
    const notice = toast(page, 'Settings could not be read');
    await expect(notice).toContainText(
      'Redactor started with the defaults and kept the old file as settings.json.bad.',
    );
    await expect(notice).toHaveCount(1);
  });

  test('settings_that_could_not_be_written_are_told_once', async ({ page, problems: _ }) => {
    await openApp(page, '?savefail');
    const sheet = await openSettings(page);
    await sheet.getByRole('switch', { name: 'Personal data' }).click();
    const notice = toast(page, 'Settings not written to disk');
    await expect(notice).toContainText('They apply now and hold until Redactor closes.');
    await sheet.getByRole('switch', { name: 'Personal data' }).click();
    await expect.poll(async () => (await lastSettings(page))?.pii).toBe(false);
    await expect(notice).toHaveCount(1);
  });
});
