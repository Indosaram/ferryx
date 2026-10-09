#!/usr/bin/env node
/**
 * scripts/qa/worktrees-design-audit.mjs
 *
 * Windows Node 24 Playwright (Edge) audit for the production RemoteApp account entry, mounted by
 * ui/account-worktrees-qa.html with the relay/tunnel replaced by ui/src/qa/mockAccountSession.ts.
 *
 * For each state (filled, loading, error, empty, offline) x viewport (375, 768, 1280):
 *   1. initial: collapsed top picker button, remote-account-empty-body with no children/text,
 *      no legacy worktree card, no chat, no terminal, no select POST.
 *   2. picker-open: state-specific picker content, still no select POST.
 * filled @ 768 and 1280 then clicks the exact worktree and checks the fake desktop recorded the
 * POST target and confirmed the workspace state before the terminal mounted.
 * Every HTTP request outside the harness origin, and every /api/ request over HTTP, fails the run.
 */

import { mkdirSync, writeFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { chromium } from "playwright-core";

const DEFAULT_HARNESS_URL = "http://localhost:5199/account-worktrees-qa.html";
const DEFAULT_OUT_DIR = "qa-evidence/worktrees-design-audit";
const EDGE_EXECUTABLE = process.env.EDGE_PATH || "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe";
const WAIT_MS = 15000;

const baseUrl = process.argv[2] || DEFAULT_HARNESS_URL;
const harnessOrigin = new URL(baseUrl).origin;
const outDir = resolve(process.argv[3] || DEFAULT_OUT_DIR);
mkdirSync(outDir, { recursive: true });

const VIEWPORTS = [
  { name: "mobile", width: 375, height: 667 },
  { name: "tablet", width: 768, height: 1024 },
  { name: "desktop", width: 1280, height: 800 },
];
const STATES = ["filled", "loading", "error", "empty", "offline"];
// At < 768px RemoteApp opens in chat mode; selection is exercised where the terminal body mounts.
const SELECT_VIEWPORTS = new Set(["tablet", "desktop"]);

const ALPHA = "mach-qa-alpha";
const TARGET = { workspaceId: "ws-ferryx", worktreeSlug: "feature-picker" };
const TARGET_LABEL = `${TARGET.workspaceId} / ${TARGET.worktreeSlug}`;
const MACHINE_NAMES = /QA Workstation Alpha|QA Linux Server|mach-qa-/;
const ERROR_TEXT = "QA relay unavailable (503)";

const summary = {
  timestamp: new Date().toISOString(),
  baseUrl,
  outDir,
  screenshots: [],
  assertions: [],
  passed: true,
  failures: [],
};

function check(description, passed, details = {}) {
  summary.assertions.push({ description, passed, ...details });
  if (!passed) {
    summary.passed = false;
    summary.failures.push(`${description}: ${JSON.stringify(details)}`);
  }
}

async function shot(page, vp, state, step) {
  const path = join(outDir, `${vp.name}-${state}-${step}.png`);
  await page.screenshot({ path, fullPage: true });
  summary.screenshots.push({ viewport: vp.name, state, step, path });
}

const qa = (page) => page.evaluate(() => {
  const r = window.__ferryxQa;
  return r ? JSON.parse(JSON.stringify(r)) : null;
});

async function selectPostCount(page) {
  const r = await qa(page);
  if (!r) throw new Error("window.__ferryxQa recorder missing: harness did not boot");
  return r.selectRequests.length;
}

async function assertPreselection(page, tag) {
  const trigger = page.getByRole("button", { name: "Change workspace context", exact: true });
  await trigger.waitFor({ state: "visible", timeout: WAIT_MS });
  check(`${tag}: top picker button collapsed`, (await trigger.getAttribute("aria-expanded")) === "false");

  const body = page.getByTestId("remote-account-empty-body");
  await body.waitFor({ state: "attached", timeout: WAIT_MS });
  const bodyShape = await body.evaluate((el) => ({ children: el.childElementCount, text: (el.textContent ?? "").trim() }));
  check(`${tag}: remote-account-empty-body has no children/text`, bodyShape.children === 0 && bodyShape.text === "", bodyShape);

  for (const testId of ["account-worktrees-container", "mobile-chat-workspace", "remote-terminal-grid", "remote-connection-badge"]) {
    check(`${tag}: no ${testId}`, (await page.getByTestId(testId).count()) === 0);
  }
  check(`${tag}: no legacy Worktrees heading`, (await page.getByRole("heading", { name: "Worktrees", exact: true }).count()) === 0);
  check(`${tag}: no select POST`, (await selectPostCount(page)) === 0);
  return trigger;
}

async function assertPickerState(page, state, tag) {
  const dialog = page.getByRole("dialog", { name: "Workspace context" });
  await dialog.waitFor({ state: "visible", timeout: WAIT_MS });
  const status = page.getByTestId("remote-account-inventory-status");
  const target = page.getByRole("button", { name: TARGET_LABEL, exact: true });

  if (state === "filled") {
    await target.waitFor({ state: "visible", timeout: WAIT_MS });
    check(`${tag}: exact worktree option visible`, true);
  } else if (state === "loading") {
    const loading = status.getByText("Loading worktrees...", { exact: true });
    await loading.waitFor({ state: "visible", timeout: WAIT_MS });
    check(`${tag}: loading row visible`, true);
  } else if (state === "error") {
    const alert = status.getByRole("alert");
    await alert.waitFor({ state: "visible", timeout: WAIT_MS });
    const text = (await alert.textContent())?.trim();
    check(`${tag}: error alert text`, text === ERROR_TEXT, { text });
  } else if (state === "empty" || state === "offline") {
    await page.waitForFunction(
      (fn) => window.__ferryxQa?.accountCalls.some((c) => c.fn === fn),
      "listMachines",
      { timeout: WAIT_MS },
    );
    await page.getByText("Loading worktrees...", { exact: true }).waitFor({ state: "detached", timeout: WAIT_MS });
  }

  const dialogText = (await dialog.textContent()) ?? "";
  check(`${tag}: no machine names or machine rows in picker`, !MACHINE_NAMES.test(dialogText), { dialogText });
  if (state !== "filled") {
    const options = await dialog.getByRole("button", { name: TARGET_LABEL, exact: true }).count();
    check(`${tag}: no selectable worktree options`, options === 0, { options });
  }
  check(`${tag}: no select POST while picker open`, (await selectPostCount(page)) === 0);
  const r = await qa(page);
  check(`${tag}: account calls bound to harness origin + seeded token`, r.violations.length === 0, { violations: r.violations });
}

async function selectExactWorktree(page, tag) {
  await page.getByRole("button", { name: TARGET_LABEL, exact: true }).click();
  await page.getByTestId("remote-terminal-grid").waitFor({ state: "visible", timeout: WAIT_MS });

  const r = await qa(page);
  const posts = r.selectRequests;
  check(`${tag}: exactly one select POST`, posts.length === 1, { posts });
  const post = posts[0];
  check(
    `${tag}: POST target is exact worktree on alpha`,
    post?.machineId === ALPHA && post?.body.workspaceId === TARGET.workspaceId && post?.body.worktreeSlug === TARGET.worktreeSlug,
    { post },
  );
  const active = r.hostStates[ALPHA]?.activeContext;
  check(
    `${tag}: fake desktop state confirms target`,
    active?.workspaceId === TARGET.workspaceId && active?.worktreeSlug === TARGET.worktreeSlug,
    { active },
  );
  const postIdx = r.tunnelFetches.findIndex((f) => f.path.startsWith("/api/v1/workspace/select"));
  const confirmIdx = r.tunnelFetches.findIndex((f, i) => i > postIdx && f.path.startsWith("/api/v1/workspace/state"));
  check(`${tag}: workspace state re-read after POST`, postIdx >= 0 && confirmIdx > postIdx, { postIdx, confirmIdx });
  const terminalSocket = r.socketsOpened.find((s) => s.path.startsWith(`/api/v1/terminal/qa-sess-${TARGET.workspaceId}-${TARGET.worktreeSlug}`));
  check(`${tag}: terminal socket opened for confirmed session`, Boolean(terminalSocket), { sockets: r.socketsOpened });
  const context = (await page.getByLabel("Current desktop context").textContent())?.trim();
  check(`${tag}: top picker shows confirmed context`, context === `${TARGET.workspaceId} / ${TARGET.worktreeSlug}`, { context });
  check(`${tag}: no mock violations`, r.violations.length === 0, { violations: r.violations });
}

console.log(`[audit] Launching Edge: ${EDGE_EXECUTABLE}`);
const browser = await chromium.launch({
  executablePath: EDGE_EXECUTABLE,
  headless: true,
  args: [
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-background-networking",
    "--disable-features=Translate,OptimizationHints,MediaRouter",
  ],
});

try {
  for (const vp of VIEWPORTS) {
    for (const state of STATES) {
      const tag = `${vp.name} [${state}]`;
      const pageErrors = [];
      const blocked = [];
      const context = await browser.newContext({
        viewport: { width: vp.width, height: vp.height },
        serviceWorkers: "block",
      });
      await context.route("**/*", (route) => {
        const url = new URL(route.request().url());
        if (url.origin === harnessOrigin && !url.pathname.startsWith("/api/")) return route.continue();
        blocked.push(`${route.request().method()} ${url.href}`);
        return route.abort("blockedbyclient");
      });
      const page = await context.newPage();
      page.on("pageerror", (err) => pageErrors.push(err.message || String(err)));
      page.on("websocket", (ws) => {
        const wsUrl = new URL(ws.url());
        // Vite's own dev-client socket lives at the harness root; any app socket is a real-network leak.
        if (wsUrl.host === new URL(harnessOrigin).host && wsUrl.pathname === "/") return;
        blocked.push(`WS ${ws.url()}`);
      });

      try {
        const url = new URL(baseUrl);
        url.searchParams.set("state", state);
        await page.goto(url.toString(), { waitUntil: "domcontentloaded" });

        const trigger = await assertPreselection(page, `${tag} initial`);
        check(`${tag} initial: no horizontal overflow`, !(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth)));
        await shot(page, vp, state, "initial");

        await trigger.click();
        check(`${tag}: picker expanded`, (await trigger.getAttribute("aria-expanded")) === "true");
        await assertPickerState(page, state, `${tag} picker-open`);
        check(`${tag} picker-open: body still empty`, (await page.getByTestId("remote-account-empty-body").evaluate((el) => el.childElementCount === 0 && !(el.textContent ?? "").trim())));
        await shot(page, vp, state, "picker-open");

        if (state === "filled" && SELECT_VIEWPORTS.has(vp.name)) {
          await selectExactWorktree(page, `${tag} selected`);
          await shot(page, vp, state, "selected");
        }
      } catch (err) {
        check(`${tag}: scenario completed`, false, { error: err instanceof Error ? err.message : String(err) });
        await shot(page, vp, state, "failure").catch(() => {});
      }

      check(`${tag}: no uncaught page errors`, pageErrors.length === 0, { pageErrors });
      check(`${tag}: no real network / desktop requests`, blocked.length === 0, { blocked });
      await context.close();
    }
  }
} catch (runError) {
  check("Runner execution completed without uncaught throw", false, {
    error: runError instanceof Error ? runError.message : String(runError),
    stack: runError instanceof Error ? runError.stack : undefined,
  });
} finally {
  await browser.close().catch(() => {});

  writeFileSync(join(outDir, "audit-summary.json"), JSON.stringify(summary, null, 2), "utf8");
  const md = `# RemoteApp Account Picker Audit

Date: ${summary.timestamp}
Base URL: ${summary.baseUrl}
Overall Verdict: ${summary.passed ? "PASSED" : "FAILED"}

## Screenshots
${summary.screenshots.map((s) => `- ${s.viewport} [${s.state}] ${s.step}: ${s.path}`).join("\n")}

## Assertions
| Check | Status | Details |
|---|---|---|
${summary.assertions.map((a) => `| ${a.description} | ${a.passed ? "PASS" : "FAIL"} | ${a.passed ? "OK" : JSON.stringify(a).replace(/\|/g, "\\|")} |`).join("\n")}
${summary.failures.length > 0 ? `\n## Failures\n${summary.failures.map((f) => `- ${f}`).join("\n")}\n` : ""}`;
  writeFileSync(join(outDir, "audit-report.md"), md, "utf8");
}

console.log(`\n[audit] Results: ${join(outDir, "audit-summary.json")}, ${join(outDir, "audit-report.md")}`);
if (!summary.passed) {
  console.error(`[audit] FAILED: ${summary.failures.length} check(s).`);
  process.exitCode = 1;
} else {
  console.log(`[audit] ALL ${summary.assertions.length} CHECKS PASSED.`);
}
