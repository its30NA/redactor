import type { Page } from '@playwright/test';
import { expect, mockCalls, openApp, test } from './fixtures';

/**
 * Settings on a Linux desktop without the Windows hotkey: the mock answers app_info with
 * `?os=linux&hotkey=portal|command&focus=1`. The texts are the ones in lib/platform.ts.
 */
const PORTAL_NOTE =
  'Your desktop may ask you to confirm this key, or let you pick another in its own keyboard settings.';
const COMMAND_NOTE = 'This desktop does not let apps set a hotkey. Add a shortcut in its keyboard settings that runs:';
/** The mock's executable (MOCK_EXECUTABLE) with the flag. */
const COMMAND = '/opt/redactor/Redactor --scrub-clipboard';
const FIRST_RUN_COMMAND_LINE =
  "Scrub the clipboard with a shortcut you add in your desktop's keyboard settings (see Settings).";
const FOCUS_NOTE = 'Redactor can reach the clipboard only while its window is in front.';
const SCRUB_ROW = 'From any app, without opening the window.';
const SCRUB_ROW_FOCUS = 'From any app. On this desktop the window comes to the front first.';
const FIRST_RUN_HOTKEY = 'Scrubs the clipboard in place, without opening this window.';
const FIRST_RUN_HOTKEY_FOCUS = 'Brings this window forward, then scrubs the clipboard.';
const LOCK_DESC = 'Lock signals from logind or a screen saver, and suspend. Lockers that send none go unseen.';
const LOCK_NOTE =
  'No lock signal found on this desktop: a screen lock does not wipe. Suspend and the idle timer still do.';

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

  test('the_command_names_this_program_and_quotes_a_path_with_spaces', async ({ page, problems: _ }) => {
    const exe = '/home/me/My Apps/Redactor.AppImage';
    const sheet = await openSettings(page, `?os=linux&hotkey=command&exe=${encodeURIComponent(exe)}`);
    const quoted = `'${exe}' --scrub-clipboard`;
    await expect(sheet.getByTestId('hotkey-note').locator('code')).toHaveText(quoted);
    await sheet.getByTestId('copy-command').click();
    const copies = (await mockCalls(page)).filter((c) => c.cmd === 'copy_text');
    expect(copies.map((c) => c.args.text)).toEqual([quoted]);
  });

  test('inside_a_flatpak_the_command_is_flatpak_run', async ({ page, problems: _ }) => {
    const sheet = await openSettings(page, '?os=linux&hotkey=command&flatpak=io.github.its30na.redactor');
    const command = 'flatpak run io.github.its30na.redactor --scrub-clipboard';
    await expect(sheet.getByTestId('hotkey-note').locator('code')).toHaveText(command);
    await sheet.getByTestId('copy-command').click();
    const copies = (await mockCalls(page)).filter((c) => c.cmd === 'copy_text');
    expect(copies.map((c) => c.args.text)).toEqual([command]);
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

  test('the_lock_row_names_what_is_heard_and_notes_a_desktop_with_no_lock_signal', async ({ page, problems: _ }) => {
    const sheet = await openSettings(page, '?os=linux&nolock');
    await expect(sheet).toContainText(LOCK_DESC);
    await expect(sheet.getByTestId('lock-note')).toHaveText(LOCK_NOTE);
    // The wipe on suspend still works: the switch stays usable.
    await expect(sheet.getByRole('switch', { name: 'Wipe when the screen locks' })).toBeEnabled();
    // Where logind or a screen saver is there, no note.
    const heard = await openSettings(page, '?os=linux');
    await expect(heard).toContainText(LOCK_DESC);
    await expect(heard.getByTestId('lock-note')).toHaveCount(0);
  });

  test('the_first_run_card_shows_no_keycaps_where_the_desktop_runs_a_command', async ({ page, problems: _ }) => {
    await openApp(page, '?firstrun&os=linux&hotkey=command');
    const card = page.getByTestId('first-run');
    await expect(card).toBeVisible();
    await expect(card.getByTestId('hotkey-command-line')).toHaveText(FIRST_RUN_COMMAND_LINE);
    await expect(card.locator('kbd')).toHaveCount(0);
    // A portal desktop may honour the key, so the keycaps stay.
    await openApp(page, '?firstrun&os=linux&hotkey=portal');
    const portal = page.getByTestId('first-run');
    await expect(portal.locator('kbd').first()).toBeVisible();
    await expect(portal.getByTestId('hotkey-command-line')).toHaveCount(0);
  });

  test('where_the_clipboard_needs_focus_settings_does_not_promise_a_closed_window', async ({ page, problems: _ }) => {
    const sheet = await openSettings(page, '?os=linux&hotkey=portal&focus=1');
    await expect(sheet.getByTestId('scrub-row-desc')).toHaveText(SCRUB_ROW_FOCUS);
    await expect(sheet.getByTestId('focus-note')).toHaveText(FOCUS_NOTE);
    await expect(sheet).not.toContainText(SCRUB_ROW);
    // Without the need for focus the row keeps its text.
    const plain = await openSettings(page, '?os=linux&hotkey=portal');
    await expect(plain.getByTestId('scrub-row-desc')).toHaveText(SCRUB_ROW);
  });

  test('where_the_clipboard_needs_focus_the_first_run_card_says_the_window_comes_forward', async ({
    page,
    problems: _,
  }) => {
    await openApp(page, '?firstrun&os=linux&hotkey=portal&focus=1');
    const card = page.getByTestId('first-run');
    await expect(card).toBeVisible();
    await expect(card.getByTestId('hotkey-desc')).toHaveText(FIRST_RUN_HOTKEY_FOCUS);
    await expect(card).not.toContainText(FIRST_RUN_HOTKEY);
    await openApp(page, '?firstrun&os=linux&hotkey=portal');
    await expect(page.getByTestId('first-run').getByTestId('hotkey-desc')).toHaveText(FIRST_RUN_HOTKEY);
  });
});

/**
 * Until app_info answers, the page does not know the desktop: it must not promise the Windows
 * shield (the welcome card's row, the pill's segment). `?appinfo=slow|fail` delays or fails it.
 */
test.describe('before app_info answers', () => {
  test('a_slow_app_info_shows_no_shield_promise_and_holds_the_welcome_card', async ({ page, problems: _ }) => {
    await page.goto('/?firstrun&os=linux&appinfo=slow');
    const pill = page.getByTestId('status-pill');
    await expect(pill).toContainText('Offline');
    await expect(pill).not.toContainText('Shield');
    await expect(page.getByTestId('first-run')).toHaveCount(0);
    // Once it answers, the card opens without the row this desktop cannot keep.
    const card = page.getByTestId('first-run');
    await expect(card).toBeVisible({ timeout: 10_000 });
    await expect(card).toContainText('Offline by design');
    await expect(card).not.toContainText('Shielded from screen capture');
    await expect(pill).not.toContainText('Shield');
  });

  test('a_failed_app_info_says_so_and_makes_no_shield_promise', async ({ page, problems: _ }) => {
    await openApp(page, '?firstrun&os=linux&appinfo=fail');
    const card = page.getByTestId('first-run');
    await expect(card).toBeVisible();
    await expect(card).not.toContainText('Shielded from screen capture');
    await expect(page.getByTestId('status-pill')).not.toContainText('Shield');
    await expect(page.locator('.toast', { hasText: 'App details could not be loaded' })).toHaveCount(1);
  });
});

test.describe('a scrub where the clipboard needs focus', () => {
  test('waiting_then_the_result_replaces_it', async ({ page, problems: _ }) => {
    // Started with `--scrub-clipboard`: the mock tells the wait, then the result, like the shell.
    await page.goto('/?os=linux&hotkey=portal&focus=1&scrub&clipboard=empty');
    const waiting = page.locator('.toast', { hasText: "Waiting for Redactor's window to scrub the clipboard…" });
    await expect(waiting).toBeVisible();
    await expect(page.locator('.toast', { hasText: 'Nothing to scrub' })).toBeVisible();
    await expect(waiting).toHaveCount(0);
  });
});

