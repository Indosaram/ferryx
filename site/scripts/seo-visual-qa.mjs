import path from "node:path";
import { existsSync, mkdirSync, statSync } from "node:fs";

const SITE_ROOT = path.resolve(import.meta.dir, "..");
const DIST = path.join(SITE_ROOT, "dist");
const BASE = "/ferryx";
const evidenceDir = process.argv[2] ?? path.join(SITE_ROOT, "seo-evidence");
mkdirSync(evidenceDir, { recursive: true });

// Injected before hydration so client-side errors are captured from the first tick.
const ERROR_COLLECTOR = `<script>
window.__seoErrors = [];
addEventListener("error", (e) => window.__seoErrors.push("error: " + (e.message || e.type)));
addEventListener("unhandledrejection", (e) => window.__seoErrors.push("rejection: " + String(e.reason)));
const nativeError = console.error;
console.error = (...args) => { window.__seoErrors.push("console: " + args.map((a) => { try { return typeof a === "object" && a !== null ? JSON.stringify(a, (k, v) => (v instanceof Error ? v.message : v)) : String(a); } catch { return String(a); } }).join(" ")); nativeError(...args); };
</script>`;

const server = Bun.serve({
  port: 0,
  async fetch(request) {
    const url = new URL(request.url);
    let pathname = decodeURIComponent(url.pathname);
    if (pathname.startsWith(BASE)) pathname = pathname.slice(BASE.length) || "/";
    let file = path.join(DIST, pathname);
    if (existsSync(file) && statSync(file).isDirectory()) file = path.join(file, "index.html");
    if (!existsSync(file)) return new Response("not found", { status: 404 });
    if (file.endsWith(".html")) {
      const html = await Bun.file(file).text();
      return new Response(html.replace("<head>", `<head>${ERROR_COLLECTOR}`), {
        headers: { "Content-Type": "text/html; charset=utf-8" },
      });
    }
    return new Response(Bun.file(file));
  },
});

const origin = `http://localhost:${server.port}${BASE}/`;

// WebKit restores a per-URL scroll offset across views in the same process, so the
// top capture must reset it explicitly or it silently photographs the wrong region.
const WAIT_FOR_LOAD = `new Promise((resolve) => {
  const settle = () => {
    history.scrollRestoration = "manual";
    window.scrollTo(0, 0);
    requestAnimationFrame(() => requestAnimationFrame(() => resolve(window.scrollY)));
  };
  if (document.readyState === "complete") { settle(); return; }
  addEventListener("load", settle, { once: true });
})`;

const PROBE = `(() => {
  const h1 = document.querySelector("h1");
  const previewHeading = document.querySelector("#preview h2");
  const jsonLd = [...document.querySelectorAll('script[type="application/ld+json"]')].map((s) => s.textContent);
  const fontLink = document.querySelector("link[rel=stylesheet][href*='fonts.googleapis.com']");
  const types = [];
  for (const raw of jsonLd) {
    try {
      const parsed = JSON.parse(raw);
      for (const node of parsed["@graph"] ?? [parsed]) types.push(node["@type"]);
    } catch (error) { types.push("PARSE_ERROR: " + error.message); }
  }
  return {
    title: document.title,
    h1Text: h1 ? h1.innerText.trim().replace(/\\s+/g, " ") : null,
    h1Height: h1 ? Math.round(h1.getBoundingClientRect().height) : 0,
    previewHeading: previewHeading ? previewHeading.innerText.trim() : null,
    jsonLdTypes: types,
    architectureLinks: [...document.querySelectorAll("a[href*='docs/architecture']")].map((a) => a.getAttribute("href")),
    downloadControls: [...document.querySelectorAll("button, a")].filter((el) => /download/i.test(el.textContent || "")).length,
    fontsMedia: fontLink ? fontLink.media : null,
    featuresRendered: !!document.querySelector("#features h2"),
    footerRendered: !!document.querySelector("footer"),
    imagesMissingSize: [...document.images].filter((img) => !img.getAttribute("width") || !img.getAttribute("height")).map((img) => img.currentSrc || img.src),
    docHeight: document.documentElement.scrollHeight,
    errors: window.__seoErrors ?? ["collector-missing"],
  };
})()`;

// C5 requires the DownloadMenu to actually work, so drive it rather than just seeing it.
const DOWNLOAD_MENU_CHECK = `new Promise((resolve) => {
  window.scrollTo(0, 0);
  const trigger = [...document.querySelectorAll("button[aria-expanded]")].find((b) => /platform/i.test(b.getAttribute("aria-label") || ""));
  if (!trigger) { resolve({ triggerFound: false }); return; }
  const before = trigger.getAttribute("aria-expanded");
  trigger.click();
  requestAnimationFrame(() => requestAnimationFrame(() => {
    const after = trigger.getAttribute("aria-expanded");
    const options = [...document.querySelectorAll("a[href], button")]
      .filter((el) => el.offsetParent !== null && /\\.(dmg|exe|msi|appimage|deb|zip|tar\\.gz)\\b/i.test(el.textContent || ""))
      .map((el) => el.textContent.trim().replace(/\\s+/g, " "));
    trigger.click();
    resolve({ triggerFound: true, expandedBefore: before, expandedAfter: after, optionCount: options.length, options: options.slice(0, 8) });
  }));
})`;

const report = { url: origin, viewports: [] };

for (const viewport of [
  { name: "desktop", width: 1440, height: 900 },
  { name: "mobile", width: 390, height: 844 },
]) {
  await using view = new Bun.WebView({ width: viewport.width, height: viewport.height });
  await view.navigate(origin);
  const scrollYAtTop = await view.evaluate(WAIT_FOR_LOAD);
  if (scrollYAtTop !== 0) throw new Error(`${viewport.name}: page did not reset to scrollY 0 (got ${scrollYAtTop})`);

  const top = path.join(evidenceDir, `landing-${viewport.name}-top.png`);
  await Bun.write(top, await view.screenshot());

  // Scroll past the fold so client:visible islands hydrate, then capture again.
  await view.evaluate(`new Promise((resolve) => {
    window.scrollTo(0, document.documentElement.scrollHeight);
    requestAnimationFrame(() => requestAnimationFrame(() => resolve(true)));
  })`);
  const bottom = path.join(evidenceDir, `landing-${viewport.name}-bottom.png`);
  await Bun.write(bottom, await view.screenshot());

  const downloadMenu = await view.evaluate(DOWNLOAD_MENU_CHECK);
  const probe = await view.evaluate(PROBE);
  report.viewports.push({ ...viewport, screenshots: [top, bottom], downloadMenu, probe });
}

server.stop(true);
console.log(JSON.stringify(report, null, 2));
