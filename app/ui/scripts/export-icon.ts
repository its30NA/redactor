/**
 * Renders `src/assets/mark.svg` to `app/icon-source.png` (1024×1024, transparent), the input
 * for `tauri icon`. Uses Playwright's Chromium so the PNG matches what the UI draws.
 * Run with `npm run icon:export`.
 *
 * `--preview <file.png>` instead writes a check sheet: the mark at 16, 20, 24, 32, 48 and
 * 256 px on a dark and a light taskbar colour, the sizes `tauri icon` produces for Windows.
 */
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium } from '@playwright/test';

const ROOT = join(import.meta.dirname, '..');
const svg = readFileSync(join(ROOT, 'src/assets/mark.svg'), 'utf8');
const dataUrl = `data:image/svg+xml;base64,${Buffer.from(svg).toString('base64')}`;

const previewAt = process.argv.indexOf('--preview');
const browser = await chromium.launch();
try {
  if (previewAt >= 0) {
    const out = process.argv[previewAt + 1] ?? 'mark-preview.png';
    const page = await browser.newPage({ viewport: { width: 860, height: 600 }, deviceScaleFactor: 1 });
    const sizes = [16, 20, 24, 32, 48, 256];
    // Small sizes are downscaled from the 1024 px PNG, the way the .ico frames are made.
    const row = (bg: string, ink: string, label: string) => `
      <div class="row" style="background:${bg};color:${ink}">
        <span class="lbl">${label}</span>
        ${sizes.map((s) => `<figure><canvas data-s="${s}" width="${s}" height="${s}"></canvas><figcaption>${s}</figcaption></figure>`).join('')}
      </div>`;
    await page.setContent(`<!doctype html><html><head><style>
      body{margin:0;font:12px system-ui;background:#888}
      .row{display:flex;align-items:flex-end;gap:22px;padding:18px 22px;height:300px;box-sizing:border-box}
      .lbl{width:70px;align-self:center;opacity:.7}
      figure{margin:0;display:flex;flex-direction:column;align-items:center;gap:6px}
      canvas{image-rendering:auto}
      figcaption{opacity:.6}
    </style></head><body>
      ${row('#1c1c1c', '#ddd', 'dark')}${row('#f3f3f3', '#333', 'light')}
    </body></html>`);
    await page.evaluate(async (src) => {
      const big = new Image();
      big.src = src;
      await big.decode();
      const master = document.createElement('canvas');
      master.width = master.height = 1024;
      master.getContext('2d')?.drawImage(big, 0, 0, 1024, 1024);
      for (const c of document.querySelectorAll('canvas[data-s]')) {
        const s = Number((c as HTMLCanvasElement).dataset.s);
        const ctx = (c as HTMLCanvasElement).getContext('2d');
        if (!ctx) continue;
        ctx.imageSmoothingQuality = 'high';
        ctx.drawImage(master, 0, 0, s, s);
      }
    }, dataUrl);
    await page.screenshot({ path: out });
    console.log(`wrote ${out}`);
  } else {
    const page = await browser.newPage({ viewport: { width: 1024, height: 1024 }, deviceScaleFactor: 1 });
    await page.setContent(
      `<!doctype html><html><body style="margin:0;background:transparent"><img src="${dataUrl}" width="1024" height="1024" style="display:block"></body></html>`,
    );
    await page.locator('img').evaluate((img: HTMLImageElement) => img.decode());
    const out = join(ROOT, '../icon-source.png');
    await page.screenshot({ path: out, omitBackground: true, clip: { x: 0, y: 0, width: 1024, height: 1024 } });
    console.log(`wrote ${out}`);
  }
} finally {
  await browser.close();
}
