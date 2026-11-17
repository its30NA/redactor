import type { Page } from '@playwright/test';
import { demoInput } from '../../src/lib/mock/demo';
import { expect, lastSettings, mockCalls, openApp, test } from './fixtures';

/**
 * The UI as it runs on a Mac (`?os=macos`: the mock's app_info says platform "macos", no shield):
 * Cmd shortcuts, Cmd in tooltips and keycaps, the menu-bar wording, the shield row off with its
 * reason, and no caption buttons (the window keeps the native traffic lights).
 */
test.use({ timezoneId: 'UTC' });

const SIZES = [
  { width: 1280, height: 800 },
  { width: 960, height: 640 },
] as const;

const blur = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
const called = async (page: Page, cmd: string) => (await mockCalls(page)).filter((c) => c.cmd === cmd);

/** Open as macOS and wait until app_info has answered (the Dock tooltip turns to Cmd). */
async function openMac(page: Page) {
  await openApp(page, '?os=macos');
  await expect(page.locator('.dock .paste')).toHaveAttribute('title', 'Paste the clipboard and scrub it (Cmd+V)');
}

test.describe('macOS', () => {
  test('cmd_v_pastes_and_scrubs', async ({ page, problems: _ }) => {
    await openMac(page);
    await blur(page);
    await page.keyboard.press('Meta+v');
    await expect(page.getByTestId('verified')).toBeVisible();
    await expect(page.getByTestId('input-count')).toContainText(`${demoInput().length.toLocaleString('en')} chars`);
  });

  test('shift_cmd_c_copies_the_safe_text', async ({ page, problems: _ }) => {
    await openMac(page);
    await blur(page);
    await page.keyboard.press('Meta+v');
    await expect(page.getByTestId('verified')).toBeVisible();
    await page.keyboard.press('Meta+Shift+C');
    await expect(page.locator('.toast', { hasText: 'Safe text copied' })).toBeVisible();
    expect((await called(page, 'copy_text')).length).toBe(1);
  });

  test('ctrl_v_does_nothing', async ({ page, problems: _ }) => {
    await openMac(page);
    await blur(page);
    await page.keyboard.press('Control+v');
    // Give a paste that should not happen the time a real one takes.
    await page.waitForTimeout(400);
    expect(await called(page, 'read_clipboard')).toEqual([]);
    await expect(page.getByTestId('input-count')).toHaveText('0 chars');
  });

  test('tooltips_and_keycaps_say_cmd', async ({ page, problems: _ }) => {
    await openMac(page);
    await expect(page.locator('.dock .wipe')).toHaveAttribute('title', 'Clear both panes (Cmd+Shift+X)');
    await expect(page.getByTestId('copy-safe')).toHaveAttribute('title', 'Copy the safe text (Cmd+Shift+C)');
    await expect(page.getByTestId('copy-safe').locator('.kbd')).toHaveAttribute('aria-label', 'Cmd+Shift+C');
    await expect(page.getByRole('button', { name: 'Open a text file (Cmd+O)' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Paste (Cmd+V)' })).toBeVisible();
    await expect(page.getByTestId('empty-state').locator('.kbd').first()).toHaveAttribute('aria-label', 'Cmd+V');
    await expect(page.getByTestId('empty-state').locator('.kbd').nth(1)).toHaveAttribute('aria-label', 'Cmd+O');
    // Nothing in the window still says Ctrl.
    expect(await page.locator('body').innerText()).not.toContain('Ctrl');
    const titles = await page.locator('[title]').evaluateAll((els) => els.map((e) => e.getAttribute('title') ?? ''));
    expect(titles.filter((t) => t.includes('Ctrl'))).toEqual([]);
  });

  test('settings_speak_mac', async ({ page, problems: _ }) => {
    await openMac(page);
    await page.getByTestId('settings-open').click();
    const sheet = page.getByRole('dialog', { name: 'Settings' });
    await expect(sheet).toBeVisible();
    await expect(sheet).toContainText('Stored on this Mac only. Never any text.');
    await expect(sheet.getByRole('heading', { name: 'Hotkey and menu bar' })).toBeVisible();
    await expect(sheet).toContainText('Keep running in the menu bar');
    await expect(sheet).toContainText('Wipe when the Mac locks');
    await expect(sheet).toContainText('Follows macOS by default.');
    await expect(sheet).toContainText('Not available on macOS: since macOS 15, screen capture ignores it.');
    const shield = sheet.getByRole('switch', { name: 'Screen-capture shield' });
    await expect(shield).toBeDisabled();
    await expect(shield).toHaveAttribute('aria-checked', 'false');
    await expect(sheet).not.toContainText('tray');
  });

  test('no_caption_buttons', async ({ page, problems: _ }) => {
    await openMac(page);
    for (const name of ['Minimize', 'Maximize', 'Close']) await expect(page.getByRole('button', { name, exact: true })).toHaveCount(0);
    // Room for the traffic lights: the mark starts 78 px plus the bar's own padding from the left edge.
    const box = await page.locator('.titlebar .brand').boundingBox();
    expect(box?.x).toBeGreaterThanOrEqual(78);
  });

  test('right_click_opens_no_menu', async ({ page, problems: _ }) => {
    await openMac(page);
    const prevented = await page.evaluate(() => {
      const target = document.querySelector('[data-testid="input-editor"]') ?? document.body;
      const e = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
      target.dispatchEvent(e);
      return e.defaultPrevented;
    });
    expect(prevented).toBe(true);
  });

  // WebKit on a Mac does not focus a button that is clicked (CI round 1). `dispatchEvent('click')`
  // is such a click in any engine: the button gets the click, not the focus.
  test('a_sheet_opened_by_a_click_returns_focus_to_its_button', async ({ page, problems: _ }) => {
    await openMac(page);
    const opener = page.getByTestId('settings-open');
    await opener.dispatchEvent('click');
    const dialog = page.getByRole('dialog', { name: 'Settings' });
    await expect(dialog).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(dialog).toBeHidden();
    await expect(opener).toBeFocused();
  });

  test('hotkey_capture_listens_after_a_click_that_did_not_focus_it', async ({ page, problems: _ }) => {
    await openMac(page);
    await page.getByTestId('settings-open').click();
    const hotkey = page.getByTestId('hotkey');
    await hotkey.dispatchEvent('click');
    await page.keyboard.press('Control+Alt+K');
    await expect.poll(async () => (await lastSettings(page))?.hotkey).toBe('Ctrl+Alt+K');
  });

  for (const size of SIZES) {
    test(`macos_empty_dark_${size.width}`, async ({ page, problems: _ }) => {
      await page.setViewportSize(size);
      await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
      await openMac(page);
      await expect(page).toHaveScreenshot(`macos_empty_dark_${size.width}.png`);
    });
  }
});

test.describe('Linux', () => {
  test('keeps_ctrl_and_the_tray_wording', async ({ page, problems: _ }) => {
    await openApp(page, '?os=linux');
    await page.getByTestId('settings-open').click();
    const sheet = page.getByRole('dialog', { name: 'Settings' });
    await expect(sheet).toContainText('Not available on Linux: the desktop cannot hide a window from screenshots.');
    await expect(sheet.getByRole('heading', { name: 'Hotkey and tray' })).toBeVisible();
    await expect(page.locator('.dock .paste')).toHaveAttribute('title', 'Paste the clipboard and scrub it (Ctrl+V)');
    await expect(page.locator('.titlebar').getByRole('button', { name: 'Close', exact: true })).toHaveCount(1);
  });
});
