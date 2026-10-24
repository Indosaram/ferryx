#!/usr/bin/env bun
/**
 * scripts/qa/sidebar-parity-visual-qa.mjs
 *
 * Captures the desktop sidebar harness and the remote harness rendering the same parity fixture
 * (ui/src/components/sidebar/sidebarParityFixture.ts), dark theme, and fails when the remote's
 * text structure (project labels, worktree labels, slot texts, in order) differs from the desktop's.
 *
 *   bun scripts/qa/sidebar-parity-visual-qa.mjs --evidence-dir <dir>
 *
 * Starts its own Vite dev server on a free port and a browser on a throwaway profile; both stop
 * before exit.
 */

import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = join(fileURLToPath(import.meta.url), "..", "..", "..");

function parseArgs(argv) {
  const args = { evidenceDir: null };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--evidence-dir") args.evidenceDir = argv[++i];
    else {
      console.error(`sidebar-parity-visual-qa: unrecognized argument: ${argv[i]}`);
      process.exit(2);
    }
  }
  if (!args.evidenceDir) {
    console.error("usage: bun scripts/qa/sidebar-parity-visual-qa.mjs --evidence-dir <dir>");
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

/** Project headers and worktree rows in document order, with their slot texts. */
const STRUCTURE_PROBE = (rootSelector) => {
  const root = document.querySelector(rootSelector);
  if (!root) return { error: `${rootSelector} not found` };
  const text = (el) => (el?.textContent ?? "").replace(/\s+/g, " ").trim();
  const entries = [];
  for (const el of root.querySelectorAll("button[aria-expanded], button[data-shortcut-worktree-path]")) {
    if (el.hasAttribute("aria-expanded")) {
      const header = el.parentElement;
      const select = el.nextElementSibling;
      const label = select?.getAttribute("aria-label") ?? "";
      const labelText = text(select?.querySelector("span"));
      entries.push({ kind: "project", label, expanded: el.getAttribute("aria-expanded") === "true", slot: text(header).slice(labelText.length).trim() });
    } else {
      const label = text(el.firstElementChild);
      entries.push({ kind: "worktree", label, slot: text(el).slice(label.length).trim(), active: el.getAttribute("aria-current") === "true" });
    }
  }
  const empties = [...root.querySelectorAll('[role="status"]')].map(text);
  return { entries, empties };
};

async function settle(page) {
  await page.waitForSelector('html[data-qa-ready="true"]', { timeout: 15_000 });
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
  await page.evaluate(() =>
    Promise.all(
      document.getAnimations()
        .filter((animation) => animation.effect?.getTiming().iterations !== Infinity)
        .map((animation) => animation.finished.catch(() => undefined)),
    ).then(() => undefined));
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve(undefined)))));
}

const CAPTURES = [
  { name: "desktop-1280x800", path: "/sidebar-qa.html?fixture=parity&theme=dark", viewport: { width: 1280, height: 800 }, root: '[data-testid="sidebar-qa-root"] [data-testid="worktree-region"]' },
  { name: "remote-1280x800", path: "/mobile-chat-qa.html?state=sidebar-parity", viewport: { width: 1280, height: 800 }, root: '[data-testid="remote-desktop-sidebar"] [data-testid="worktree-region"]' },
  { name: "remote-768x1024", path: "/mobile-chat-qa.html?state=sidebar-parity", viewport: { width: 768, height: 1024 }, root: '[data-testid="remote-desktop-sidebar"] [data-testid="worktree-region"]' },
  { name: "remote-390x844-drawer", path: "/mobile-chat-qa.html?state=sidebar-parity", viewport: { width: 390, height: 844 }, root: '[data-testid="remote-sidebar-drawer"] [data-testid="worktree-region"]', touch: true },
  { name: "phone-home-worktrees-390x844", path: "/mobile-chat-qa.html?state=home-worktrees", viewport: { width: 390, height: 844 }, root: '[data-testid="remote-worktrees-inline"] [data-testid="worktree-region"]', touch: true },
  { name: "remote-1280x800-inventory", path: "/mobile-chat-qa.html?state=sidebar-parity-inventory", viewport: { width: 1280, height: 800 }, root: '[data-testid="remote-desktop-sidebar"] [data-testid="worktree-region"]' },
];

const NOT_SYNCED_PROBE = () => {
  const el = document.querySelector('[data-testid="remote-sidebar-not-synced"]');
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return { text: (el.textContent ?? "").trim(), visible: r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== "hidden" };
};

function diffStructures(expected, actual) {
  const a = JSON.stringify(expected, null, 1).split("\n");
  const b = JSON.stringify(actual, null, 1).split("\n");
  const lines = [];
  for (let i = 0; i < Math.max(a.length, b.length); i += 1) {
    if (a[i] !== b[i]) lines.push(`@${i}: desktop ${a[i] ?? "<none>"} | remote ${b[i] ?? "<none>"}`);
  }
  return lines;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  mkdirSync(args.evidenceDir, { recursive: true });
  const port = await freePort();
  const vite = startVite(port);
  const profile = mkdtempSync(join(tmpdir(), "sidebar-parity-qa-"));
  const report = { port, vitePid: vite.child.pid, captures: {}, consoleErrors: [], failures: [] };
  let context = null;
  try {
    await vite.ready;
    const { chromium } = loadPlaywright();
    context = await chromium.launchPersistentContext(profile, { channel: "chrome", headless: true, colorScheme: "dark" });
    // Init scripts run before <html> exists: set the theme the moment the root element attaches.
    await context.addInitScript(() => {
      const apply = () => {
        if (!document.documentElement) return false;
        document.documentElement.dataset.theme = "dark";
        return true;
      };
      if (apply()) return;
      const observer = new MutationObserver(() => { if (apply()) observer.disconnect(); });
      observer.observe(document, { childList: true });
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
      const structure = await page.evaluate(STRUCTURE_PROBE, capture.root);
      const notSynced = await page.evaluate(NOT_SYNCED_PROBE);
      const shot = join(args.evidenceDir, `${capture.name}.png`);
      await page.screenshot({ path: shot });
      report.captures[capture.name] = { structure, notSynced, screenshot: shot };
      if (structure.error) report.failures.push(`${capture.name}: ${structure.error}`);
      await page.close();
    }
  } finally {
    await context?.close();
    vite.child.kill("SIGTERM");
    rmSync(profile, { recursive: true, force: true });
  }

  const { captures, failures } = report;
  report.parityDiff = diffStructures(captures["desktop-1280x800"].structure, captures["remote-1280x800"].structure);
  if (report.parityDiff.length > 0) failures.push(`desktop vs remote 1280 text structure differs (${report.parityDiff.length} lines)`);
  if ((captures["desktop-1280x800"].structure.entries ?? []).length === 0) failures.push("desktop capture has no sidebar entries");
  for (const name of ["remote-768x1024", "remote-390x844-drawer", "phone-home-worktrees-390x844"]) {
    const labels = (captures[name].structure.entries ?? []).map((entry) => `${entry.kind}:${entry.label}`);
    const expected = (captures["desktop-1280x800"].structure.entries ?? []).map((entry) => `${entry.kind}:${entry.label}`);
    if (JSON.stringify(labels) !== JSON.stringify(expected)) failures.push(`${name}: labels differ from the desktop: ${JSON.stringify(labels)}`);
  }
  if (captures["remote-1280x800"].notSynced !== null) failures.push("desktop-sourced remote shows the not-synced caption");
  const inventory = captures["remote-1280x800-inventory"].notSynced;
  if (!inventory?.visible || inventory.text !== "Not synced with desktop") failures.push(`inventory-sourced remote lacks the visible not-synced caption: ${JSON.stringify(inventory)}`);
  if (report.consoleErrors.length > 0) failures.push(`${report.consoleErrors.length} console errors`);

  const reportPath = join(args.evidenceDir, "sidebar-parity-report.json");
  writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  console.log(JSON.stringify({ reportPath, gate: failures.length === 0 ? "PASS" : "FAIL", failures, parityDiff: report.parityDiff, consoleErrors: report.consoleErrors }, null, 2));
  process.exit(failures.length === 0 ? 0 : 1);
}

await main();
