import { expect, lastSettings, mockCalls, openApp, test } from './fixtures';

test.describe('hardening', () => {
  test('csp_header_is_served_and_enforced', async ({ page, problems }) => {
    const res = await page.goto('/');
    expect(res?.headers()['content-security-policy']).toContain("style-src 'self'");
    await page.locator('.app[data-ready]').waitFor();
    // Negative control: an inline <style> must be refused, proving the guard is live.
    await page.evaluate(() => {
      const s = document.createElement('style');
      s.textContent = 'body { outline: 1px solid red; }';
      document.head.append(s);
    });
    await expect.poll(() => problems.some((p) => p.includes('CSP violation'))).toBe(true);
    problems.length = 0;
  });

  test('title_bar_is_a_drag_region_and_buttons_are_not', async ({ page, problems: _ }) => {
    await openApp(page);
    await expect(page.locator('header.titlebar')).toHaveAttribute('data-tauri-drag-region', '');
    const draggableButtons = page.locator('header.titlebar button[data-tauri-drag-region]');
    await expect(draggableButtons).toHaveCount(0);
  });

  test('window_controls_call_the_window_api', async ({ page, problems: _ }) => {
    await openApp(page);
    await page.getByRole('button', { name: 'Minimize' }).click();
    await page.getByRole('button', { name: 'Maximize' }).click();
    await expect(page.getByRole('button', { name: 'Restore' })).toBeVisible();
    await page.getByRole('button', { name: 'Close' }).click();
    const cmds = (await mockCalls(page)).map((c) => c.cmd).filter((c) => c.startsWith('plugin:window|'));
    expect(cmds).toEqual(
      expect.arrayContaining(['plugin:window|minimize', 'plugin:window|toggle_maximize', 'plugin:window|close']),
    );
  });
});

test.describe('controls', () => {
  test('theme_override_cycles_and_saves', async ({ page, problems: _ }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openApp(page);
    const html = page.locator('html');
    const bg = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    const toggle = page.getByTestId('theme-toggle');

    await expect(html).not.toHaveAttribute('data-theme');
    expect(await bg()).toBe('rgb(7, 10, 18)');

    await toggle.click();
    await expect(html).toHaveAttribute('data-theme', 'dark');
    expect((await lastSettings(page))?.theme).toBe('dark');

    await toggle.click();
    await expect(html).toHaveAttribute('data-theme', 'light');
    expect((await lastSettings(page))?.theme).toBe('light');
    expect(await bg()).toBe('rgb(246, 247, 251)');

    await toggle.click();
    await expect(html).not.toHaveAttribute('data-theme');
    expect((await lastSettings(page))?.theme).toBe('system');
  });

  test('sensitivity_by_click_and_arrow_keys', async ({ page, problems: _ }) => {
    await openApp(page);
    const group = page.getByRole('radiogroup', { name: 'Sensitivity' });
    await expect(group.getByRole('radio', { name: 'Balanced' })).toHaveAttribute('aria-checked', 'true');

    await group.getByRole('radio', { name: 'Paranoid' }).click();
    await expect(group.getByRole('radio', { name: 'Paranoid' })).toHaveAttribute('aria-checked', 'true');
    expect((await lastSettings(page))?.sensitivity).toBe('paranoid');

    await page.keyboard.press('ArrowLeft');
    await expect(group.getByRole('radio', { name: 'Balanced' })).toHaveAttribute('aria-checked', 'true');
    await expect(group.getByRole('radio', { name: 'Balanced' })).toBeFocused();
    await page.keyboard.press('Home');
    await expect(group.getByRole('radio', { name: 'Relaxed' })).toHaveAttribute('aria-checked', 'true');
    expect((await lastSettings(page))?.sensitivity).toBe('relaxed');
  });

  test('pii_switch', async ({ page, problems: _ }) => {
    await openApp(page);
    const pii = page.getByRole('switch', { name: 'PII' });
    await expect(pii).toHaveAttribute('aria-checked', 'false');
    await pii.click();
    await expect(pii).toHaveAttribute('aria-checked', 'true');
    expect((await lastSettings(page))?.pii).toBe(true);
    await pii.press('Space');
    await expect(pii).toHaveAttribute('aria-checked', 'false');
  });

  test('settings_sheet_traps_focus_and_closes_on_escape', async ({ page, problems: _ }) => {
    await openApp(page);
    const opener = page.getByTestId('settings-open');
    await opener.click();
    const dialog = page.getByRole('dialog', { name: 'Settings' });
    await expect(dialog).toBeVisible();
    await expect.poll(() => dialog.evaluate((d) => d.contains(document.activeElement))).toBe(true);

    for (let i = 0; i < 12; i++) await page.keyboard.press('Tab');
    expect(await dialog.evaluate((d) => d.contains(document.activeElement))).toBe(true);

    await page.keyboard.press('Escape');
    await expect(dialog).toBeHidden();
    await expect(opener).toBeFocused();

    await opener.click();
    await expect(dialog).toBeVisible();
    await page.mouse.click(200, 400); // the scrim
    await expect(dialog).toBeHidden();
  });

  test('settings_sheet_motion_override', async ({ page, problems: _ }) => {
    await openApp(page);
    await page.getByTestId('settings-open').click();
    await page.getByRole('radiogroup', { name: 'Motion' }).getByRole('radio', { name: 'Reduced' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-motion', 'reduce');
    expect((await lastSettings(page))?.reducedMotion).toBe('on');
  });

  test('toast_shows_and_dismisses', async ({ page, problems: _ }) => {
    await openApp(page);
    await page.getByTestId('copy-safe').click();
    const toast = page.locator('.toast', { hasText: 'Nothing to copy yet' });
    await expect(toast).toBeVisible();
    await toast.getByRole('button', { name: 'Dismiss' }).click();
    await expect(toast).toBeHidden();
  });

  test('paste_reads_the_clipboard_through_the_api', async ({ page, problems: _ }) => {
    await openApp(page);
    await page.getByRole('button', { name: /Paste & scrub/ }).click();
    await expect(page.getByTestId('input-count')).not.toHaveText('0 chars');
    expect((await mockCalls(page)).some((c) => c.cmd === 'read_clipboard')).toBe(true);
  });

  test('empty_clipboard_says_so', async ({ page, problems: _ }) => {
    await openApp(page, '?clipboard=empty');
    await page.getByRole('button', { name: /Paste & scrub/ }).click();
    await expect(page.locator('.toast', { hasText: 'Nothing to paste' })).toBeVisible();
    await expect(page.getByTestId('empty-state')).toBeVisible();
  });
});

test.describe('layout', () => {
  for (const size of [
    { width: 960, height: 640 },
    { width: 1280, height: 800 },
  ]) {
    test(`fits_without_overflow_${size.width}`, async ({ page, problems: _ }) => {
      await page.setViewportSize(size);
      await openApp(page);
      const overflow = await page.evaluate(() => ({
        x: document.documentElement.scrollWidth - innerWidth,
        y: document.documentElement.scrollHeight - innerHeight,
      }));
      expect(overflow).toEqual({ x: 0, y: 0 });
      for (const el of await page.locator('.dock button, header.titlebar button').all()) {
        const box = await el.boundingBox();
        expect(box, 'control rendered').not.toBeNull();
        if (!box) continue;
        expect(box.x).toBeGreaterThanOrEqual(0);
        expect(box.x + box.width).toBeLessThanOrEqual(size.width);
        expect(box.y + box.height).toBeLessThanOrEqual(size.height);
      }
    });
  }

  test('findings_rail_collapses_on_narrow_windows_and_on_demand', async ({ page, problems: _ }) => {
    await page.setViewportSize({ width: 960, height: 640 });
    await openApp(page);
    await expect(page.getByRole('button', { name: 'Show findings' })).toBeVisible();
    await page.getByRole('button', { name: 'Show findings' }).click();
    await expect(page.getByRole('heading', { name: 'Findings' })).toBeVisible();

    await page.setViewportSize({ width: 1280, height: 800 });
    await page.getByRole('button', { name: 'Hide findings' }).click();
    await expect(page.getByRole('button', { name: 'Show findings' })).toBeVisible();
  });
});

test.describe('motion', () => {
  const running = (page: import('@playwright/test').Page) =>
    page.evaluate(() => document.getAnimations().filter((a) => a.playState === 'running').length);

  test('reduced_motion_stops_every_animation', async ({ page, problems: _ }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await openApp(page);
    await page.getByTestId('copy-safe').click(); // a toast enters too
    expect(await page.evaluate(() => document.getAnimations().length)).toBe(0);
  });

  test('full_motion_runs_the_aurora_and_pauses_it_when_inactive', async ({ page, problems: _ }) => {
    await page.emulateMedia({ reducedMotion: 'no-preference' });
    await openApp(page);
    await page.evaluate(() => (document.documentElement.dataset.window = 'active'));
    expect(await running(page)).toBeGreaterThanOrEqual(4); // three aurora blobs and the status pulse
    await page.evaluate(() => window.dispatchEvent(new Event('blur')));
    await expect(page.locator('html')).toHaveAttribute('data-window', 'inactive');
    await expect.poll(() => running(page)).toBe(0);
  });

  test('ambient_motion_only_animates_transform_and_opacity', async ({ page, problems: _ }) => {
    await page.emulateMedia({ reducedMotion: 'no-preference' });
    await openApp(page);
    await page.evaluate(() => (document.documentElement.dataset.window = 'active'));
    const props = await page.evaluate(() =>
      document
        .getAnimations()
        .filter((a) => a.effect instanceof KeyframeEffect && a.effect.getTiming().iterations === Infinity)
        .flatMap((a) =>
          (a.effect as KeyframeEffect)
            .getKeyframes()
            .flatMap((k) => Object.keys(k).filter((p) => !['offset', 'easing', 'composite', 'computedOffset'].includes(p))),
        ),
    );
    expect(props.length).toBeGreaterThan(0);
    expect(new Set(props)).toEqual(new Set(['transform', 'opacity']));
  });

  test('idle_main_thread_is_calm', async ({ page, problems: _ }) => {
    await page.emulateMedia({ reducedMotion: 'no-preference' });
    await openApp(page);
    await page.evaluate(() => (document.documentElement.dataset.window = 'active'));
    await page.waitForTimeout(800); // let the arrival animation finish
    const cdp = await page.context().newCDPSession(page);
    await cdp.send('Performance.enable');
    const busy = async () => {
      const { metrics } = await cdp.send('Performance.getMetrics');
      return metrics.find((m) => m.name === 'TaskDuration')?.value ?? 0;
    };
    const before = await busy();
    await page.waitForTimeout(3000);
    const spent = (await busy()) - before;
    test.info().annotations.push({ type: 'idle main thread', description: `${Math.round(spent * 1000)} ms in 3 s` });
    // The aurora runs on the compositor; the main thread should be nearly idle (< 5 %).
    expect(spent).toBeLessThan(0.15);
  });
});
