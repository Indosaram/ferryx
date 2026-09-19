// Shared navbar geometry probe, used by both scripts/navbar-visual-qa.mjs and the
// navbar.responsive.test.ts regression suite so the script and the test can never drift.
import path from "node:path";
import { existsSync, mkdirSync, statSync } from "node:fs";

const PLAYWRIGHT_CANDIDATES = [
  process.env.PLAYWRIGHT_MODULE,
  "playwright",
  "../../ui/node_modules/playwright/index.mjs",
].filter(Boolean);

/** Resolves an already-installed Playwright, or null when none is reachable. */
export async function loadPlaywright() {
  for (const candidate of PLAYWRIGHT_CANDIDATES) {
    try {
      return await import(candidate);
    } catch {
      // Try the next candidate; the caller decides what an exhausted list means.
    }
  }
  return null;
}

/** Serves a built dist directory on an ephemeral port. */
export function serveDist(dist) {
  if (!existsSync(path.join(dist, "index.html"))) throw new Error(`no index.html in ${dist}`);
  return Bun.serve({
    port: 0,
    async fetch(request) {
      const url = new URL(request.url);
      let file = path.join(dist, decodeURIComponent(url.pathname));
      if (existsSync(file) && statSync(file).isDirectory()) file = path.join(file, "index.html");
      if (!existsSync(file)) return new Response("not found", { status: 404 });
      return new Response(Bun.file(file));
    },
  });
}

// Evaluated in the page. A squeezed flex item still paints its text outside its own box,
// so box-vs-box intersection alone misses the real collision; Range rects give the painted
// ink extent, which is what a visitor actually sees overlapping.
export const NAVBAR_PROBE = `(() => {
  const round = (n) => Math.round(n * 100) / 100;
  const rect = (el) => { const r = el.getBoundingClientRect(); return { x: round(r.x), y: round(r.y), w: round(r.width), h: round(r.height), right: round(r.right), bottom: round(r.bottom) }; };
  const header = document.querySelector("header");
  if (!header) return { error: "no header" };
  const pill = header.firstElementChild;
  const groups = [...pill.children].map((el, i) => ({
    index: i,
    tag: el.tagName.toLowerCase(),
    label: (el.textContent || "").trim().replace(/\\s+/g, " ").slice(0, 40) || el.className,
    visible: el.getClientRects().length > 0 && getComputedStyle(el).visibility !== "hidden",
    rect: rect(el),
  }));
  const visible = groups.filter((g) => g.visible && g.rect.w > 0);
  const overlaps = [];
  for (let a = 0; a < visible.length; a++) {
    for (let b = a + 1; b < visible.length; b++) {
      const A = visible[a].rect, B = visible[b].rect;
      const ox = Math.min(A.right, B.right) - Math.max(A.x, B.x);
      const oy = Math.min(A.bottom, B.bottom) - Math.max(A.y, B.y);
      if (ox > 0.5 && oy > 0.5) overlaps.push({ a: visible[a].label, b: visible[b].label, overlapX: round(ox), overlapY: round(oy) });
    }
  }
  const textRuns = [];
  const walker = document.createTreeWalker(header, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (!node.textContent.trim()) continue;
    const range = document.createRange();
    range.selectNodeContents(node);
    const r = range.getBoundingClientRect();
    if (r.width < 0.5 || r.height < 0.5) continue;
    const ownerRect = node.parentElement.getBoundingClientRect();
    textRuns.push({
      text: node.textContent.trim().slice(0, 30),
      rect: rect(range),
      paintedOutsideOwnBox: round(Math.max(0, r.right - ownerRect.right) + Math.max(0, ownerRect.left - r.left)),
    });
  }
  const iconRects = [...header.querySelectorAll("svg, img")].filter((el) => el.getClientRects().length > 0).map((el) => ({
    label: el.tagName.toLowerCase() + ":" + ((el.closest("[aria-label]")?.getAttribute("aria-label")) || el.getAttribute("alt") || ""),
    rect: rect(el),
  }));
  const paintedOverlaps = [];
  for (const run of textRuns) {
    for (const icon of iconRects) {
      const ox = Math.min(run.rect.right, icon.rect.right) - Math.max(run.rect.x, icon.rect.x);
      const oy = Math.min(run.rect.bottom, icon.rect.bottom) - Math.max(run.rect.y, icon.rect.y);
      if (ox > 0.5 && oy > 0.5) paintedOverlaps.push({ text: run.text, icon: icon.label, overlapX: round(ox), overlapY: round(oy) });
    }
  }
  const pillStyle = getComputedStyle(pill);
  const contentWidth = pill.clientWidth - parseFloat(pillStyle.paddingLeft) - parseFloat(pillStyle.paddingRight);
  const childrenWidth = visible.reduce((sum, g) => sum + g.rect.w, 0);
  const controls = [...header.querySelectorAll("a[href], button")].filter((el) => el.getClientRects().length > 0).map((el) => ({
    label: (el.getAttribute("aria-label") || el.textContent || "").trim().replace(/\\s+/g, " ").slice(0, 40),
    rect: rect(el),
  }));
  return {
    viewportWidth: window.innerWidth,
    groups,
    overlaps,
    paintedOverlaps,
    clippedText: textRuns.filter((t) => t.paintedOutsideOwnBox > 0.5),
    pill: rect(pill),
    header: rect(header),
    contentWidth: round(contentWidth),
    childrenWidth: round(childrenWidth),
    contentOverflow: round(childrenWidth - contentWidth),
    pillScrollOverflow: round(pill.scrollWidth - pill.clientWidth),
    controlsOutsideViewport: controls.filter((c) => c.rect.x < -0.5 || c.rect.right > window.innerWidth + 0.5),
    controlsBelowMinTapTarget: controls.filter((c) => c.rect.w < 24 || c.rect.h < 24).map((c) => ({ label: c.label, w: c.rect.w, h: c.rect.h })),
    documentHorizontalOverflow: round(document.documentElement.scrollWidth - window.innerWidth),
    downloadReachable: controls.some((c) => /download/i.test(c.label)),
    themeToggleReachable: controls.some((c) => /theme/i.test(c.label)),
    versionBadgeText: [...header.querySelectorAll("*")].map((el) => (el.children.length === 0 ? (el.textContent || "").trim() : "")).filter((t) => /^v\\d+\\.\\d+\\.\\d+/.test(t)),
  };
})()`;

// A download menu that opens off-screen is as broken as an overlap, so drive it for real.
const MENU_CHECK = `(async () => {
  const round = (n) => Math.round(n * 100) / 100;
  const trigger = [...document.querySelectorAll("header button[aria-expanded]")].pop();
  if (!trigger) return { triggerFound: false };
  trigger.click();
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  const panel = document.querySelector("[data-ferryx-location='download_menu']");
  const r = panel ? panel.getBoundingClientRect() : null;
  const state = {
    triggerFound: true,
    expanded: trigger.getAttribute("aria-expanded"),
    panel: r ? { x: round(r.x), right: round(r.right), w: round(r.width) } : null,
    panelWithinViewport: r ? r.x >= -0.5 && r.right <= window.innerWidth + 0.5 : null,
  };
  trigger.click();
  await new Promise((r2) => requestAnimationFrame(() => requestAnimationFrame(r2)));
  return state;
})()`;

export const DEFAULT_VIEWPORTS = [
  { name: "320", width: 320, height: 720 },
  { name: "390", width: 390, height: 844 },
  { name: "768", width: 768, height: 1024 },
  { name: "1440", width: 1440, height: 900 },
];

/**
 * Loads the built landing page in the locally installed Google Chrome at each viewport and
 * returns navbar geometry, download-menu state, and page errors. No browser is downloaded.
 */
export async function measureNavbar({ dist, viewports = DEFAULT_VIEWPORTS, evidenceDir, label = "run" }) {
  const playwright = await loadPlaywright();
  if (!playwright) throw new Error("playwright not resolvable; set PLAYWRIGHT_MODULE");
  if (evidenceDir) mkdirSync(evidenceDir, { recursive: true });

  const server = serveDist(dist);
  const origin = `http://localhost:${server.port}/`;
  const browser = await playwright.chromium.launch({ channel: "chrome" });
  const results = [];

  try {
    for (const vp of viewports) {
      const context = await browser.newContext({
        viewport: { width: vp.width, height: vp.height },
        deviceScaleFactor: 2,
        isMobile: vp.width < 768,
        hasTouch: vp.width < 768,
      });
      const page = await context.newPage();
      const consoleErrors = [];
      page.on("pageerror", (e) => consoleErrors.push(`pageerror: ${e.message}`));
      page.on("console", (m) => { if (m.type() === "error") consoleErrors.push(`console: ${m.text()}`); });

      await page.goto(origin, { waitUntil: "load" });
      // Hydration signal, not a timer: the toggle exists only once the island mounts.
      await page.waitForSelector("header button[aria-label*='theme']", { state: "attached" });

      const probe = await page.evaluate(NAVBAR_PROBE);
      const screenshots = {};
      if (evidenceDir) {
        screenshots.navbar = path.join(evidenceDir, `navbar-${label}-${vp.name}.png`);
        await page.screenshot({ path: screenshots.navbar, clip: { x: 0, y: 0, width: vp.width, height: Math.min(vp.height, 220) } });
        screenshots.full = path.join(evidenceDir, `landing-${label}-${vp.name}.png`);
        await page.screenshot({ path: screenshots.full });
      }
      const menuState = await page.evaluate(MENU_CHECK);
      results.push({ ...vp, probe, menuState, consoleErrors, screenshots });
      await context.close();
    }
  } finally {
    await browser.close();
    server.stop(true);
  }

  return { label, origin, dist, viewports: results };
}
