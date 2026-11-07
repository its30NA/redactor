import { expect, test as base, type Page } from '@playwright/test';
import type { MockCall } from '../../src/lib/mock/ipc';

/** Collects console errors, page errors and CSP violations; every test asserts there are none. */
export const test = base.extend<{ problems: string[] }>({
  problems: async ({ page }, use) => {
    const problems: string[] = [];
    page.on('console', (msg) => {
      if (msg.type() === 'error' || /Content Security Policy/i.test(msg.text())) problems.push(msg.text());
    });
    page.on('pageerror', (err) => problems.push(String(err)));
    await page.addInitScript(() => {
      document.addEventListener('securitypolicyviolation', (e) => {
        console.error(`CSP violation: ${e.violatedDirective} ${e.blockedURI}`);
      });
    });
    await use(problems);
    expect(problems, 'console errors / CSP violations').toEqual([]);
  },
});

export { expect };

/** Open the app and wait until settings are loaded and fonts are ready. */
export async function openApp(page: Page, query = ''): Promise<void> {
  await page.goto(`/${query}`);
  await page.locator('.app[data-ready]').waitFor();
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
}

export async function mockCalls(page: Page): Promise<MockCall[]> {
  return page.evaluate(() => window.__REDACTOR_MOCK__?.calls ?? []);
}

export async function lastSettings(page: Page): Promise<Record<string, unknown> | undefined> {
  const calls = await mockCalls(page);
  const set = calls.filter((c) => c.cmd === 'set_settings').at(-1);
  return set?.args.settings as Record<string, unknown> | undefined;
}
