#!/usr/bin/env bun
/**
 * ui/scripts/supervisor-panel-visual-qa.mjs
 *
 * Captures the Supervisor panel showcase (ui/supervisor-panel-qa.html) at 1440 px (desktop and
 * phone surfaces side by side) and at 390 px (phone scale on every block), dark theme, and
 * measures every interactive control's hit area at 390 px from the DOM. Fails when a control is
 * smaller than 44x44 CSS px on the phone, when the page overflows horizontally at 390 px, or when
 * the page logs an error.
 *
 *   bun ui/scripts/supervisor-panel-visual-qa.mjs --evidence-dir <dir>
 *
 * Starts its own Vite dev server on a free port and a headless browser on a throwaway profile;
 * both stop before exit.
 */

import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = join(fileURLToPath(import.meta.url), "..", "..", "..");  // <repo>/ui/scripts/<file>
const MIN_TARGET = 44;

function parseArgs(argv) {
  const args = { evidenceDir: null };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--evidence-dir") args.evidenceDir = argv[++i];
    else {
      console.error(`supervisor-panel-visual-qa: unrecognized argument: ${argv[i]}`);
      process.exit(2);
    }
  }
  if (!args.evidenceDir) {
    console.error("usage: bun ui/scripts/supervisor-panel-visual-qa.mjs --evidence-dir <dir>");
    process.exit(2);
  }
  return args;
}

function loadPlaywright() {
  const require = createRequire(join(repo, "package.json"));
  return require(process.env.PLAYWRIGHT_CORE_PATH ?? join(repo, "ui", "node_modules", "playwright-core"));
}

function freePort() {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

/** Resolves once Vite prints its ready line; rejects when it exits first or stays silent for 60s. */
function startVite(port) {
  const child = spawn("bun", ["--bun", "vite"], {
    cwd: join(repo, "ui"),
    env: { ...process.env, FERRYX_DEV_PORT: String(port) },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const ready = new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("vite did not report ready within 60s")), 60_000);
    let output = "";
    const onData = (chunk) => {
      output += chunk.toString();
      if (/ready in/i.test(output)) {
        clearTimeout(timer);
        resolve();
      }
    };
    child.stdout.on("data", onData);
    child.stderr.on("data", onData);
    child.once("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`vite exited (${code}) before ready:\n${output}`));
    });
  });
  return { child, ready };
}

async function settle(page) {
  await page.waitForSelector('html[data-qa-ready="true"]', { timeout: 30_000 });
  await page.waitForSelector('[data-testid="supervisor-panel-showcase"]', { timeout: 15_000 });
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve(undefined)))));
}

/** Every visible interactive control with its rect, label and the block it sits in. */
const HIT_TARGET_PROBE = (minTarget) => {
  const selector = 'button, a[href], input, textarea, select, [role="button"], [role="option"], [role="menuitem"], [role="tab"], [tabindex]:not([tabindex="-1"])';
  const text = (el) => (el.getAttribute("aria-label") || el.textContent || el.getAttribute("placeholder") || "").replace(/\s+/g, " ").trim().slice(0, 80);
  const targets = [];
  for (const el of document.querySelectorAll(selector)) {
    const rect = el.getBoundingClientRect();
    const style = getComputedStyle(el);
    if (rect.width === 0 || rect.height === 0 || style.visibility === "hidden" || style.display === "none") continue;
    const block = el.closest("section")?.querySelector("h2")?.textContent?.trim() ?? null;
    targets.push({
      tag: el.tagName.toLowerCase(),
      role: el.getAttribute("role"),
      label: text(el),
      block,
      width: Math.round(rect.width * 10) / 10,
      height: Math.round(rect.height * 10) / 10,
      x: Math.round(rect.x),
      y: Math.round(rect.y + window.scrollY),
      ok: rect.width >= minTarget && rect.height >= minTarget,
    });
  }
  const doc = document.documentElement;
  return {
    targets,
    undersized: targets.filter((target) => !target.ok),
    overflowX: doc.scrollWidth > doc.clientWidth ? { scrollWidth: doc.scrollWidth, clientWidth: doc.clientWidth } : null,
  };
};

const CAPTURES = [
  { name: "showcase-1440-dark", path: "/supervisor-panel-qa.html?theme=dark", viewport: { width: 1440, height: 900 }, measure: false },
  { name: "showcase-390-dark", path: "/supervisor-panel-qa.html?theme=dark&touch=1", viewport: { width: 390, height: 844 }, measure: true, touch: true },
];

async function main() {
  const args = parseArgs(process.argv.slice(2));
  mkdirSync(args.evidenceDir, { recursive: true });
  const port = await freePort();
  const vite = startVite(port);
  const profile = mkdtempSync(join(tmpdir(), "supervisor-panel-qa-"));
  const report = { port, captures: {}, consoleErrors: [], failures: [] };
  let context = null;
  try {
    await vite.ready;
    const { chromium } = loadPlaywright();
    context = await chromium.launchPersistentContext(profile, {
      channel: process.env.PLAYWRIGHT_CHANNEL ?? "chrome",
      headless: true,
      colorScheme: "dark",
    });
    for (const capture of CAPTURES) {
      const page = await context.newPage();
      page.on("console", (message) => {
        if (message.type() === "error") report.consoleErrors.push(`${capture.name}: ${message.text()}`);
      });
      page.on("pageerror", (error) => report.consoleErrors.push(`${capture.name}: pageerror ${error.message}`));
      await page.setViewportSize(capture.viewport);
      await page.goto(`http://127.0.0.1:${port}${capture.path}`, { waitUntil: "load" });
      await settle(page);
      const shot = join(args.evidenceDir, `${capture.name}.png`);
      await page.screenshot({ path: shot, fullPage: true });
      const entry = { screenshot: shot, viewport: capture.viewport };
      if (capture.measure) {
        const measured = await page.evaluate(HIT_TARGET_PROBE, MIN_TARGET);
        entry.hitTargets = { minTarget: MIN_TARGET, count: measured.targets.length, undersized: measured.undersized, all: measured.targets };
        entry.overflowX = measured.overflowX;
        for (const target of measured.undersized) {
          report.failures.push(`${capture.name}: ${target.tag}${target.role ? `[${target.role}]` : ""} "${target.label}" in "${target.block}" is ${target.width}x${target.height}`);
        }
        if (measured.overflowX) report.failures.push(`${capture.name}: horizontal overflow ${JSON.stringify(measured.overflowX)}`);
      }
      report.captures[capture.name] = entry;
      await page.close();
    }
  } finally {
    await context?.close();
    vite.child.kill("SIGTERM");
    rmSync(profile, { recursive: true, force: true });
  }

  for (const error of report.consoleErrors) report.failures.push(`console: ${error}`);
  writeFileSync(join(args.evidenceDir, "supervisor-panel-hit-targets.json"), `${JSON.stringify(report, null, 2)}\n`);
  if (report.failures.length > 0) {
    console.error(`supervisor-panel-visual-qa: ${report.failures.length} failure(s)`);
    for (const failure of report.failures) console.error(`  ${failure}`);
    process.exit(1);
  }
  const phone = report.captures["showcase-390-dark"].hitTargets;
  console.log(`supervisor-panel-visual-qa: ok, ${phone.count} controls at 390 px, all >= ${MIN_TARGET} px`);
}

await main();
