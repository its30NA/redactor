import { expect, openApp, test } from './fixtures';

/**
 * Runs in its own Playwright project after every other test (playwright.config.ts), so other
 * workers do not compete for the CPU while it measures main-thread tasks.
 */
test.describe('size', () => {
  test('five_mib_paste_stays_responsive', async ({ page, browser, problems: _ }) => {
    test.setTimeout(120_000);
    await openApp(page, '?clipboard=large');
    await browser.startTracing(page, { categories: ['toplevel', 'devtools.timeline', 'disabled-by-default-devtools.timeline'] });
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await page.keyboard.press('Control+v');
    await expect(page.getByTestId('verified')).toBeVisible({ timeout: 60_000 });
    await page.waitForTimeout(1600); // the arrival moment settles
    const trace = JSON.parse((await browser.stopTracing()).toString('utf8')) as {
      traceEvents: { name: string; ph: string; dur?: number; tid: number; pid: number; args?: { name?: string } }[];
    };
    const ev = trace.traceEvents;
    // Renderer main threads ("CrRendererMain"); the longest task on any of them must stay short.
    const mains = new Set(
      ev.filter((e) => e.ph === 'M' && e.name === 'thread_name' && e.args?.name === 'CrRendererMain').map((e) => `${e.pid}:${e.tid}`),
    );
    const tasks = ev.filter((e) => e.name === 'RunTask' && typeof e.dur === 'number' && mains.has(`${e.pid}:${e.tid}`));
    expect(mains.size, 'renderer main thread in the trace').toBeGreaterThan(0);
    expect(tasks.length, 'main-thread tasks in the trace').toBeGreaterThan(100);
    const longest = Math.max(...tasks.map((e) => (e.dur ?? 0) / 1000));
    expect(longest, `longest main-thread task ${longest.toFixed(0)} ms`).toBeLessThan(200);
    await expect(page.getByTestId('input-count')).toContainText('5,');
  });
});
