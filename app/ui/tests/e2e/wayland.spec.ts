import type { Page } from '@playwright/test';
import { expect, mockCalls, openApp, test } from './fixtures';

/**
 * Settings on a Linux desktop without the Windows hotkey: the mock answers app_info with
 * `?os=linux&hotkey=portal|command&focus=1`. The texts are the ones in lib/platform.ts.
 */
const PORTAL_NOTE =
  'Your desktop may ask you to confirm this key, or let you pick another in its own keyboard settings.';
const COMMAND_NOTE = 'This desktop does not let apps set a hotkey. Add a shortcut in its keyboard settings that runs:';
const COMMAND = 'redactor --scrub-clipboard';
const FOCUS_NOTE =
  'On this desktop Redactor can reach the clipboard only while its window is in front. The hotkey brings it forward first.';

/** Open the app as that desktop, open Settings and wait until app_info has answered (the shield row turns off). */
async function openSettings(page: Page, query: string) {
  await openApp(page, query);
  await page.getByTestId('settings-open').click();
  const sheet = page.getByRole('dialog', { name: 'Settings' });
  await expect(sheet).toBeVisible();
  await expect(sheet).toContainText('Not available on Linux');
  return sheet;
}

test.describe('Wayland', () => {
  test('portal_keeps_the_recorder_and_explains_the_confirmation', async ({ page, problems: _ }) => {
    const sheet = await openSettings(page, '?os=linux&hotkey=portal');
    await expect(sheet.getByTestId('hotkey')).toBeVisible();
    await expect(sheet.getByTestId('hotkey-note')).toHaveText(PORTAL_NOTE);
    await expect(sheet.getByTestId('copy-command')).toHaveCount(0);
    await expect(sheet.getByTestId('focus-note')).toHaveCount(0);
  });

  test('command_mode_has_no_recorder_but_the_command_and_a_copy_button', async ({ page, problems: _ }) => {
    const sheet = await openSettings(page, '?os=linux&hotkey=command');
    await expect(sheet.getByTestId('hotkey')).toHaveCount(0);
    const note = sheet.getByTestId('hotkey-note');
    await expect(note).toContainText(COMMAND_NOTE);
    await expect(note.locator('code')).toHaveText(COMMAND);
    await sheet.getByTestId('copy-command').click();
    await expect(page.locator('.toast', { hasText: 'Command copied' })).toBeVisible();
    const copies = (await mockCalls(page)).filter((c) => c.cmd === 'copy_text');
    expect(copies.map((c) => c.args.text)).toEqual([COMMAND]);
  });

  test('a_clipboard_that_needs_focus_says_so', async ({ page, problems: _ }) => {
    const sheet = await openSettings(page, '?os=linux&hotkey=command&focus=1');
    await expect(sheet.getByTestId('focus-note')).toHaveText(FOCUS_NOTE);
    // The same note sits under a portal hotkey, beside the portal note.
    const portal = await openSettings(page, '?os=linux&hotkey=portal&focus=1');
    await expect(portal.getByTestId('focus-note')).toHaveText(FOCUS_NOTE);
    await expect(portal.getByTestId('hotkey-note')).toHaveText(PORTAL_NOTE);
  });

  test('keys_without_focus_adds_no_note', async ({ page, problems: _ }) => {
    const sheet = await openSettings(page, '?os=linux');
    await expect(sheet.getByTestId('hotkey')).toBeVisible();
    await expect(sheet.getByTestId('hotkey-note')).toHaveCount(0);
    await expect(sheet.getByTestId('focus-note')).toHaveCount(0);
    await expect(sheet.getByTestId('copy-command')).toHaveCount(0);
  });
});
