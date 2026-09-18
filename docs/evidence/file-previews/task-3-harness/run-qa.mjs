#!/usr/bin/env bun
/**
 * Real-browser QA for the read-only text/Markdown preview renderer (plan task 3).
 *
 * Builds the isolated harness with the shipped Vite/Tailwind pipeline, serves it
 * from an ephemeral loopback port together with a stand-in capability endpoint,
 * then drives real Chromium at 1280x800 and 390x844. Every network request the
 * page makes is recorded: hostile Markdown must produce zero requests beyond the
 * harness origin, and no capability request for a refused source.
 *
 * Usage: bun docs/evidence/file-previews/task-3-harness/run-qa.mjs [--evidence-dir <dir>]
 * Owns only its own server and browser; it never starts the app, the daemon or :5173.
 */
import { createRequire } from "node:module";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, normalize, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const harnessDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(harnessDir, "../../../..");
const uiDir = join(repoRoot, "ui");
const fixturesDir = join(harnessDir, "fixtures");

let evidenceDir = join(repoRoot, "docs/evidence/file-previews/task-3");
const args = process.argv.slice(2);
for (let i = 0; i < args.length; i++) {
  if (args[i] === "--evidence-dir") evidenceDir = resolve(args[++i]);
  else {
    console.error(`unrecognized argument: ${args[i]}`);
    process.exit(2);
  }
}
const shotDir = join(evidenceDir, "screenshots");
mkdirSync(shotDir, { recursive: true });

const uiRequire = createRequire(join(uiDir, "package.json"));
const globalRequire = createRequire(join("/Users/indo/.bun/install/global/node_modules", "package.json"));
const importFrom = async (req, specifier) => import(pathToFileURL(req.resolve(specifier)).href);

const { build } = await importFrom(uiRequire, "vite");
const { default: reactPlugin } = await importFrom(uiRequire, "@vitejs/plugin-react");
const { default: tailwindcss } = await importFrom(uiRequire, "tailwindcss");
const { default: autoprefixer } = await importFrom(uiRequire, "autoprefixer");
const { chromium } = await importFrom(globalRequire, "playwright");
const { default: tailwindConfig } = await import(pathToFileURL(join(uiDir, "tailwind.config.js")).href);

// Tailwind content globs in the shipped config are cwd-relative.
process.chdir(uiDir);

const distDir = join(harnessDir, "dist");
await build({
  root: harnessDir,
  base: "./",
  logLevel: "warn",
  plugins: [reactPlugin()],
  resolve: {
    alias: {
      "@": join(uiDir, "src"),
      // The harness lives outside ui/, so its own bare imports need explicit targets.
      react: uiRequire.resolve("react"),
      "react/jsx-runtime": uiRequire.resolve("react/jsx-runtime"),
      "react-dom/client": uiRequire.resolve("react-dom/client"),
      "react-dom": uiRequire.resolve("react-dom"),
    },
  },
  css: {
    postcss: {
      plugins: [
        tailwindcss({
          ...tailwindConfig,
          content: [join(uiDir, "src/**/*.{ts,tsx}"), join(harnessDir, "*.{html,tsx}")],
        }),
        autoprefixer(),
      ],
    },
  },
  build: { outDir: distDir, emptyOutDir: true, target: "es2022" },
});

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".png": "image/png",
  ".woff2": "font/woff2",
  ".svg": "image/svg+xml",
};

/** Stand-in for the backend child-handle resolver: relative, contained, existing. */
function resolveChildAsset(relativePath) {
  const normalized = normalize(relativePath);
  if (normalized.startsWith("..") || normalized.startsWith("/")) return null;
  const full = join(harnessDir, normalized);
  if (!full.startsWith(fixturesDir)) return null;
  if (!existsSync(full)) return null;
  return full;
}

const childHandles = new Map();
const serverRequests = [];

const server = Bun.serve({
  hostname: "127.0.0.1",
  port: 0,
  fetch(request) {
    const url = new URL(request.url);
    serverRequests.push(url.pathname + url.search);

    if (url.pathname === "/capability/resolve") {
      const relativePath = url.searchParams.get("path") ?? "";
      const full = resolveChildAsset(relativePath);
      if (!full) {
        return Response.json({ reason: "MissingFile", message: `no such child asset` }, { status: 404 });
      }
      const handle = `child-${childHandles.size + 1}`;
      childHandles.set(handle, full);
      const bytes = readFileSync(full);
      return Response.json({
        handle,
        displayName: relativePath.split("/").pop(),
        kind: "image",
        byteLength: bytes.length,
        mediaType: "image/png",
        mediaUrl: `/capability/asset/${handle}`,
      });
    }

    if (url.pathname.startsWith("/capability/asset/")) {
      const full = childHandles.get(url.pathname.slice("/capability/asset/".length));
      if (!full) return new Response("gone", { status: 404 });
      return new Response(readFileSync(full), {
        headers: { "content-type": "image/png", "cache-control": "no-store" },
      });
    }

    const requested = url.pathname === "/" ? "/index.html" : url.pathname;
    const candidates = [join(distDir, requested), join(harnessDir, requested)];
    for (const candidate of candidates) {
      if (!candidate.startsWith(harnessDir)) continue;
      if (!existsSync(candidate)) continue;
      const extension = candidate.slice(candidate.lastIndexOf("."));
      return new Response(readFileSync(candidate), {
        headers: { "content-type": MIME[extension] ?? "application/octet-stream" },
      });
    }
    return new Response("not found", { status: 404 });
  },
});

const origin = `http://127.0.0.1:${server.port}`;
const VIEWPORTS = [
  { name: "1280x800", width: 1280, height: 800 },
  { name: "390x844", width: 390, height: 844 },
];

const checks = [];
function check(id, expectation, pass, detail) {
  checks.push({ id, expectation, pass, detail });
  console.log(`${pass ? "PASS" : "FAIL"} ${id} — ${expectation}${detail ? ` :: ${detail}` : ""}`);
}

const browser = await chromium.launch();
const networkLog = [];

async function withPage(viewport, url, body) {
  const context = await browser.newContext({
    viewport: { width: viewport.width, height: viewport.height },
    deviceScaleFactor: 2,
  });
  const page = await context.newPage();
  const requests = [];
  page.on("request", (request) => requests.push({ url: request.url(), type: request.resourceType() }));
  page.on("requestfailed", (request) =>
    requests.push({ url: request.url(), type: request.resourceType(), failed: true }),
  );
  const consoleErrors = [];
  page.on("pageerror", (error) => consoleErrors.push(String(error)));
  await page.goto(`${origin}${url}`, { waitUntil: "load" });
  await page.waitForSelector('[data-testid="file-preview-text"]');
  await page.evaluate(() => document.fonts.ready);
  const result = await body(page, requests, consoleErrors);
  networkLog.push({ scenario: `${url} @ ${viewport.name}`, requests });
  const foreign = requests.filter((entry) => !entry.url.startsWith(origin));
  check(
    `network-origin${url}@${viewport.name}`,
    "every request stays on the harness loopback origin",
    foreign.length === 0,
    foreign.map((entry) => entry.url).join(", ") || "0 foreign requests",
  );
  check(
    `no-page-errors${url}@${viewport.name}`,
    "no uncaught page errors",
    consoleErrors.length === 0,
    consoleErrors.join(" | ") || "none",
  );
  await context.close();
  return result;
}

async function shoot(page, name) {
  await page.screenshot({ path: join(shotDir, `${name}.png`), animations: "disabled" });
}

for (const viewport of VIEWPORTS) {
  // --- ordinary Markdown -------------------------------------------------
  await withPage(viewport, "/?fixture=markdown", async (page, requests) => {
    await page.waitForSelector('[data-testid="file-preview-image"]');
    await shoot(page, `task-3-markdown-rendered-${viewport.name}`);

    const imageReady = await page.$eval(
      '[data-testid="file-preview-image"]',
      (img) => img.complete && img.naturalWidth === 320 && img.currentSrc.includes("/capability/asset/"),
    );
    check(
      `markdown-local-image@${viewport.name}`,
      "relative image is decoded from a capability URL",
      imageReady,
      "naturalWidth 320 via /capability/asset/",
    );

    const structure = await page.evaluate(() => {
      const doc = document.querySelector('[data-testid="file-preview-markdown"]');
      return {
        headings: doc.querySelectorAll("h1, h2").length,
        rows: doc.querySelectorAll("tbody tr").length,
        anchors: doc.querySelectorAll("a[href]").length,
        overflow: doc.scrollWidth - doc.clientWidth,
      };
    });
    check(
      `markdown-structure@${viewport.name}`,
      "GFM headings and table rows render, no navigable anchor exists",
      structure.headings >= 3 && structure.rows === 3 && structure.anchors === 0,
      JSON.stringify(structure),
    );
    check(
      `markdown-no-overflow@${viewport.name}`,
      "long tokens stay inside the document viewport",
      structure.overflow <= 1,
      `scrollWidth-clientWidth=${structure.overflow}`,
    );

    await page.click('[data-testid="file-preview-external-link"]');
    await page.click('[data-testid="file-preview-document-link"]');
    const events = await page.evaluate(() => window.__harnessEvents);
    const externalOpens = events.filter((event) => event.kind === "openExternalUrl");
    const documentRequests = events.filter((event) => event.kind === "requestDocument");
    check(
      `markdown-capability-routing@${viewport.name}`,
      "anchor clicks reach the capability, not the network",
      externalOpens.length === 1 &&
        externalOpens[0].value === "https://ferryx.app/docs" &&
        documentRequests.length === 1 &&
        documentRequests[0].value === "./notes.md",
      JSON.stringify(events.filter((event) => event.kind !== "requestImage")),
    );
    check(
      `markdown-no-remote-fetch@${viewport.name}`,
      "clicking an external link performs no navigation or fetch",
      requests.every((entry) => !entry.url.includes("ferryx.app")),
      `${requests.length} requests total`,
    );

    const beforeScroll = await page.evaluate(
      () => document.querySelector('[data-testid="file-preview-markdown"]').scrollTop,
    );
    await page.click('[data-testid="file-preview-fragment-link"]');
    const afterScroll = await page.evaluate(
      () => document.querySelector('[data-testid="file-preview-markdown"]').scrollTop,
    );
    check(
      `markdown-fragment-scroll@${viewport.name}`,
      "in-document fragment scrolls the preview instead of navigating",
      afterScroll !== beforeScroll && page.url() === `${origin}/?fixture=markdown`,
      `scrollTop ${beforeScroll} -> ${afterScroll}`,
    );

    await page.click('[data-testid="file-preview-source-toggle"]');
    await page.waitForSelector('[data-testid="file-preview-source"]');
    await shoot(page, `task-3-markdown-source-${viewport.name}`);
    const sourceIsLiteral = await page.$eval(
      '[data-testid="file-preview-source"]',
      (node) => node.querySelector("table") === null && node.textContent.includes("| Field | Value | Notes |"),
    );
    check(
      `markdown-source-toggle@${viewport.name}`,
      "source mode shows literal Markdown, never a rendered table",
      sourceIsLiteral,
      "pipe table visible as text",
    );
  });

  // --- hostile Markdown --------------------------------------------------
  await withPage(viewport, "/?fixture=hostile", async (page, requests) => {
    await page.waitForSelector('[data-testid="file-preview-image-error"]');
    await shoot(page, `task-3-hostile-rendered-${viewport.name}`);

    const state = await page.evaluate(() => {
      const doc = document.querySelector('[data-testid="file-preview-markdown"]');
      return {
        pwned: window.__pwned ?? null,
        title: document.title,
        scripts: doc.querySelectorAll("script").length,
        iframes: doc.querySelectorAll("iframe").length,
        images: doc.querySelectorAll("img").length,
        anchors: doc.querySelectorAll("a[href]").length,
        rejectedLinks: doc.querySelectorAll('[data-testid="file-preview-rejected-link"]').length,
        rejectedImages: doc.querySelectorAll('[data-testid="file-preview-rejected-image"]').length,
        imageErrors: doc.querySelectorAll('[data-testid="file-preview-image-error"]').length,
        literalScript: doc.textContent.includes('<script>window.__pwned = "block-script"'),
        overflow: doc.scrollWidth - doc.clientWidth,
        events: window.__harnessEvents,
      };
    });

    check(
      `hostile-inert@${viewport.name}`,
      "no script ran and no active element was mounted",
      state.pwned === null &&
        state.title === "FilePreviewText QA harness (plan task 3)" &&
        state.scripts === 0 &&
        state.iframes === 0 &&
        state.images === 0 &&
        state.anchors === 0,
      JSON.stringify({
        pwned: state.pwned,
        scripts: state.scripts,
        iframes: state.iframes,
        images: state.images,
        anchors: state.anchors,
      }),
    );
    check(
      `hostile-literal-html@${viewport.name}`,
      "raw HTML is displayed as literal text",
      state.literalScript,
      "script source visible as text",
    );
    check(
      `hostile-refusals@${viewport.name}`,
      "every unsafe link and image is refused in place",
      state.rejectedLinks >= 7 && state.rejectedImages === 3 && state.imageErrors === 1,
      JSON.stringify({
        rejectedLinks: state.rejectedLinks,
        rejectedImages: state.rejectedImages,
        imageErrors: state.imageErrors,
      }),
    );
    check(
      `hostile-capability-budget@${viewport.name}`,
      "only the one relative image reached the capability",
      state.events.filter((event) => event.kind === "requestImage").length === 1 &&
        state.events.filter((event) => event.kind === "requestImage")[0].value ===
          "./fixtures/does-not-exist.png",
      JSON.stringify(state.events),
    );
    check(
      `hostile-no-overflow@${viewport.name}`,
      "the unbreakable token stays inside the viewport",
      state.overflow <= 1,
      `scrollWidth-clientWidth=${state.overflow}`,
    );

    const tracker = requests.filter(
      (entry) =>
        entry.url.includes("tracker.example") ||
        entry.url.startsWith("data:") ||
        entry.url.startsWith("file:"),
    );
    check(
      `hostile-zero-ambient-requests@${viewport.name}`,
      "hostile Markdown issues no remote, data or file request",
      tracker.length === 0,
      tracker.map((entry) => entry.url).join(", ") || "0 refused-scheme requests",
    );

    await page.click('[data-testid="file-preview-rejected-link"]');
    const afterClick = await page.evaluate(() => ({
      pwned: window.__pwned ?? null,
      events: window.__harnessEvents.length,
      url: window.location.href,
    }));
    check(
      `hostile-click-inert@${viewport.name}`,
      "clicking a refused link does nothing at all",
      afterClick.pwned === null &&
        afterClick.events === state.events.length &&
        afterClick.url === `${origin}/?fixture=hostile`,
      JSON.stringify(afterClick),
    );
  });

  // --- plain text with caret and search ----------------------------------
  await withPage(viewport, "/?fixture=text&line=4&col=3", async (page) => {
    await page.waitForSelector('[data-testid="file-preview-caret"]');
    const caret = await page.$eval('[data-testid="file-preview-caret"]', (node) => ({
      offset: node.dataset.offset,
      column: node.dataset.column,
    }));
    const location = await page.textContent('[data-testid="file-preview-location"]');
    check(
      `text-unicode-caret@${viewport.name}`,
      "caret column counts Unicode scalars on an emoji line",
      caret.offset === "6" && caret.column === "3" && location.trim() === "Ln 4, Col 3",
      JSON.stringify({ caret, location }),
    );

    await page.fill('[data-testid="file-preview-search-input"]', "hello");
    await page.waitForSelector('[data-testid="file-preview-match-0"]');
    await shoot(page, `task-3-text-search-${viewport.name}`);
    const search = await page.evaluate(() => {
      const source = document.querySelector('[data-testid="file-preview-source"]');
      return {
        status: document.querySelector('[data-testid="file-preview-search-status"]').textContent,
        matches: source.querySelectorAll("mark").length,
        selection: window.getSelection().toString(),
        editable: source.querySelectorAll("textarea, [contenteditable], input").length,
        overflow: source.scrollWidth - source.clientWidth,
      };
    });
    check(
      `text-search-selection@${viewport.name}`,
      "search highlights all matches and selects the active one",
      search.status === "1 of 3" && search.matches === 3 && search.selection.toLowerCase() === "hello",
      JSON.stringify(search),
    );
    check(
      `text-read-only@${viewport.name}`,
      "the document region mounts no editing surface",
      search.editable === 0,
      `${search.editable} editable nodes`,
    );
    check(
      `text-no-overflow@${viewport.name}`,
      "long paths wrap instead of scrolling horizontally",
      search.overflow <= 1,
      `scrollWidth-clientWidth=${search.overflow}`,
    );

    await page.fill('[data-testid="file-preview-search-input"]', "");
    await shoot(page, `task-3-text-${viewport.name}`);
  });
}

await browser.close();
server.stop(true);

const failed = checks.filter((entry) => !entry.pass);
const report = {
  generatedAt: new Date().toISOString(),
  origin,
  viewports: VIEWPORTS,
  checks,
  network: networkLog,
  serverRequests,
  passed: checks.length - failed.length,
  failed: failed.length,
};
writeFileSync(join(evidenceDir, "task-3-browser-report.json"), `${JSON.stringify(report, null, 2)}\n`);
console.log(`\n${report.passed}/${checks.length} checks passed; screenshots in ${shotDir}`);
process.exit(failed.length === 0 ? 0 : 1);
