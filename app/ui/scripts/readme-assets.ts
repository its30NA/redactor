/**
 * Renders the pictures in `assets/` from the app's own UI, using the mock build and its
 * synthetic demo text. Nothing here touches a real desktop or real data.
 *
 *   npm run build:mock
 *   npx vite preview --mode mock &            # serves http://localhost:4179
 *   node scripts/readme-assets.ts <out-dir> [--gif]
 *
 * Writes hero-dark.png and hero-light.png (1280x800 at device scale 2), social-preview.png
 * (1280x640, GitHub's size) and, with --gif, demo.webm (a screen recording the GIF is made from).
 */
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium, type Page } from '@playwright/test';

const out = process.argv[2];
if (!out) throw new Error('usage: node scripts/readme-assets.ts <out-dir> [--gif]');
const withGif = process.argv.includes('--gif');
const BASE = process.env.REDACTOR_MOCK_URL ?? 'http://localhost:4179';
const UI = join(import.meta.dirname, '..');

async function open(page: Page, query = '') {
  await page.goto(`${BASE}/${query}`);
  await page.locator('.app[data-ready]').waitFor();
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
}

async function paste(page: Page) {
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.keyboard.press('Control+v');
}

async function settled(page: Page) {
  await page.getByTestId('verified').waitFor();
  await page.waitForFunction(() => document.querySelectorAll('.cm-chip-veil, .scramble.running').length === 0);
}

const browser = await chromium.launch();
try {
  for (const scheme of ['dark', 'light'] as const) {
    const ctx = await browser.newContext({
      viewport: { width: 1280, height: 800 },
      deviceScaleFactor: 2,
      colorScheme: scheme,
      reducedMotion: 'reduce',
      timezoneId: 'UTC',
    });
    const page = await ctx.newPage();
    await open(page);
    await paste(page);
    await settled(page);
    await page.screenshot({ path: join(out, `hero-${scheme}.png`) });
    await ctx.close();
  }

  // Social preview: the app icon, the name and the pitch, in the app's own fonts and colours.
  {
    const font = (file: string) =>
      readFileSync(join(UI, 'node_modules/@fontsource-variable/inter/files', file)).toString('base64');
    const icon = readFileSync(join(UI, '..', 'icon-source.png')).toString('base64');
    const ctx = await browser.newContext({ viewport: { width: 1280, height: 640 }, deviceScaleFactor: 1 });
    const page = await ctx.newPage();
    await page.setContent(`<!doctype html><html><head><meta charset="utf-8"><style>
      @font-face{font-family:Inter;font-weight:100 900;src:url(data:font/woff2;base64,${font('inter-latin-wght-normal.woff2')}) format('woff2')}
      html,body{margin:0;width:1280px;height:640px;background:#070A12;color:#E6EAF2;font-family:Inter,sans-serif;overflow:hidden}
      .bg{position:absolute;inset:0;background:
        radial-gradient(640px 420px at 16% 12%,rgba(34,211,238,.20),transparent 70%),
        radial-gradient(700px 460px at 92% 96%,rgba(139,92,246,.22),transparent 70%)}
      .row{position:absolute;inset:0;display:flex;align-items:center;gap:56px;padding:0 96px}
      img{width:232px;height:232px;flex:none}
      h1{margin:0;font-size:92px;font-weight:700;letter-spacing:-.03em;line-height:1}
      p{margin:22px 0 0;font-size:36px;line-height:1.3;color:#B7BFD1;max-width:760px}
      .tag{margin-top:34px;display:inline-block;padding:10px 20px;border:1px solid rgba(255,255,255,.16);border-radius:999px;font-size:24px;color:#8B93A7}
    </style></head><body><div class="bg"></div><div class="row">
      <img alt="" src="data:image/png;base64,${icon}">
      <div><h1>Redactor</h1><p>Strip secrets out of text before you paste it anywhere. Local only.</p>
      <span class="tag">Windows app &middot; scrub CLI &middot; Rust engine</span></div>
    </div></body></html>`);
    await page.evaluate(() => document.fonts.ready.then(() => undefined));
    await page.screenshot({ path: join(out, 'social-preview.png') });
    await ctx.close();
  }

  if (withGif) {
    const ctx = await browser.newContext({
      viewport: { width: 1280, height: 800 },
      deviceScaleFactor: 1,
      colorScheme: 'dark',
      timezoneId: 'UTC',
      recordVideo: { dir: out, size: { width: 1280, height: 800 } },
    });
    const page = await ctx.newPage();
    await open(page);
    await page.waitForTimeout(1500);
    await paste(page);
    await settled(page);
    await page.waitForTimeout(2200);
    await page.keyboard.press('Control+2');
    await page.waitForTimeout(800);
    await ctx.close();
  }
} finally {
  await browser.close();
}
