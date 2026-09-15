/**
 * Real-browser driver for the plan task 4 image renderer.
 *
 * Starts the harness Vite dev server on an OS-assigned loopback port (never
 * 5173), drives a real Chromium at 1280x800 and 390x844 through the actual
 * fit/100%/zoom/pan/reset controls and the corrupt + unsupported fixtures,
 * captures screenshots, then shuts the server and browser down and removes the
 * temporary node_modules symlink it created.
 *
 * Every wait is a DOM signal produced by a real browser event (the renderer only
 * renders its dimensions readout after an `img` load event, and its error panel
 * after an `img` error event). There is no sleep and no polling delay.
 *
 * Usage: bun docs/evidence/file-previews/task-4/harness/drive.mjs
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const UI = path.resolve(HERE, "../../../../../ui");
const SHOTS = path.resolve(HERE, "../screenshots");
const LINK = path.join(HERE, "node_modules");

let createdLink = false;
if (!fs.existsSync(LINK)) {
  fs.symlinkSync(path.join(UI, "node_modules"), LINK, "dir");
  createdLink = true;
}

// Playwright is a globally installed tool here, not a project dependency.
const playwrightModule = [
  process.env.PLAYWRIGHT_MODULE,
  path.join(os.homedir(), ".bun/install/global/node_modules/playwright/index.js"),
  "/usr/local/lib/node_modules/playwright/index.js",
  "/opt/homebrew/lib/node_modules/playwright/index.js",
].find((candidate) => candidate && fs.existsSync(candidate));
if (!playwrightModule) throw new Error("playwright not found; set PLAYWRIGHT_MODULE");

const { createServer } = await import(path.join(LINK, "vite/dist/node/index.js"));
const { chromium } = await import(playwrightModule);

fs.mkdirSync(SHOTS, { recursive: true });

const results = [];
let failures = 0;
function check(name, condition, detail = "") {
  const ok = Boolean(condition);
  if (!ok) failures += 1;
  results.push(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
  console.log(results[results.length - 1]);
}

const server = await createServer({ configFile: path.join(HERE, "vite.config.mjs") });
await server.listen();
const url = server.resolvedUrls.local[0];
console.log(`harness server: ${url}`);

const browser = await chromium.launch();

const VIEWPORTS = [
  { name: "desktop-1280x800", width: 1280, height: 800 },
  { name: "mobile-390x844", width: 390, height: 844 },
];

const FORMATS = [
  { fixture: "png", dimensions: "1600 × 1000" },
  { fixture: "jpeg", dimensions: "1200 × 750" },
  { fixture: "gif", dimensions: "640 × 400" },
  { fixture: "gif-animated", dimensions: "320 × 200" },
  { fixture: "webp", dimensions: "900 × 563" },
];

try {
  for (const viewport of VIEWPORTS) {
    const context = await browser.newContext({
      viewport: { width: viewport.width, height: viewport.height },
      deviceScaleFactor: 1,
    });
    const page = await context.newPage();
    const consoleErrors = [];
    page.on("console", (message) => {
      if (message.type() === "error") consoleErrors.push(message.text());
    });
    page.on("pageerror", (error) => consoleErrors.push(`pageerror: ${error.message}`));

    await page.goto(url, { waitUntil: "domcontentloaded" });

    const dimensions = page.locator('[data-testid="file-preview-image-dimensions"]');
    const zoom = page.locator('[data-testid="file-preview-image-zoom"]');
    const canvas = page.locator('[data-testid="file-preview-image-canvas"]');
    const frame = page.locator('[data-testid="file-preview-image-viewport"]');
    const log = page.locator('[data-testid="harness-log"]');

    // --- every allowlisted raster format actually decodes in the browser ---
    for (const format of FORMATS) {
      if (format.fixture !== "png") await page.click(`[data-fixture="${format.fixture}"]`);
      await dimensions.filter({ hasText: format.dimensions }).waitFor({ timeout: 10_000 });
      const decoded = await page.evaluate(() => {
        const img = document.querySelector("img");
        return img ? { complete: img.complete, width: img.naturalWidth, box: img.getBoundingClientRect().width } : null;
      });
      check(
        `${viewport.name} ${format.fixture} decoded and fitted`,
        decoded && decoded.complete && decoded.width > 0 && decoded.box > 0,
        JSON.stringify(decoded),
      );
      check(
        `${viewport.name} ${format.fixture} fits inside the viewport`,
        await page.evaluate(() => {
          const img = document.querySelector("img");
          const box = document.querySelector('[data-testid="file-preview-image-viewport"]');
          if (!img || !box) return false;
          const i = img.getBoundingClientRect();
          const b = box.getBoundingClientRect();
          return i.width <= b.width + 1 && i.height <= b.height + 1;
        }),
      );
    }

    await page.click('[data-fixture="png"]');
    await dimensions.filter({ hasText: "1600 × 1000" }).waitFor();
    await page.screenshot({ path: path.join(SHOTS, `${viewport.name}-png-fit.png`) });

    // --- 100% then 200%, with a real pointer drag pan ---
    await page.click('button[aria-label="Actual size"]');
    await zoom.filter({ hasText: "100%" }).waitFor();
    const actualWidth = await page.evaluate(() => document.querySelector("img").getBoundingClientRect().width);
    check(`${viewport.name} 100% renders intrinsic pixels`, Math.round(actualWidth) === 1600, `${actualWidth}px`);

    for (let i = 0; i < 10; i += 1) await page.click('button[aria-label="Zoom in"]');
    await zoom.filter({ hasText: "200%" }).waitFor();
    const zoomedWidth = await page.evaluate(() => document.querySelector("img").getBoundingClientRect().width);
    check(`${viewport.name} 200% doubles the rendered width`, Math.round(zoomedWidth) === 3200, `${zoomedWidth}px`);

    const box = await frame.boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 - 140, box.y + box.height / 2 - 90, { steps: 10 });
    await page.mouse.up();
    const transform = await canvas.evaluate((node) => node.style.transform);
    check(`${viewport.name} pan follows the pointer`, /translate\(-1\d\dpx, -\d\dpx\)/.test(transform), transform);
    await page.screenshot({ path: path.join(SHOTS, `${viewport.name}-png-zoom200-panned.png`) });

    await page.click('button[aria-label="Reset view"]');
    await zoom.filter({ hasText: "Fit" }).waitFor();
    check(
      `${viewport.name} reset clears zoom and pan`,
      (await canvas.evaluate((node) => node.style.transform)) === "",
    );

    // --- zoom bounds ---
    await page.click('button[aria-label="Actual size"]');
    for (let i = 0; i < 80; i += 1) await page.click('button[aria-label="Zoom in"]');
    await zoom.filter({ hasText: "800%" }).waitFor();
    check(
      `${viewport.name} zoom clamps at 800%`,
      await page.isDisabled('button[aria-label="Zoom in"]'),
    );
    for (let i = 0; i < 80; i += 1) await page.click('button[aria-label="Zoom out"]');
    await zoom.filter({ hasText: "10%" }).waitFor();
    check(
      `${viewport.name} zoom clamps at 10%`,
      await page.isDisabled('button[aria-label="Zoom out"]'),
    );
    await page.click('button[aria-label="Fit"]');
    await zoom.filter({ hasText: "Fit" }).waitFor();

    // --- corrupt image: real decode failure, retry and external action ---
    await page.click('[data-fixture="corrupt"]');
    await page.locator('[data-testid="file-preview-image-error"]').waitFor({ timeout: 10_000 });
    check(
      `${viewport.name} corrupt image reports an unknown machine reason`,
      (await log.textContent()).includes("reason=null"),
    );
    await page.screenshot({ path: path.join(SHOTS, `${viewport.name}-corrupt-error.png`) });
    await page.click('button:has-text("Open externally")');
    check(`${viewport.name} corrupt image offers external open`, (await log.textContent()).includes("external corrupt"));
    await page.click('button:has-text("Reload file")');
    check(`${viewport.name} corrupt image offers reload`, (await log.textContent()).includes("reload corrupt"));
    await page.click('button:has-text("Retry")');
    await page.locator('[data-testid="file-preview-image-error"]').waitFor();
    check(
      `${viewport.name} retry re-attempts the decode and fails again`,
      (await log.textContent()).match(/failure corrupt-\d+ reason=null/g).length >= 2,
    );

    // --- SVG and missing metadata never create an img element ---
    for (const fixture of ["svg", "no-url"]) {
      await page.click(`[data-fixture="${fixture}"]`);
      await page.locator('[data-testid="file-preview-image-unsupported"]').waitFor();
      check(
        `${viewport.name} ${fixture} creates no img element`,
        (await page.evaluate(() => document.querySelectorAll("img").length)) === 0,
      );
      check(
        `${viewport.name} ${fixture} reports UnsupportedFormat`,
        (await log.textContent()).includes(`failure ${fixture}`) &&
          (await log.textContent()).includes("reason=UnsupportedFormat"),
      );
    }
    await page.screenshot({ path: path.join(SHOTS, `${viewport.name}-unsupported.png`) });

    // --- replacement mid-load keeps the newest image only ---
    await page.click('[data-fixture="race"]');
    await dimensions.filter({ hasText: "900 × 563" }).waitFor({ timeout: 10_000 });
    check(
      `${viewport.name} replacement shows the newest image with no error`,
      (await page.locator('[data-testid="file-preview-image-error"]').count()) === 0,
    );

    const unexpected = consoleErrors.filter(
      (text) => !text.includes("corrupt.png") && !text.includes("diagram.svg"),
    );
    check(`${viewport.name} no unexpected console errors`, unexpected.length === 0, unexpected.join(" | "));

    await context.close();
  }
} finally {
  await browser.close();
  await server.close();
  if (createdLink) fs.rmSync(LINK, { force: true });
}

console.log(`\n${results.length} checks, ${failures} failed`);
process.exit(failures === 0 ? 0 : 1);
