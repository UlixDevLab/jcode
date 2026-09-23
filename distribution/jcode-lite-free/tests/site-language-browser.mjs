// Run with an existing Playwright installation. No browser or package installation.
// PLAYWRIGHT_MODULE=/path/to/playwright SITE_BROWSER_CHANNEL=chrome node this-file.mjs URL [staged-html]
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const [url, html] = process.argv.slice(2);
assert.ok(url, 'Expected page URL');
const browser = await chromium.launch({ headless: true, ...(process.env.SITE_BROWSER_CHANNEL ? { channel: process.env.SITE_BROWSER_CHANNEL } : {}) });
const results = [];
try {
  for (const width of [390, 1280]) {
    const page = await browser.newPage({ viewport: { width, height: 900 } });
    if (html) await page.route('**/*', route => route.request().isNavigationRequest() && route.request().frame() === page.mainFrame()
      ? route.fulfill({ contentType: 'text/html', body: fs.readFileSync(html, 'utf8') }) : route.continue());
    for (const edition of ['lite', 'free']) for (const language of ['uk', 'en']) {
      await page.goto(`${url}?edition=${edition}&lang=${language}`, { waitUntil: 'domcontentloaded', timeout: 30000 });
      async function check(expectedEdition, expectedLanguage) {
        const views = page.locator('[data-edition][data-language]:visible');
        assert.equal(await views.count(), 1, 'Exactly one translated edition must be visible');
        assert.equal(await views.getAttribute('data-edition'), expectedEdition);
        assert.equal(await views.getAttribute('data-language'), expectedLanguage);
        assert.equal(await page.locator('html').getAttribute('lang'), expectedLanguage);
        assert.equal(await page.locator(`[data-edition-button="${expectedEdition}"]`).getAttribute('aria-pressed'), 'true');
        assert.equal(await page.locator(`[data-language-button="${expectedLanguage}"]`).getAttribute('aria-pressed'), 'true');
        assert.ok((await views.locator('h1').innerText()).trim().length > 10);
        assert.equal(new URL(page.url()).searchParams.get('edition'), expectedEdition);
        assert.equal(new URL(page.url()).searchParams.get('lang'), expectedLanguage);
      }
      await check(edition, language);
      for (const lang of ['en', 'uk', 'en', 'uk']) {
        await page.locator(`[data-language-button="${lang}"]`).click();
        await check(edition, lang);
      }
      for (const next of ['free', 'lite', 'free']) {
        await page.locator(`[data-edition-button="${next}"]`).click();
        await check(next, 'uk');
      }
      await page.reload({ waitUntil: 'domcontentloaded' });
      await check('free', 'uk');
      results.push({ width, initialEdition: edition, initialLanguage: language, togglesAndReload: 'PASS' });
    }
    await page.close();
  }
  console.log(JSON.stringify({ url, mode: html ? 'exact staged HTML with live assets' : 'live website', results }, null, 2));
} finally { await browser.close(); }
