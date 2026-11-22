// Rendert HTML-Dateien zu PNG mit Playwright (Chromium). Playwright ist keine Abhängigkeit des Repos:
// `PLAYWRIGHT_DIR` zeigt auf einen Ordner, in dem `npm i playwright` gelaufen ist (siehe README).
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';

export async function renderAll(jobs) {
  const dir = process.env.PLAYWRIGHT_DIR;
  if (!dir) throw new Error('PLAYWRIGHT_DIR fehlt (Ordner mit installiertem playwright)');
  const { chromium } = createRequire(dir.replace(/\/?$/, '/'))('playwright');
  const browser = await chromium.launch();
  try {
    for (const j of jobs) {
      const ctx = await browser.newContext({ viewport: { width: j.w, height: j.h ?? 100 }, deviceScaleFactor: j.scale ?? 1 });
      const page = await ctx.newPage();
      await page.goto(pathToFileURL(j.html).href);
      await page.evaluate(() => document.fonts.ready);
      await page.screenshot({ path: j.png, fullPage: !j.h, ...(j.clip ? { clip: j.clip } : {}) });
      await ctx.close();
      console.log('png', j.png);
    }
  } finally { await browser.close(); }
}
