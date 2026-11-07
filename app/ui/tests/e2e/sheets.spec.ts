import type { Page } from '@playwright/test';
import { expect, lastSettings, mockCalls, openApp, test } from './fixtures';

const called = async (page: Page, cmd: string) => (await mockCalls(page)).filter((c) => c.cmd === cmd);
const sheet = (page: Page) => page.getByRole('dialog', { name: 'Remembered' });

/** Password-shaped single token, assembled here: the repo takes no secret-shaped literals. */
const PASSWORD = ['Hunter', '2', '!', 'Pass', '9'].join('');

async function openRemembered(page: Page) {
  await page.getByTestId('remembered-open').click();
  await expect(sheet(page)).toBeVisible();
}

test.describe('Remembered sheet', () => {
  test('lists session secrets by label only', async ({ page, problems: _ }) => {
    await openApp(page);
    await expect(page.getByTestId('remembered-open')).toHaveAccessibleName(/2 this session/);
    await openRemembered(page);
    const rows = page.getByTestId('session-secret');
    await expect(rows).toHaveCount(2);
    await expect(rows.first()).toContainText('Staging DB');
    await expect(rows.first().getByRole('img', { name: 'hidden value' })).toBeVisible();
    await expect(page.getByTestId('session-section')).not.toContainText(/chars/);
    await expect(page.getByTestId('session-section')).toContainText('Only the label is shown');
    await expect(page.getByTestId('session-section')).toContainText('never written to disk');
    await expect(page.getByTestId('session-section')).toContainText('when Windows locks');
  });

  test('forget one, then forget all', async ({ page, problems: _ }) => {
    await openApp(page);
    await openRemembered(page);
    await page.getByRole('button', { name: 'Forget Staging DB' }).click();
    await expect(page.getByTestId('session-secret')).toHaveCount(1);
    expect((await called(page, 'session_forget')).at(-1)?.args).toEqual({ id: 's1' });
    await page.getByRole('button', { name: 'Forget all' }).click();
    await expect(page.getByTestId('session-secret')).toHaveCount(0);
    await expect(page.getByTestId('session-section')).toContainText('Nothing remembered yet');
    expect((await called(page, 'session_clear')).length).toBe(1);
  });

  test('add a secret through the masked field', async ({ page, problems: _ }) => {
    await openApp(page, '?session=empty');
    await openRemembered(page);
    await page.getByTestId('add-secret-open').click();
    const dialog = page.getByTestId('add-secret');
    await expect(dialog).toBeVisible();
    const field = page.getByTestId('secret-input');
    await expect(field).toHaveAttribute('type', 'password');
    await page.getByLabel('Label', { exact: true }).fill('Wi-Fi');
    await field.fill('abcde');
    await expect(dialog.getByRole('button', { name: 'Remember' })).toBeDisabled();
    await expect(dialog).toContainText('At least 6 characters (5 so far)');
    await field.fill('abcdef-ghij');
    await dialog.getByRole('button', { name: 'Show the secret' }).click();
    await expect(field).toHaveAttribute('type', 'text');
    await dialog.getByRole('button', { name: 'Remember' }).click();
    await expect(dialog).toBeHidden();
    await expect(page.getByTestId('session-secret')).toHaveCount(1);
    await expect(page.getByTestId('session-secret')).not.toContainText(/chars/);
    const add = (await called(page, 'session_add')).at(-1);
    expect(add?.args.label).toBe('Wi-Fi');
    expect(add?.args.secret).toBe('•'.repeat(11)); // the mock logs it masked
    // Reopened, the field is empty again.
    await page.getByTestId('add-secret-open').click();
    await expect(page.getByTestId('secret-input')).toHaveValue('');
  });

  test('esc closes the dialog first, then the sheet', async ({ page, problems: _ }) => {
    await openApp(page);
    await openRemembered(page);
    await page.getByTestId('add-secret-open').click();
    await expect(page.getByTestId('add-secret')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('add-secret')).toBeHidden();
    await expect(sheet(page)).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(sheet(page)).toBeHidden();
  });

  test('always redact: add a word, refuse a password, remove a chip', async ({ page, problems: _ }) => {
    await openApp(page);
    await openRemembered(page);
    const section = page.getByTestId('always-section');
    await expect(section).toContainText('Not for passwords');
    await expect(section).toContainText('plain text');
    await expect(page.getByTestId('always-chip')).toHaveCount(2);

    const input = page.getByTestId('always-input');
    await input.fill('Kestrel Labs');
    await input.press('Enter');
    await expect(page.getByTestId('always-chip')).toHaveCount(3);
    expect((await lastSettings(page))?.alwaysRedact).toEqual(['Nightjar', 'build-07.corp.example', 'Kestrel Labs']);

    await input.fill('Müller GmbH & Co. KG');
    await input.press('Enter');
    await expect(page.getByTestId('always-chip')).toHaveCount(4);
    await expect(page.getByTestId('password-warning')).toHaveCount(0);
    await page.getByRole('button', { name: 'Remove Müller GmbH & Co. KG' }).click();
    await expect(page.getByTestId('always-chip')).toHaveCount(3);

    await input.fill('ab');
    await input.press('Enter');
    await expect(section.getByRole('alert')).toContainText('At least 3 characters');

    await page.getByRole('button', { name: 'Remove Nightjar' }).click();
    await expect(page.getByTestId('always-chip')).toHaveCount(2);
    expect((await lastSettings(page))?.alwaysRedact).toEqual(['build-07.corp.example', 'Kestrel Labs']);
  });

  test('a password-shaped entry: "Keep for this session" never touches the settings', async ({ page, problems: _ }) => {
    await openApp(page);
    await openRemembered(page);
    const input = page.getByTestId('always-input');
    const before = (await called(page, 'set_settings')).length;
    await input.fill(PASSWORD);
    await input.press('Enter');
    const warning = page.getByTestId('password-warning');
    await expect(warning).toBeVisible();
    await expect(warning).toContainText('stored on this PC in plain text');
    await expect(page.getByTestId('keep-session')).toBeVisible();
    await expect(page.getByTestId('add-anyway')).toBeVisible();
    expect((await called(page, 'set_settings')).length).toBe(before);
    expect(await called(page, 'session_add')).toHaveLength(0);

    await page.getByTestId('keep-session').click();
    await expect(warning).toBeHidden();
    await expect(input).toHaveValue('');
    await expect(page.getByTestId('session-secret')).toHaveCount(3);
    await expect(page.getByTestId('session-secret').last()).toContainText('Typed secret');
    const add = await called(page, 'session_add');
    expect(add).toHaveLength(1);
    expect(add[0]?.args.label).toBe('Typed secret');
    expect(add[0]?.args.secret).toBe('•'.repeat(PASSWORD.length)); // the mock logs it masked
    expect((await called(page, 'set_settings')).length).toBe(before);
    expect(JSON.stringify(await mockCalls(page))).not.toContain(PASSWORD);
    await expect(page.getByTestId('always-chip')).toHaveCount(2);
  });

  test('a password-shaped entry: "Add anyway" puts it on the list', async ({ page, problems: _ }) => {
    await openApp(page);
    await openRemembered(page);
    const input = page.getByTestId('always-input');
    await input.fill(PASSWORD);
    await input.press('Enter');
    await expect(page.getByTestId('password-warning')).toBeVisible();
    await page.getByTestId('add-anyway').click();
    await expect(page.getByTestId('password-warning')).toBeHidden();
    await expect(input).toHaveValue('');
    await expect(page.getByTestId('always-chip')).toHaveCount(3);
    expect((await lastSettings(page))?.alwaysRedact).toEqual(['Nightjar', 'build-07.corp.example', PASSWORD]);
    expect(await called(page, 'session_add')).toHaveLength(0);
    await expect(page.getByTestId('session-secret')).toHaveCount(2);
  });

  test('editing the field dismisses the password warning', async ({ page, problems: _ }) => {
    await openApp(page);
    await openRemembered(page);
    const input = page.getByTestId('always-input');
    await input.fill(PASSWORD);
    await input.press('Enter');
    await expect(page.getByTestId('password-warning')).toBeVisible();
    await input.fill('Kestrel');
    await expect(page.getByTestId('password-warning')).toBeHidden();
  });

  test('session-changed and wipe events update the list', async ({ page, problems: _ }) => {
    await openApp(page);
    await openRemembered(page);
    await page.evaluate(() =>
      window.__REDACTOR_MOCK__?.emit('session-changed', [{ id: 'x', label: 'From the tray', addedAt: new Date().toISOString() }]),
    );
    await expect(page.getByTestId('session-secret')).toHaveCount(1);
    await expect(page.getByTestId('session-secret')).toContainText('From the tray');
    await page.evaluate(() => window.__REDACTOR_MOCK__?.emit('wipe', { reason: 'idle' }));
    await expect(page.getByTestId('session-secret')).toHaveCount(0);
  });
});

test.describe('settings-changed event', () => {
  test('tray shield toggle reaches the status pill', async ({ page, problems: _ }) => {
    await openApp(page);
    await expect(page.getByTestId('status-pill')).toContainText('Shield on');
    await page.evaluate(() =>
      window.__REDACTOR_MOCK__?.emit('settings-changed', {
        theme: 'system',
        sensitivity: 'balanced',
        pii: false,
        stripInvisible: true,
        hotkey: 'Ctrl+Alt+R',
        trayMode: true,
        autoWipeMinutes: 10,
        wipeOnLock: true,
        contentProtection: false,
        placeholderStyle: 'labeled',
        reducedMotion: 'system',
        alwaysRedact: [],
        firstRunDone: true,
      }),
    );
    await expect(page.getByTestId('status-pill')).toContainText('Shield off');
  });
});

test.describe('wipe event', () => {
  test('wipe_event_clears_both_panes', async ({ page, problems: _ }) => {
    await openApp(page);
    await page.getByRole('button', { name: /Paste & scrub/ }).click();
    await expect(page.getByTestId('verified')).toBeVisible();
    await page.evaluate(() => window.__REDACTOR_MOCK__?.emit('wipe', { reason: 'lock' }));
    await expect(page.getByTestId('input-count')).toHaveText('0 chars');
    await expect(page.getByTestId('output-editor').locator('.cm-line').first()).toHaveText('');
    await expect(page.getByTestId('output-editor').locator('.cm-chip')).toHaveCount(0);
    await expect(page.locator('.toast', { hasText: 'Windows was locked' })).toBeVisible();
    await expect(page.getByTestId('remembered-open')).toHaveAccessibleName(/0 this session/);
  });

  test('sleep wipes too and says why', async ({ page, problems: _ }) => {
    await openApp(page);
    await page.getByRole('button', { name: /Paste & scrub/ }).click();
    await expect(page.getByTestId('verified')).toBeVisible();
    await page.evaluate(() => window.__REDACTOR_MOCK__?.emit('wipe', { reason: 'sleep' }));
    await expect(page.getByTestId('input-count')).toHaveText('0 chars');
    await expect(page.locator('.toast', { hasText: 'Windows went to sleep. Both panes are empty.' })).toBeVisible();
  });
});

test.describe('Settings sheet', () => {
  test('hotkey capture refuses reserved keys and saves a good one', async ({ page, problems: _ }) => {
    await openApp(page);
    await page.getByTestId('settings-open').click();
    const hotkey = page.getByTestId('hotkey');
    await hotkey.click();
    await page.keyboard.press('Control+Alt+A');
    await expect(page.getByRole('alert')).toContainText('This shortcut is reserved');
    await page.keyboard.press('Control+Alt+K');
    await expect.poll(async () => (await lastSettings(page))?.hotkey).toBe('Ctrl+Alt+K');
    await expect(hotkey).toContainText('K');
    // Esc while capturing cancels the capture, not the sheet.
    await hotkey.click();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog', { name: 'Settings' })).toBeVisible();
  });

  test('switches and choices save', async ({ page, problems: _ }) => {
    await openApp(page);
    await page.getByTestId('settings-open').click();
    const dialog = page.getByRole('dialog', { name: 'Settings' });
    await dialog.getByRole('switch', { name: 'Personal data' }).click();
    await expect.poll(async () => (await lastSettings(page))?.pii).toBe(true);
    await dialog.getByRole('switch', { name: 'Wipe when Windows locks' }).click();
    await expect.poll(async () => (await lastSettings(page))?.wipeOnLock).toBe(false);
    await dialog.getByRole('radio', { name: '30' }).click();
    await expect.poll(async () => (await lastSettings(page))?.autoWipeMinutes).toBe(30);
    await dialog.getByRole('radio', { name: 'Generic' }).click();
    await expect.poll(async () => (await lastSettings(page))?.placeholderStyle).toBe('generic');
    await dialog.getByRole('switch', { name: 'Screen-capture shield' }).click();
    await expect(page.getByTestId('status-pill')).toContainText('Shield off');
  });
});

test.describe('first run', () => {
  test('shows once and is remembered', async ({ page, problems: _ }) => {
    await openApp(page, '?firstrun');
    const card = page.getByTestId('first-run');
    await expect(card).toBeVisible();
    await expect(card).toContainText('Offline by design');
    await expect(card).toContainText('Ctrl');
    await expect(card).toContainText('Shielded from screen capture');
    await card.getByRole('button', { name: 'Get started' }).click();
    await expect(card).toBeHidden();
    await expect.poll(async () => (await lastSettings(page))?.firstRunDone).toBe(true);
  });

  test('not shown once done', async ({ page, problems: _ }) => {
    await openApp(page);
    await page.waitForTimeout(300);
    await expect(page.getByTestId('first-run')).toHaveCount(0);
  });
});
