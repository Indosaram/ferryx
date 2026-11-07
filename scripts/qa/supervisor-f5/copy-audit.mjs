#!/usr/bin/env bun
import { runGit } from '../unified-supervisor/git-env.mjs';
/**
 * scripts/qa/supervisor-f5/copy-audit.mjs
 *
 * F5 of docs/plans/SUPERVISOR_AUTONOMY_PANEL_PLAN_2026-11-02.md: "Copy and duplication audit:
 * automated DOM scan at 1440 and 390 for Must-NOT-Have strings and for any session id appearing
 * in two panel sections."
 *
 *   bun scripts/qa/supervisor-f5/copy-audit.mjs --ui <checkout>/ui --evidence-dir <new dir>
 *
 * Builds harness.tsx against the ui tree under test (--ui, default this checkout's ui; its
 * node_modules must be installed), serves the build on 127.0.0.1, and drives headless Chrome over
 * the real desktop Sidebar panel and the real phone home, each at 1440x900 and 390x844, in five
 * seeded states (fixture, failed, paused, setup, guards). Writes report.json, findings.json and
 * one full-page PNG per capture. Exit 0 no findings, 1 findings, 2 harness error.
 * The temp harness files placed in the ui tree, the server and the browser are removed on exit.
 */
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = join(fileURLToPath(import.meta.url), "..");
const repo = resolve(here, "..", "..", "..");

const SURFACES = ["desktop", "phone"];
const STATES = ["fixture", "failed", "paused", "setup", "guards"];
const VIEWPORTS = [
  { name: "1440", width: 1440, height: 900 },
  { name: "390", width: 390, height: 844 },
];
/** The fixtures' clock (ui/src/lib/supervisorPanel/fixtures.ts). */
const FIXTURE_NOW = Date.UTC(2026, 10, 2, 14, 17, 0);
const HARNESS_TSX = "src/__qa_f5_supervisor_panel.tsx";
const HARNESS_HTML = "__qa_f5_supervisor_panel.html";

function parseArgs(argv) {
  const args = { ui: join(repo, "ui"), evidenceDir: null };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--ui") args.ui = resolve(argv[++i]);
    else if (argv[i] === "--evidence-dir") args.evidenceDir = resolve(argv[++i]);
    else {
      console.error(`copy-audit: unrecognized argument: ${argv[i]}`);
      process.exit(2);
    }
  }
  if (!args.evidenceDir) {
    console.error("usage: bun scripts/qa/supervisor-f5/copy-audit.mjs [--ui <ui dir>] --evidence-dir <dir>");
    process.exit(2);
  }
  return args;
}

function loadPlaywright(ui) {
  const require = createRequire(join(ui, "package.json"));
  return require(process.env.PLAYWRIGHT_CORE_PATH ?? "playwright-core");
}

function gitRevision(dir) {
  const out = runGit(["-C", dir, "rev-parse", "HEAD"], { encoding: "utf8" });
  const dirty = runGit(["-C", dir, "status", "--porcelain", "--", "."], { encoding: "utf8" });
  return out.status === 0 ? `${out.stdout.trim()}${dirty.stdout.trim() ? "+dirty" : ""}` : "unknown";
}

function build(ui, outDir) {
  writeFileSync(
    join(ui, HARNESS_HTML),
    `<!doctype html><html lang="en"><head><meta charset="UTF-8" /><meta name="viewport" content="width=device-width, initial-scale=1.0, viewport-fit=cover" /><title>F5 copy audit</title></head><body><div id="root"></div><script type="module" src="/${HARNESS_TSX}"></script></body></html>\n`,
  );
  copyFileSync(join(here, "harness.tsx"), join(ui, HARNESS_TSX));
  const result = spawnSync("bun", ["--bun", "vite", "build", "--config", join(here, "vite.config.mjs")], {
    cwd: ui,
    env: { ...process.env, F5_UI_ROOT: ui, F5_OUT_DIR: outDir, F5_HTML: HARNESS_HTML },
    encoding: "utf8",
  });
  return { status: result.status, log: `${result.stdout ?? ""}${result.stderr ?? ""}` };
}

const MIME = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml", ".woff2": "font/woff2", ".woff": "font/woff", ".png": "image/png", ".json": "application/json" };

function serve(dir) {
  const server = createServer((req, res) => {
    const path = decodeURIComponent((req.url ?? "/").split("?")[0]);
    const file = join(dir, path === "/" ? HARNESS_HTML : path);
    if (!file.startsWith(dir) || !existsSync(file)) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { "content-type": MIME[extname(file)] ?? "application/octet-stream" }).end(readFileSync(file));
  });
  return new Promise((resolveServer) => server.listen(0, "127.0.0.1", () => resolveServer(server)));
}

/** Runs in the page: every finding for one capture. */
const SCAN = () => {
  const f5 = window.__f5;
  const findings = [];
  const exempt = [];
  const text = (el) => (el?.innerText ?? el?.textContent ?? "").replace(/\s+/g, " ").trim();
  const selectorOf = (el) => {
    const parts = [];
    for (let node = el; node && node.nodeType === 1 && node !== document.body; node = node.parentElement) {
      const id = node.getAttribute("data-testid");
      const label = node.getAttribute("aria-label");
      let part = node.tagName.toLowerCase();
      if (id) part += `[data-testid="${id}"]`;
      else if (label && /^(section|article|nav|aside)$/.test(part)) part += `[aria-label="${label}"]`;
      parts.unshift(part);
      if (id && parts.length > 1) break;
    }
    return parts.join(" > ");
  };
  const visible = (el) => !!el && (typeof el.checkVisibility === "function" ? el.checkVisibility() : el.getClientRects().length > 0);

  // Must NOT have: words and shapes that never belong in default views (plan D12 + Must NOT have).
  const PATTERNS = [
    { id: "second-surface:inbox", re: /\binbox\b/i },
    { id: "second-surface:recent-audit", re: /\brecent audit\b/i },
    { id: "second-surface:supervisor-page", re: /\bsupervisor page\b/i },
    { id: "vocab:lane", re: /\blanes?\b/i },
    { id: "vocab:goal", re: /\bgoals?\b/i },
    { id: "vocab:rev", re: /\brev(?:ision)?s?\b/i },
    { id: "vocab:grant", re: /\bgrant(?:s|ed)?\b/i },
    { id: "vocab:adopt", re: /\badopt(?:s|ed|ion)?\b/i },
    { id: "engine-code", re: /\b[A-Z][A-Z0-9]*_[A-Z0-9_]{2,}\b/ },
    { id: "json", re: /[{[]\s*"[A-Za-z_]+"\s*:/ },
    { id: "cli-command", re: /(?:^|\s)(?:(?:cargo|bun|npm|npx|sqlx|ferryx-cli|curl)\s+[a-z-]+|git\s+(?:status|commit|push|pull|checkout|rebase|merge|log|diff|reset|clone|fetch|worktree)\b)/ },
    { id: "cli-flag", re: /(?:^|\s)--[a-z][a-z-]+/ },
    { id: "env-var", re: /\$[A-Z_]{3,}/ },
    { id: "raw-hex-id", re: /\b(?=[0-9a-f]*\d)(?=[0-9a-f]*[a-f])[0-9a-f]{4,40}\b/ },
    { id: "raw-uuid", re: /\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/i },
    { id: "raw-session-ref", re: /\b[a-z][a-z0-9-]*:[a-z][a-z0-9-]*\b/ },
  ];
  const ids = f5.forbiddenIds;

  // D8: a decision card shows the prompt's command as one mono line. Recorded, not counted.
  const isDecisionCommand = (el) => !!el.closest('[data-testid="decision-card"]') && el.hasAttribute("title") && el.classList.contains("font-mono");

  const check = (value, el, where) => {
    if (!value) return;
    for (const id of ids) {
      if (value.includes(id)) {
        (isDecisionCommand(el) ? exempt : findings).push({ check: "raw-id", string: id, context: value.slice(0, 160), where, selector: selectorOf(el) });
      }
    }
    for (const { id, re } of PATTERNS) {
      const m = value.match(re);
      if (!m) continue;
      // A colon pair inside a sentence ("Proxy errors 13:20-13:50") is a time, not a ref.
      if (id === "raw-session-ref" && /^\d/.test(m[0])) continue;
      const entry = { check: id, string: m[0].trim(), context: value.slice(0, 160), where, selector: selectorOf(el) };
      (isDecisionCommand(el) ? exempt : findings).push(entry);
    }
  };

  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const el = node.parentElement;
    if (!el || !visible(el) || /^(script|style|noscript)$/i.test(el.tagName)) continue;
    check(node.textContent.replace(/\s+/g, " ").trim(), el, "text");
  }
  for (const el of document.body.querySelectorAll("[aria-label],[title],[placeholder],[alt]")) {
    if (!visible(el)) continue;
    for (const attr of ["aria-label", "title", "placeholder", "alt"]) check(el.getAttribute(attr), el, `@${attr}`);
  }

  // One supervisor surface, and no phone tab bar (no Supervisor/Inbox/Worktrees tabs).
  const surfaces = [...document.querySelectorAll('[data-testid="supervisor-panel-surface"],[data-testid="setup-card"]')].filter(visible);
  if (surfaces.length !== 1) findings.push({ check: "second-surface:count", string: `${surfaces.length} panel surfaces`, selector: surfaces.map(selectorOf).join(" | ") || "body" });
  if (f5.surface === "phone") {
    for (const tab of [...document.querySelectorAll('[role="tab"],[role="tablist"]')].filter(visible)) {
      findings.push({ check: "second-surface:phone-tab", string: text(tab) || tab.getAttribute("aria-label"), selector: selectorOf(tab) });
    }
  }
  for (const el of [...document.querySelectorAll('[data-testid^="notification-center"],[data-testid*="inbox"],[data-testid*="lane"],[data-testid*="goal-card"],[data-testid*="audit"]')].filter(visible)) {
    findings.push({ check: "second-surface:element", string: el.getAttribute("data-testid"), selector: selectorOf(el) });
  }

  // Sections 3-5: which session each card/row stands for.
  const byTitle = new Map(f5.sessions.map((s) => [s.title, s]));
  const membership = [];
  for (const card of document.querySelectorAll('[data-testid="decision-card"]')) {
    const title = (card.getAttribute("aria-label") ?? "").replace(/^Decision for /, "");
    membership.push({ section: "Needs you", title, ref: byTitle.get(title)?.ref ?? null, selector: selectorOf(card) });
  }
  for (const row of document.querySelectorAll('[data-testid="handling-row"]')) {
    const title = text(row.querySelector("button span span"));
    membership.push({ section: "Handling", title, ref: byTitle.get(title)?.ref ?? null, selector: selectorOf(row) });
  }
  for (const row of document.querySelectorAll('[data-testid="session-row"]')) {
    const title = text(row.querySelector("span > span"));
    membership.push({ section: "Sessions", title, ref: byTitle.get(title)?.ref ?? null, glyph: row.getAttribute("data-state"), detail: text(row.querySelectorAll("span > span")[1]), selector: selectorOf(row) });
  }
  const seen = new Map();
  for (const m of membership) {
    const key = m.ref ?? `title:${m.title}`;
    const list = seen.get(key) ?? [];
    list.push(m);
    seen.set(key, list);
  }
  for (const [key, list] of seen) {
    if (list.length < 2) continue;
    const sections = [...new Set(list.map((m) => m.section))];
    findings.push({
      check: sections.length > 1 ? "duplicate:cross-section" : "duplicate:same-section",
      string: key,
      sections: list.map((m) => m.section),
      selector: list.map((m) => m.selector).join(" | "),
    });
  }

  // Decision cards: one per valid escalation, recommended option first, reason present.
  const cards = [...document.querySelectorAll('[data-testid="decision-card"]')];
  const valid = new Set(f5.decisions.filter((d) => d.valid).map((d) => d.ref)).size;
  if (f5.state !== "setup" && cards.length !== valid) findings.push({ check: "decision:count", string: `${cards.length} cards for ${valid} valid escalations`, selector: 'section[aria-label="Needs you"]' });
  for (const card of cards) {
    const buttons = [...card.querySelectorAll("button[data-recommended]")];
    if (buttons[0]?.getAttribute("data-recommended") !== "true") findings.push({ check: "decision:recommended-first", string: text(buttons[0]), selector: selectorOf(card) });
  }

  // Unknown state renders Idle with "status unknown", never Working.
  for (const s of f5.sessions.filter((x) => x.state === "unknown")) {
    const row = membership.find((m) => m.ref === s.ref && m.section === "Sessions");
    if (!row) findings.push({ check: "unknown-state:missing-row", string: s.title, selector: 'section[aria-label="Sessions"]' });
    else if (row.glyph !== "idle" || row.detail !== "status unknown") findings.push({ check: "unknown-state:working-fallback", string: `${s.title}: glyph=${row.glyph} detail=${row.detail}`, selector: row.selector });
  }

  return { findings, exempt, membership, surfaces: surfaces.length };
};

async function settle(page, state) {
  await page.waitForSelector('html[data-qa-ready="true"]', { timeout: 15_000 });
  await page.waitForSelector('[data-testid="supervisor-panel-surface"], [data-testid="setup-card"]', { timeout: 15_000 });
  // The desktop host reads the transcript after the snapshot; wait for it where one is seeded.
  if (state !== "setup") await page.waitForSelector('[data-testid="supervisor-transcript"]', { timeout: 15_000 });
  // Open the collapsed log ("All N") so every log line is scanned.
  const all = page.getByRole("button", { name: /^All \d+$/ });
  if ((await all.count()) > 0) await all.first().click();
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(undefined)))));
}

async function launch(chromium) {
  try {
    return { browser: await chromium.launch({ channel: "chrome", headless: true }), source: "channel:chrome" };
  } catch (error) {
    const bin = process.env.CHROME_BIN;
    if (!bin) throw error;
    return { browser: await chromium.launch({ headless: true, executablePath: bin }), source: `executablePath:${bin}` };
  }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (existsSync(args.evidenceDir) && spawnSync("ls", ["-A", args.evidenceDir], { encoding: "utf8" }).stdout.trim()) {
    console.error(`copy-audit: evidence dir is not empty: ${args.evidenceDir}`);
    process.exit(2);
  }
  mkdirSync(args.evidenceDir, { recursive: true });
  const report = {
    plan: "docs/plans/SUPERVISOR_AUTONOMY_PANEL_PLAN_2026-11-02.md F5",
    scriptRevision: gitRevision(repo),
    uiRevision: gitRevision(args.ui),
    startedAt: new Date().toISOString(),
    captures: [],
    consoleErrors: [],
    cleanup: [],
  };
  const outDir = mkdtempSync(join(tmpdir(), "ferryx-f5-dist-"));
  let server = null;
  let browser = null;
  let exitCode = 2;
  try {
    const built = build(args.ui, outDir);
    writeFileSync(join(args.evidenceDir, "vite-build.log"), built.log);
    if (built.status !== 0) throw new Error(`vite build failed (${built.status}); see vite-build.log`);
    server = await serve(outDir);
    const { port } = server.address();
    const { chromium } = loadPlaywright(args.ui);
    const launched = await launch(chromium);
    browser = launched.browser;
    report.browser = `${launched.source} ${browser.version()}`;
    for (const surface of SURFACES) {
      for (const viewport of VIEWPORTS) {
        for (const state of STATES) {
          const name = `${surface}-${viewport.name}-${state}`;
          const context = await browser.newContext({ viewport: { width: viewport.width, height: viewport.height }, colorScheme: "dark", hasTouch: surface === "phone" });
          const page = await context.newPage();
          await page.clock.setFixedTime(FIXTURE_NOW);
          page.on("console", (m) => { if (m.type() === "error") report.consoleErrors.push(`${name}: ${m.text()}`); });
          page.on("pageerror", (e) => report.consoleErrors.push(`${name}: pageerror ${e.message}`));
          const capture = { name, surface, width: viewport.width, state };
          try {
            await page.goto(`http://127.0.0.1:${port}/${HARNESS_HTML}?surface=${surface}&state=${state}`, { waitUntil: "load" });
            await settle(page, state);
            const scan = await page.evaluate(SCAN);
            capture.findings = scan.findings.map((f) => ({ ...f, surface, width: viewport.width, state }));
            capture.exempt = scan.exempt;
            capture.membership = scan.membership;
            capture.screenshot = `${name}.png`;
            await page.screenshot({ path: join(args.evidenceDir, capture.screenshot), fullPage: true });
          } catch (error) {
            capture.error = String(error?.stack ?? error);
            await page.screenshot({ path: join(args.evidenceDir, `${name}-error.png`), fullPage: true }).catch(() => undefined);
          }
          report.captures.push(capture);
          await context.close();
        }
      }
    }
    const errors = report.captures.filter((c) => c.error);
    exitCode = errors.length > 0 ? 2 : report.captures.some((c) => c.findings.length > 0) ? 1 : 0;
  } catch (error) {
    report.error = String(error?.stack ?? error);
  } finally {
    if (browser) { await browser.close(); report.cleanup.push("browser closed"); }
    if (server) { await new Promise((r) => server.close(r)); report.cleanup.push("http server closed"); }
    for (const file of [HARNESS_TSX, HARNESS_HTML]) { rmSync(join(args.ui, file), { force: true }); report.cleanup.push(`removed ${join(args.ui, file)}`); }
    rmSync(outDir, { recursive: true, force: true });
    report.cleanup.push(`removed ${outDir}`);
  }
  const findings = report.captures.flatMap((c) => c.findings ?? []);
  report.finishedAt = new Date().toISOString();
  report.gate = exitCode === 0 ? "PASS" : exitCode === 1 ? "FAIL" : "ERROR";
  report.summary = report.captures.map((c) => ({ name: c.name, findings: c.findings?.length ?? null, error: c.error ? c.error.split("\n")[0] : undefined }));
  writeFileSync(join(args.evidenceDir, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
  writeFileSync(join(args.evidenceDir, "findings.json"), `${JSON.stringify(findings, null, 2)}\n`);
  console.log(JSON.stringify({ gate: report.gate, uiRevision: report.uiRevision, findings: findings.length, summary: report.summary, consoleErrors: report.consoleErrors.length, error: report.error }, null, 2));
  process.exit(exitCode);
}

await main();
