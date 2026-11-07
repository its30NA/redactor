import { defineConfig, devices } from '@playwright/test';

/**
 * UI tests run against the mock build served with the shell's CSP (vite preview --mode mock),
 * so a CSP violation shows up as a test failure, not first inside the Tauri window.
 */
const PORT = 4179;

export default defineConfig({
  testDir: 'tests/e2e',
  snapshotPathTemplate: '{testDir}/__screenshots__/{arg}{ext}',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? 'github' : 'list',
  expect: {
    toHaveScreenshot: { maxDiffPixelRatio: 0.01, animations: 'disabled', caret: 'hide' },
  },
  use: {
    ...devices['Desktop Chrome'],
    baseURL: `http://localhost:${PORT}`,
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 1,
    colorScheme: 'dark',
    trace: 'retain-on-failure',
  },
  projects: [
    { name: 'chromium', use: { browserName: 'chromium' }, testIgnore: /perf\.spec\.ts/ },
    // Timing checks run alone, after everything else, so parallel workers do not skew them.
    { name: 'perf', use: { browserName: 'chromium' }, testMatch: /perf\.spec\.ts/, dependencies: ['chromium'] },
  ],
  webServer: {
    command: 'npm run build:mock && npm run preview:mock',
    url: `http://localhost:${PORT}`,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
