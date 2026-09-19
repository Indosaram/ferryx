// Landing-page lab performance measurement against a built site.
//
// Usage: bun run site/scripts/landing-perf-qa.mjs <distDir> [label] [runs]
//
// This is a Lighthouse-equivalent *lab* measurement, not a Lighthouse score. It drives the
// locally installed Google Chrome through Playwright + CDP and applies Lighthouse's mobile
// preset explicitly:
//   - viewport 412x823 @ DPR 1.75, mobile emulation on
//   - CPU throttling 4x (Emulation.setCPUThrottlingRate)
//   - network "Slow 4G": 1638.4 kbps down / 675 kbps up / 150 ms RTT
// Metrics come from the page's own PerformanceObserver (FCP, LCP, CLS, long tasks) plus CDP
// network accounting, so every number below is observed, never modelled or scored.
import path from "node:path";
import { existsSync, mkdirSync, statSync } from "node:fs";

const playwrightEntry =
  process.env.PLAYWRIGHT_MODULE ?? "/Users/indo/.bun/install/global/node_modules/playwright/index.mjs";
const { chromium } = await import(playwrightEntry);

const SITE_ROOT = path.resolve(import.meta.dir ?? path.dirname(new URL(import.meta.url).pathname), "..");
const dist = path.resolve(process.argv[2] ?? path.join(SITE_ROOT, "dist"));
const label = process.argv[3] ?? "run";
const runs = Number(process.argv[4] ?? 3);

if (!existsSync(path.join(dist, "index.html"))) throw new Error(`no index.html in ${dist}`);

const server = Bun.serve({
  port: 0,
  async fetch(request) {
    const url = new URL(request.url);
    let file = path.join(dist, decodeURIComponent(url.pathname));
    if (existsSync(file) && statSync(file).isDirectory()) file = path.join(file, "index.html");
    if (!existsSync(file)) return new Response("not found", { status: 404 });
    return new Response(Bun.file(file));
  },
});
const origin = `http://localhost:${server.port}/`;

const MOBILE = { width: 412, height: 823, deviceScaleFactor: 1.75, isMobile: true, hasTouch: true };
const SLOW_4G = { offline: false, downloadThroughput: (1638.4 * 1024) / 8, uploadThroughput: (675 * 1024) / 8, latency: 150 };
const CPU_THROTTLE = 4;

// Registered via addInitScript so the observers exist before the first paint of the document.
const OBSERVER = `
window.__perf = { lcp: 0, cls: 0, longTasks: [], fcp: 0 };
new PerformanceObserver((l) => { for (const e of l.getEntries()) if (e.name === "first-contentful-paint") window.__perf.fcp = e.startTime; })
  .observe({ type: "paint", buffered: true });
new PerformanceObserver((l) => { const es = l.getEntries(); window.__perf.lcp = es[es.length - 1].startTime; })
  .observe({ type: "largest-contentful-paint", buffered: true });
new PerformanceObserver((l) => { for (const e of l.getEntries()) if (!e.hadRecentInput) window.__perf.cls += e.value; })
  .observe({ type: "layout-shift", buffered: true });
new PerformanceObserver((l) => { for (const e of l.getEntries()) window.__perf.longTasks.push({ start: e.startTime, duration: e.duration }); })
  .observe({ type: "longtask", buffered: true });
`;

const COLLECT = `(() => {
  const nav = performance.getEntriesByType("navigation")[0];
  const p = window.__perf;
  // Total Blocking Time: long-task time beyond 50 ms between FCP and load, Lighthouse's definition.
  const tbt = p.longTasks
    .filter((t) => t.start + t.duration > p.fcp)
    .reduce((sum, t) => sum + Math.max(0, Math.min(t.duration, t.start + t.duration - Math.max(t.start, p.fcp)) - 50), 0);
  const scripts = performance.getEntriesByType("resource").filter((r) => r.initiatorType === "script" || /\\.js(\\?|$)/.test(r.name));
  return {
    fcpMs: Math.round(p.fcp),
    lcpMs: Math.round(p.lcp),
    cls: Math.round(p.cls * 1000) / 1000,
    tbtMs: Math.round(tbt),
    longTaskCount: p.longTasks.length,
    longestTaskMs: Math.round(Math.max(0, ...p.longTasks.map((t) => t.duration))),
    domContentLoadedMs: Math.round(nav.domContentLoadedEventEnd),
    loadMs: Math.round(nav.loadEventEnd),
    scriptResourceCount: scripts.length,
    scriptTransferBytes: scripts.reduce((s, r) => s + r.transferSize, 0),
    scriptDecodedBytes: scripts.reduce((s, r) => s + r.decodedBodySize, 0),
  };
})()`;

const browser = await chromium.launch({ channel: "chrome" });
const samples = [];

for (let i = 0; i < runs; i++) {
  const context = await browser.newContext({ viewport: { width: MOBILE.width, height: MOBILE.height }, deviceScaleFactor: MOBILE.deviceScaleFactor, isMobile: MOBILE.isMobile, hasTouch: MOBILE.hasTouch });
  await context.addInitScript(OBSERVER);
  const page = await context.newPage();
  const cdp = await context.newCDPSession(page);
  await cdp.send("Network.enable");
  await cdp.send("Network.setCacheDisabled", { cacheDisabled: true });
  await cdp.send("Network.emulateNetworkConditions", SLOW_4G);
  await cdp.send("Emulation.setCPUThrottlingRate", { rate: CPU_THROTTLE });

  const wire = { requests: 0, encodedBytes: 0, byType: {} };
  cdp.on("Network.responseReceived", (e) => { wire.requests++; wire.byType[e.type] = (wire.byType[e.type] ?? 0) + 1; });
  cdp.on("Network.loadingFinished", (e) => { wire.encodedBytes += e.encodedDataLength; });

  await page.goto(origin, { waitUntil: "load", timeout: 120000 });
  // Hydration completion signal, not a timer: the island's interactive control appears only
  // after React mounts, and the network-idle wait bounds any trailing lazy chunks.
  await page.waitForSelector("header button[aria-label*='theme']", { state: "attached", timeout: 120000 });
  await page.waitForLoadState("networkidle", { timeout: 120000 });

  const metrics = await page.evaluate(COLLECT);
  samples.push({ ...metrics, wireRequests: wire.requests, wireEncodedBytes: wire.encodedBytes, wireByType: wire.byType });
  await context.close();
}

await browser.close();
server.stop(true);

const median = (key) => {
  const values = samples.map((s) => s[key]).sort((a, b) => a - b);
  return values[Math.floor(values.length / 2)];
};
const summary = {};
for (const key of Object.keys(samples[0])) {
  if (typeof samples[0][key] === "number") summary[key] = median(key);
}

console.log(JSON.stringify({
  label,
  dist,
  runs,
  device: "emulated mobile 412x823 @ DPR 1.75 (Lighthouse mobile preset geometry)",
  throttling: `CPU ${CPU_THROTTLE}x slowdown; network Slow 4G 1638.4/675 kbps, 150 ms RTT; browser cache disabled`,
  host: `local static server, ${process.platform}/${process.arch}`,
  median: summary,
  samples,
}, null, 2));
