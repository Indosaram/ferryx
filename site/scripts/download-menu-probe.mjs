// Ad-hoc measurement of the open download-menu panel geometry at narrow viewports, plus
// optional screenshots with the navbar panel left open. Not part of the suite;
// navbar.responsive.test.ts owns the regression.
import path from "node:path";
import { mkdirSync } from "node:fs";
import { loadPlaywright, serveDist } from "./navbar-probe.mjs";

const DIST = process.env.MENU_QA_DIST ?? path.resolve(import.meta.dir, "../../.navbar-qa-dist");
const EVIDENCE = process.env.MENU_QA_EVIDENCE ?? null;
const VIEWPORTS = [320, 390, 768, 1440];

/** `keepOpen` decides whether each panel is closed again after it is measured. */
const MEASURE = (keepOpen) => `(async () => {
  const round = (n) => Math.round(n * 100) / 100;
  const out = [];
  const triggers = [...document.querySelectorAll("button[aria-expanded]")];
  for (const trigger of triggers) {
    const surface = trigger.closest("header") ? "navbar" : "hero";
    trigger.click();
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    const panel = document.querySelector("[data-ferryx-location='download_menu']");
    const r = panel ? panel.getBoundingClientRect() : null;
    const t = trigger.getBoundingClientRect();
    out.push({
      surface,
      trigger: { x: round(t.x), right: round(t.right) },
      panel: r ? { x: round(r.x), right: round(r.right), w: round(r.width), bottom: round(r.bottom) } : null,
      within: r ? r.x >= -0.5 && r.right <= window.innerWidth + 0.5 : null,
      docOverflow: round(document.documentElement.scrollWidth - window.innerWidth),
    });
    if (${keepOpen ? "false" : "true"} || !r) {
      trigger.click();
      await new Promise((r2) => requestAnimationFrame(() => requestAnimationFrame(r2)));
    } else {
      break;
    }
  }
  return { viewportWidth: window.innerWidth, menus: out };
})()`;

const playwright = await loadPlaywright();
if (!playwright) throw new Error("playwright not resolvable; set PLAYWRIGHT_MODULE");
if (EVIDENCE) mkdirSync(EVIDENCE, { recursive: true });

const server = serveDist(DIST);
const browser = await playwright.chromium.launch({ channel: "chrome" });
try {
  for (const width of VIEWPORTS) {
    const context = await browser.newContext({
      viewport: { width, height: width < 768 ? 844 : 900 },
      deviceScaleFactor: 2,
      isMobile: width < 768,
      hasTouch: width < 768,
    });
    const page = await context.newPage();
    await page.goto(`http://localhost:${server.port}/`, { waitUntil: "load" });
    await page.waitForSelector("header button[aria-label*='theme']", { state: "attached" });
    console.log(JSON.stringify(await page.evaluate(MEASURE(false)), null, 1));
    if (EVIDENCE) {
      await page.evaluate(MEASURE(true));
      await page.waitForSelector("[data-ferryx-location='download_menu']", { state: "visible" });
      await page.screenshot({ path: path.join(EVIDENCE, `download-menu-${width}.png`) });
    }
    await context.close();
  }
} finally {
  await browser.close();
  server.stop(true);
}
