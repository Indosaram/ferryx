// Real-surface QA for consent-gated analytics. Serves an isolated build, drives Chromium at
// desktop and mobile viewports, and inspects both the dataLayer and the actual GA4 network
// payloads (batched hits included, which is where gtag.js can re-derive the raw page URL).
//
//   bun scripts/analytics-consent-qa.mjs <distDir> <evidenceDir>
//
// QA_CHROMIUM may point at a Playwright Chromium binary when the default download is absent.
import path from 'node:path';
import { existsSync, statSync, mkdirSync } from 'node:fs';
import { chromium, devices } from '../../ui/node_modules/playwright/index.mjs';

const DIST = process.argv[2] ?? '/tmp/ferryx-ga-on';
const OUT = process.argv[3] ?? '/tmp/ferryx-ga-qa';
mkdirSync(OUT, { recursive: true });

const server = Bun.serve({
  port: 0,
  async fetch(request) {
    const url = new URL(request.url);
    let file = path.join(DIST, decodeURIComponent(url.pathname));
    if (existsSync(file) && statSync(file).isDirectory()) file = path.join(file, 'index.html');
    if (!existsSync(file)) return new Response('not found', { status: 404 });
    return new Response(Bun.file(file));
  },
});
const origin = `http://localhost:${server.port}`;
const LEAK = 'SECRET123';
const ENTRY = `${origin}/?utm_source=qa&utm_medium=mail&secret=${LEAK}`;

const browser = await chromium.launch({ executablePath: process.env.QA_CHROMIUM });
const results = [];

const readEvents = (page) =>
  page.evaluate(() =>
    [...(window.dataLayer ?? [])]
      .map((entry) => (typeof entry?.length === 'number' ? Array.prototype.slice.call(entry) : [entry]))
      .filter((entry) => entry[0] === 'event')
      .map((entry) => [entry[1], entry[2]]),
  );

async function session(name, contextOptions) {
  const context = await browser.newContext(contextOptions);
  const page = await context.newPage();
  const google = [];
  const collected = [];
  page.on('request', (req) => {
    if (/google-analytics|analytics\.google/.test(req.url())) {
      collected.push(decodeURIComponent(`${req.url()}||${req.postData() ?? ''}`));
    }
    if (/googletagmanager|google-analytics|analytics\.google/.test(req.url())) google.push(req.url());
  });
  const consoleErrors = [];
  page.on('pageerror', (e) => consoleErrors.push(String(e)));
  await page.goto(ENTRY, { waitUntil: 'networkidle' });

  const panel = page.locator('[data-consent-panel]');
  const accept = page.locator('[data-consent-action="granted"]');
  const decline = page.locator('[data-consent-action="denied"]');
  const reopen = page.locator('[data-consent-action="reopen"]');

  const before = {
    panelVisible: await panel.isVisible(),
    googleRequests: google.length,
    dataLayer: await page.evaluate(() => typeof window.dataLayer),
    storage: await page.evaluate(() => localStorage.getItem('ferryx.site.analytics-consent')),
    acceptBox: await accept.boundingBox(),
    declineBox: await decline.boundingBox(),
    identicalStyling: await page.evaluate(() => {
      const a = getComputedStyle(document.querySelector('[data-consent-action="granted"]'));
      const d = getComputedStyle(document.querySelector('[data-consent-action="denied"]'));
      return a.backgroundColor === d.backgroundColor && a.color === d.color && a.fontWeight === d.fontWeight;
    }),
  };
  await page.screenshot({ path: path.join(OUT, `${name}-prompt.png`) });

  await decline.click();
  const afterDecline = {
    panelVisible: await panel.isVisible(),
    reopenVisible: await reopen.isVisible(),
    googleRequests: google.length,
    storage: await page.evaluate(() => localStorage.getItem('ferryx.site.analytics-consent')),
  };
  await page.screenshot({ path: path.join(OUT, `${name}-declined.png`) });

  await reopen.click();
  const afterReopen = {
    panelVisible: await panel.isVisible(),
    focused: await page.evaluate(() => document.activeElement?.dataset?.consentAction ?? null),
  };

  const pageViewRequest = page.waitForRequest(
    (req) => /google-analytics|analytics\.google/.test(req.url()) &&
      /(?:[?&\n]|^)en=page_view(?:[&\n]|$)/.test(`${req.url()}\n${req.postData() ?? ''}`),
    { timeout: 15000 },
  );
  await accept.click();
  await pageViewRequest;
  const afterAccept = {
    googleRequestCount: google.length,
    storage: await page.evaluate(() => localStorage.getItem('ferryx.site.analytics-consent')),
    events: await readEvents(page),
    reopenVisible: await reopen.isVisible(),
  };
  await page.screenshot({ path: path.join(OUT, `${name}-accepted.png`) });

  // Download intent through the real hydrated menu, with navigation suppressed.
  await page.locator('button[aria-label="Select platform and architecture"]').click();
  await page.screenshot({ path: path.join(OUT, `${name}-menu.png`) });
  const assetLink = page
    .locator('[data-ferryx-location="download_menu"] a[href*="releases/latest/download"]')
    .first();
  await assetLink.evaluate((el) => el.addEventListener('click', (e) => e.preventDefault(), true));
  const downloadRequest = page.waitForRequest(
    (req) => /google-analytics|analytics\.google/.test(req.url()) &&
      /(?:[?&\n]|^)en=download_click(?:[&\n]|$)/.test(`${req.url()}\n${req.postData() ?? ''}`),
    { timeout: 15000 },
  );
  await assetLink.click();
  await downloadRequest;

  const networkHits = collected.map((hit) => ({
    events: [...hit.matchAll(/(?:[?&\n]|^)en=([^&\n|]+)/g)].map((m) => m[1]),
    reportedUrl: /[?&]dl=([^&|]*)/.exec(hit)?.[1] ?? null,
    leaksQuery: hit.includes(LEAK),
  }));

  results.push({
    name,
    before,
    afterDecline,
    afterReopen,
    afterAccept,
    events: await readEvents(page),
    networkHits,
    consoleErrors,
  });
  await context.close();
}

await session('desktop', { viewport: { width: 1440, height: 900 } });
await session('mobile', devices['iPhone 13']);

await browser.close();
server.stop(true);
console.log(JSON.stringify(results, null, 2));
