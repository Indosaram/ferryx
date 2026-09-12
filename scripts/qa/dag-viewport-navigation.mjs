#!/usr/bin/env node
/**
 * Real-host DAG viewport navigation QA runner.
 *
 * Task 1 scope (.omo/plans/dag-viewport-navigation.md): `--phase baseline` only.
 * It opens the real dag-viewport-qa.html harness in a dedicated real Chrome
 * context, proves graph/card/edge/fixture existence first, then performs the
 * exact plan S1/S2 real input sequences (Chrome mouse.move/down/move/up and
 * page.mouse.wheel) and records that pan/zoom behavior is absent (faithful RED)
 * — never selector/setup failure.
 *
 * `--phase green` and scenarios S3-S6 belong to task 2/3 and exit nonzero here.
 * Invalid arguments exit nonzero. The runner owns and closes its isolated Chrome
 * context and nothing else: server lifetime is owned by the invoking
 * monitor/session, which starts the Vite dev server on 127.0.0.1:5193 before
 * invoking this runner. This runner drives browser input only and is explicitly
 * NOT native/desktop proof.
 */
import { createRequire } from "node:module";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repo = resolve(scriptDir, "..", "..");
const DEFAULT_URL = "http://127.0.0.1:5193/dag-viewport-qa.html";
const PAGE_TIMEOUT_MS = 20000;

const delay = (ms) => new Promise((r) => setTimeout(r, ms));

function fail(exitCode, message) {
  process.stderr.write(`dag-viewport-navigation runner: ${message}\n`);
  process.exit(exitCode);
}

function parseArgs(argv) {
  const parsed = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--phase") parsed.phase = argv[++i];
    else if (a === "--host") parsed.host = argv[++i];
    else if (a === "--evidence-dir") parsed.evidenceDir = argv[++i];
    else if (a === "--url") parsed.url = argv[++i];
    else if (a === "--help" || a === "-h") parsed.help = true;
    else return { error: `unrecognized argument: ${a}` };
  }
  return parsed;
}

const parsed = parseArgs(process.argv.slice(2));
if (parsed && parsed.error) fail(2, `invalid arguments: ${parsed.error}`);
if (!parsed || parsed.help || !parsed.phase || !parsed.host || !parsed.evidenceDir) {
  process.stdout.write(
    "Usage: bun scripts/qa/dag-viewport-navigation.mjs --phase baseline|green " +
    "--host modal|standalone|both --evidence-dir <directory> [--url <qa page url>]\n" +
    "Server lifetime is owned by the invoking monitor; this runner owns only its Chrome context.\n" +
    "Task 1 ships baseline only; --phase green exits nonzero until task 2.\n",
  );
  process.exit(!parsed || parsed.error || (!parsed.help && (!parsed.phase || !parsed.host || !parsed.evidenceDir)) ? 2 : 0);
}
const { phase, host, evidenceDir, url = DEFAULT_URL } = parsed;

if (phase !== "baseline" && phase !== "green") {
  fail(2, "invalid arguments: --phase must be 'baseline' or 'green'");
}
if (phase === "green") {
  fail(2, "--phase green is not part of task 1: the baseline-only runner ships with wave 1; " +
    "green-phase scenarios (S3-S6) land with task 2 per .omo/plans/dag-viewport-navigation.md");
}
if (host !== "modal" && host !== "standalone" && host !== "both") {
  fail(2, "invalid arguments: --host must be 'modal', 'standalone' or 'both'");
}

function resolvePlaywrightCore() {
  try {
    return { pw: createRequire(join(repo, "package.json"))("playwright-core"), source: "repo node_modules" };
  } catch { /* fall through */ }
  const globalCandidate = "/Users/indo/.bun/install/global/node_modules/playwright-core";
  try {
    return { pw: createRequire(join(repo, "package.json"))(globalCandidate), source: globalCandidate };
  } catch { /* fall through */ }
  fail(2, "playwright-core is unavailable; set PLAYWRIGHT_CORE_PATH to an existing playwright-core package directory");
}

function parseServerCommandSpec() { /* removed: the runner never owns the server */ }
void parseServerCommandSpec;

const SCOPE = {
  modal: '[data-testid="dag-pane-modal"]',
  standalone: '[data-testid="qa-standalone-host"]',
};

function check(id, kind, expectation, pass, detail) {
  return { id, kind, expectation, pass: Boolean(pass), ...(detail !== undefined ? { detail } : {}) };
}

function delta(before, after) {
  return { x: +(after.x - before.x).toFixed(2), y: +(after.y - before.y).toFixed(2) };
}

function movedBy(d, x, y, tol = 1) {
  return Math.abs(d.x - x) <= tol && Math.abs(d.y - y) <= tol;
}

function isIdentityTransform(t) {
  return t === "none" || t === "matrix(1, 0, 0, 1, 0, 0)";
}

async function settle(page) {
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
}

async function captureGeometry(page, host, cardTestId, edgeTestId) {
  const scopeSel = SCOPE[host];
  return page.evaluate(({ scopeSel, cardTestId, edgeTestId }) => {
    const root = document.querySelector(scopeSel);
    if (!root) return null;
    const graphView = root.querySelector('[data-testid="dag-graph-view"]');
    if (!graphView) return null;
    const header = graphView.querySelector('[data-testid="dag-header"]');
    const svg = graphView.querySelector('svg[data-testid="dag-edge-layer"]');
    const card = graphView.querySelector(`[data-testid="${cardTestId}"]`);
    const path = svg ? svg.querySelector(`[data-testid="${edgeTestId}"]`) : null;
    if (!header || !svg || !card || !path) return null;
    const world = svg.parentElement;
    const viewport = world.parentElement;
    const rect = (el) => {
      const r = el.getBoundingClientRect();
      return { x: +r.x.toFixed(2), y: +r.y.toFixed(2), width: +r.width.toFixed(2), height: +r.height.toFixed(2) };
    };
    const len = path.getTotalLength();
    const mid = path.getPointAtLength(len / 2);
    const ctm = path.getScreenCTM();
    if (!ctm) return null;
    const midScreen = new DOMPoint(mid.x, mid.y).matrixTransform(ctm);
    return {
      header: rect(header),
      card: rect(card),
      edgeEndpoint: { x: +midScreen.x.toFixed(2), y: +midScreen.y.toFixed(2) },
      ctmScale: +ctm.a.toFixed(6),
      ctmTranslate: { e: +ctm.e.toFixed(3), f: +ctm.f.toFixed(3) },
      worldTransform: getComputedStyle(world).transform,
      viewport: rect(viewport),
      world: rect(world),
      scroll: { top: viewport.scrollTop, left: viewport.scrollLeft },
      overflow: { w: viewport.scrollWidth - viewport.clientWidth, h: viewport.scrollHeight - viewport.clientHeight },
      document: {
        scrollX: window.scrollX,
        scrollY: window.scrollY,
        vvScale: window.visualViewport ? window.visualViewport.scale : null,
      },
    };
  }, { scopeSel, cardTestId, edgeTestId });
}

async function elementFromPointLabel(page, x, y) {
  return page.evaluate(([px, py]) => {
    const el = document.elementFromPoint(px, py);
    if (!el) return "none";
    const tid = el.getAttribute("data-testid");
    return `${el.tagName.toLowerCase()}${tid ? `/${tid}` : ""}`;
  }, [x, y]);
}

async function openFixture(page, host, fixture) {
  // Every host run starts from a fresh page load; the harness re-seeds the real
  // dagStore on mount, so fixture A/B ownership state is deterministic.
  await page.goto(url, { waitUntil: "load" });
  if (host === "modal") {
    const badge = page.locator('[data-testid="qa-modal-host"] [data-testid="dag-pane-badge-button"]');
    await badge.waitFor({ state: "visible", timeout: PAGE_TIMEOUT_MS });
    await badge.click();
  }
  const scopeSel = SCOPE[host];
  await page.locator(`${scopeSel} [data-testid="dag-graph-view"][data-run-id="${fixture.runId}"]`)
    .waitFor({ state: "visible", timeout: PAGE_TIMEOUT_MS });
  await page.locator(`${scopeSel} [data-testid="dag-header"]`).waitFor({ state: "visible", timeout: PAGE_TIMEOUT_MS });
  await page.locator(`${scopeSel} [data-testid="dag-edge-layer"]`).waitFor({ state: "visible", timeout: PAGE_TIMEOUT_MS });
  await page.locator(`${scopeSel} [data-testid="${fixture.card}"]`).waitFor({ state: "visible", timeout: PAGE_TIMEOUT_MS });
  await page.locator(`${scopeSel} [data-testid="${fixture.edge}"]`).waitFor({ state: "visible", timeout: PAGE_TIMEOUT_MS });
}

const FIXTURE_A = { runId: "qa-dag-a", card: "dag-node-a", edge: "dag-edge-a-c" };
const FIXTURE_TALL = { runId: "qa-dag-tall", card: "dag-node-t0", edge: "dag-edge-t0-t1" };
const FIXTURE_BIG = { runId: "qa-dag-big", card: "dag-node-n0", edge: "dag-edge-n0-n1" };

/**
 * The QA monitor serves 5193 (per plan), while ui/vite.config.ts pins the HMR
 * client to ws://127.0.0.1:5173 for the Tauri dev flow. On any other port the
 * Vite dev client logs a refused HMR websocket and an unhandled error — dev
 * infrastructure noise, not a defect of the page under test. These entries are
 * separated into `environmentNoise` (raw text preserved) and excluded from the
 * page-error verdict. Everything else still fails the run.
 */
function isViteHmrClientNoise(message) {
  return message.includes("ws://127.0.0.1:5173")
    || message.includes("[vite] failed to connect to websocket")
    || message.includes("WebSocket closed without opened");
}

async function runHost(browser, host, evidencePaths) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, deviceScaleFactor: 1 });
  const page = await context.newPage();
  const consoleErrors = [];
  const pageErrors = [];
  const actions = [];
  const checks = [];
  const skipNotes = [];
  const screenshots = [];
  let scenarioError = null;
  try {
    page.on("console", (msg) => {
      if (msg.type() === "error") consoleErrors.push({ text: msg.text(), at: new Date().toISOString() });
    });
    page.on("pageerror", (err) => pageErrors.push({ message: String(err?.message ?? err), at: new Date().toISOString() }));

    await openFixture(page, host, FIXTURE_A);
    checks.push(check("existence-real-graph-card-edge", "setup", "pass", true,
      { host, graphRunId: "qa-dag-a", card: FIXTURE_A.card, edge: FIXTURE_A.edge }));

    const geo0 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    if (!geo0) throw new Error("geometry capture returned null after existence proof");
    checks.push(check("existence-viewport-positive-rect", "setup", "pass",
      geo0.viewport.width > 0 && geo0.viewport.height > 0, { viewport: geo0.viewport }));

    const shot = async (name) => {
      const path = join(evidenceDir, name);
      await page.screenshot({ path });
      screenshots.push(name);
      actions.push({ action: "screenshot", name });
      evidencePaths.push(name);
      return name;
    };
    await shot(`${host}-initial.png`);

    // ---- S1: real Chrome drag on the background, expected delta (120, 80) ----
    const vp = geo0.viewport;
    const startX = vp.x + 100;
    const startY = vp.y + 100;
    const startTarget = await elementFromPointLabel(page, startX, startY);
    actions.push({ action: "mouse.move", x: startX, y: startY, target: startTarget });
    await page.mouse.move(startX, startY);
    actions.push({ action: "mouse.down", button: "left" });
    await page.mouse.down();
    actions.push({ action: "mouse.move", x: startX + 120, y: startY + 80, steps: 8 });
    await page.mouse.move(startX + 120, startY + 80, { steps: 8 });
    actions.push({ action: "mouse.up" });
    await page.mouse.up();
    await settle(page);
    const geo1 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    await shot(`${host}-after-pan.png`);

    const cardDelta = delta(geo0.card, geo1.card);
    const edgeDelta = delta(geo0.edgeEndpoint, geo1.edgeEndpoint);
    const headerDelta = delta(geo0.header, geo1.header);
    checks.push(check("S1-pan-card-moves-120-80", "behavioral-red", "fail-red",
      movedBy(cardDelta, 120, 80),
      { assertion: `card ${FIXTURE_A.card} must move by (120,80) within 1 CSS px after left-button drag from background`, expected: { x: 120, y: 80 }, actual: cardDelta }));
    checks.push(check("S1-pan-edge-endpoint-moves-120-80", "behavioral-red", "fail-red",
      movedBy(edgeDelta, 120, 80),
      { assertion: `edge ${FIXTURE_A.edge} getScreenCTM endpoint must move by (120,80) within 1 CSS px after the same drag`, expected: { x: 120, y: 80 }, actual: edgeDelta }));
    checks.push(check("S1-pan-header-unchanged", "behavioral", "pass",
      movedBy(headerDelta, 0, 0, 0.5), { actual: headerDelta }));
    checks.push(check("S1-pan-scroll-unchanged", "observation", "pass",
      geo1.scroll.top === geo0.scroll.top && geo1.scroll.left === geo0.scroll.left,
      { before: geo0.scroll, after: geo1.scroll }));
    checks.push(check("S1-pan-cards-and-edges-aligned", "behavioral", "pass",
      Math.abs(cardDelta.x - edgeDelta.x) <= 1 && Math.abs(cardDelta.y - edgeDelta.y) <= 1,
      { cardDelta, edgeDelta }));

    // ---- S1 variant: drag starting on a noninteractive node card ----
    const cardCx = geo1.card.x + geo1.card.width / 2;
    const cardCy = geo1.card.y + geo1.card.height / 2;
    const cardTarget = await elementFromPointLabel(page, cardCx, cardCy);
    actions.push({ action: "mouse.move", x: cardCx, y: cardCy, target: cardTarget });
    await page.mouse.move(cardCx, cardCy);
    await page.mouse.down();
    actions.push({ action: "mouse.move", x: cardCx + 120, y: cardCy + 80, steps: 8 });
    await page.mouse.move(cardCx + 120, cardCy + 80, { steps: 8 });
    actions.push({ action: "mouse.up" });
    await page.mouse.up();
    await settle(page);
    const geo2 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    const cardDelta2 = delta(geo1.card, geo2.card);
    checks.push(check("S1-pan-from-card-moves-120-80", "behavioral-red", "fail-red",
      movedBy(cardDelta2, 120, 80),
      { assertion: `card ${FIXTURE_A.card} must move by (120,80) within 1 CSS px when the drag starts on the card itself`, expected: { x: 120, y: 80 }, actual: cardDelta2, startTarget: cardTarget }));
    skipNotes.push("S1 repeats at 0.5x/2x require camera controls that do not exist in current code; the S1 primary sequences above are the baseline RED for that absence.");

    // ---- S2: wheel zoom in then out at the viewport center ----
    const centerX = vp.x + vp.width / 2;
    const centerY = vp.y + vp.height / 2;
    const gz0 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    actions.push({ action: "mouse.move", x: centerX, y: centerY, target: await elementFromPointLabel(page, centerX, centerY) });
    await page.mouse.move(centerX, centerY);
    actions.push({ action: "mouse.wheel", deltaX: 0, deltaY: -120 });
    await page.mouse.wheel(0, -120);
    await settle(page);
    const gz1 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    await shot(`${host}-after-wheel-in.png`);
    actions.push({ action: "mouse.wheel", deltaX: 0, deltaY: 120 });
    await page.mouse.wheel(0, 120);
    await settle(page);
    const gz2 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    await shot(`${host}-after-wheel-out.png`);

    checks.push(check("S2-zoom-in-increases-scale", "behavioral-red", "fail-red",
      gz1.ctmScale > gz0.ctmScale + 1e-6,
      { assertion: "wheel(0,-120) at viewport center must increase graph scale (getScreenCTM().a)", expected: `> ${gz0.ctmScale}`, actual: gz1.ctmScale }));
    checks.push(check("S2-zoom-out-returns-scale", "behavioral", "pass",
      Math.abs(gz2.ctmScale - gz0.ctmScale) <= 1e-6,
      { note: "vacuous in baseline: no zoom happened at all (expected RED is the increase assertion above)", expected: gz0.ctmScale, actual: gz2.ctmScale }));
    checks.push(check("S2-zoom-anchor-world-point-stable", "behavioral", "pass",
      movedBy(delta(gz0.edgeEndpoint, gz2.edgeEndpoint), 0, 0, 1),
      { note: "vacuous in baseline (no camera movement); anchor preservation becomes meaningful with the task 2 camera", before: gz0.edgeEndpoint, after: gz2.edgeEndpoint }));
    checks.push(check("S2-document-and-visual-viewport-unchanged", "behavioral", "pass",
      gz1.document.scrollY === 0 && (gz1.document.vvScale === null || gz1.document.vvScale === 1),
      { afterWheelIn: gz1.document }));

    // ---- S2: horizontal-only wheel must not be a camera gesture ----
    const gh0 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    actions.push({ action: "mouse.wheel", deltaX: 120, deltaY: 0 });
    await page.mouse.wheel(120, 0);
    await settle(page);
    const gh1 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S2-horizontal-wheel-camera-unchanged", "behavioral", "pass",
      gh1.ctmScale === gh0.ctmScale && gh0.worldTransform === gh1.worldTransform && isIdentityTransform(gh0.worldTransform),
      { note: "camera invariant = scale unchanged and world transform stays identity; scroll routing is recorded separately because incumbent scroll legitimately moves screen rects", scaleBefore: gh0.ctmScale, scaleAfter: gh1.ctmScale, worldTransform: gh1.worldTransform }));
    checks.push(check("observation-horizontal-wheel-scroll-delta", "observation", "pass",
      true, { detail: "recorded incumbent scroll routing for horizontal wheel", scrollDelta: delta({ x: gh0.scroll.left, y: gh0.scroll.top }, { x: gh1.scroll.left, y: gh1.scroll.top }), overflow: gh0.overflow }));
    skipNotes.push("S2 deltaMode 0/1/2 and Ctrl variants and at-limits invariants require the camera implementation; they are unit/component cases for task 2, not baseline browser scenarios.");

    // ---- Incumbent scroll observation (standalone host): the reported scrolling
    // problem, separated from the absent camera controls. ----
    if (host === "standalone") {
      await page.locator('[data-testid="qa-load-tall"]').click();
      await page.locator(`${SCOPE.standalone} [data-testid="dag-node-t0"]`).waitFor({ state: "visible", timeout: PAGE_TIMEOUT_MS });
      await settle(page);
      const gt0 = await captureGeometry(page, host, FIXTURE_TALL.card, FIXTURE_TALL.edge);
      await page.mouse.move(centerX, centerY);
      actions.push({ action: "mouse.wheel", deltaX: 0, deltaY: 500, fixture: "tall" });
      await page.mouse.wheel(0, 500);
      await settle(page);
      const gt1 = await captureGeometry(page, host, FIXTURE_TALL.card, FIXTURE_TALL.edge);
      checks.push(check("incumbent-scroll-vertical-works", "observation", "pass",
        gt1.scroll.top - gt0.scroll.top > 0,
        { before: gt0.scroll, after: gt1.scroll, overflow: gt0.overflow }));
      await shot("standalone-scroll-tall.png");

      await page.locator('[data-testid="qa-load-big"]').click();
      await page.locator(`${SCOPE.standalone} [data-testid="dag-node-n0"]`).waitFor({ state: "visible", timeout: PAGE_TIMEOUT_MS });
      await settle(page);
      const gb0 = await captureGeometry(page, host, FIXTURE_BIG.card, FIXTURE_BIG.edge);
      await page.mouse.move(centerX, centerY);
      actions.push({ action: "mouse.wheel", deltaX: 600, deltaY: 0, fixture: "big" });
      await page.mouse.wheel(600, 0);
      await settle(page);
      const gb1 = await captureGeometry(page, host, FIXTURE_BIG.card, FIXTURE_BIG.edge);
      checks.push(check("incumbent-scroll-horizontal-works", "observation", "pass",
        gb1.scroll.left - gb0.scroll.left > 0,
        { before: gb0.scroll, after: gb1.scroll, overflow: gb0.overflow }));
      await shot("standalone-scroll-big.png");
    }
  } catch (e) {
    scenarioError = String(e?.stack ?? e);
    checks.push(check("harness-error", "setup", "pass", false, { error: scenarioError }));
    try {
      const errorShot = join(evidenceDir, `${host}-error.png`);
      await page.screenshot({ path: errorShot });
      screenshots.push(`${host}-error.png`);
    } catch { /* best effort */ }
  } finally {
    await context.close();
  }
  const pageErrorNoise = pageErrors.filter((e) => isViteHmrClientNoise(e.message));
  const pageErrorReal = pageErrors.filter((e) => !isViteHmrClientNoise(e.message));
  return { checks, actions, consoleErrors, pageErrors: pageErrorReal, environmentNoise: {
    reason: "ui/vite.config.ts pins hmr clientPort 5173; the QA monitor serves 5193, so the Vite dev client's HMR websocket is refused. Dev-client noise, not page behavior. Raw entries preserved below.",
    pageErrors: pageErrorNoise,
    consoleErrors: consoleErrors.filter((e) => isViteHmrClientNoise(e.text)),
  }, screenshots, skipNotes, scenarioError };
}

function summarize(results, hosts) {
  const expectedRed = [];
  const observedRed = [];
  const missingSetup = [];
  const errors = [];
  for (const h of hosts) {
    const data = results.hosts[h];
    if (!data) continue;
    for (const c of data.checks) {
      if (c.expectation === "fail-red") {
        expectedRed.push(`${h}:${c.id}`);
        if (!c.pass) observedRed.push(`${h}:${c.id}`);
      }
      if (c.kind === "setup" && !c.pass) missingSetup.push(`${h}:${c.id}`);
    }
    if (data.scenarioError) errors.push(`${h}: ${data.scenarioError}`);
    if (data.pageErrors.length > 0) errors.push(`${h} pageErrors: ${JSON.stringify(data.pageErrors)}`);
  }
  return {
    expectedRed,
    observedRed,
    redConfirmed: expectedRed.length > 0 && observedRed.length === expectedRed.length,
    missingSetup,
    errors,
    ok: missingSetup.length === 0 && errors.length === 0 && expectedRed.length > 0 && observedRed.length === expectedRed.length,
  };
}

// ---------------------------------------------------------------- main ----
mkdirSync(evidenceDir, { recursive: true });
const hosts = host === "both" ? ["modal", "standalone"] : [host];
const actions = [];

const { pw, source: pwSource } = resolvePlaywrightCore();
let browser;
let browserSource;
const chromeBin = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
try {
  browser = await pw.chromium.launch({ channel: "chrome", headless: true });
  browserSource = "channel:chrome (real Google Chrome, headless)";
} catch (channelError) {
  if (!existsSync(chromeBin)) fail(1, `Google Chrome channel unavailable and ${chromeBin} missing: ${String(channelError)}`);
  browser = await pw.chromium.launch({ headless: true, executablePath: chromeBin });
  browserSource = `executablePath:${chromeBin} (headless)`;
}

const evidencePaths = [];
const results = {
  phase,
  runner: { script: "scripts/qa/dag-viewport-navigation.mjs", repo, startedAt: new Date().toISOString() },
  browser: {
    family: "Chrome",
    source: browserSource,
    playwrightCore: pwSource,
    input: "Playwright CDP real-input mouse/wheel on a dedicated real Chrome context",
    nativeProof: false,
    note: "Browser-only evidence; never counts as native/desktop proof (see .omo/evidence/dag-viewport-navigation/native/).",
  },
  server: { url, note: "not owned by this runner; invoking monitor/session owns the Vite server" },
  hosts: {},
};
let exitCode = 1;
try {
  for (const h of hosts) {
    results.hosts[h] = await runHost(browser, h, evidencePaths);
  }
  results.redSummary = summarize(results, hosts);
  results.screenshots = evidencePaths;
  results.finishedAt = new Date().toISOString();
  writeFileSync(join(evidenceDir, "results.json"), JSON.stringify(results, null, 2));
  writeFileSync(join(evidenceDir, "actions.json"), JSON.stringify(
    { generatedAt: new Date().toISOString(), byHost: Object.fromEntries(hosts.map((h) => [h, results.hosts[h]?.actions ?? []])), runner: actions },
    null, 2,
  ));
  const summary = results.redSummary;
  for (const line of summary.observedRed) {
    process.stdout.write(`RED-CONFIRMED ${line}\n`);
  }
  for (const line of summary.expectedRed.filter((id) => !summary.observedRed.includes(id))) {
    process.stdout.write(`RED-MISSING ${line}\n`);
  }
  for (const line of summary.missingSetup) process.stdout.write(`SETUP-FAILED ${line}\n`);
  for (const line of summary.errors) process.stdout.write(`ERROR ${line}\n`);
  process.stdout.write(`summary: setup=${summary.missingSetup.length === 0 ? "ok" : "failed"} redConfirmed=${summary.redConfirmed} hosts=${hosts.join(",")}\n`);
  exitCode = summary.ok ? 0 : 1;
} catch (e) {
  results.fatal = String(e?.stack ?? e);
  results.finishedAt = new Date().toISOString();
  writeFileSync(join(evidenceDir, "results.json"), JSON.stringify(results, null, 2));
  process.stdout.write(`FATAL ${results.fatal}\n`);
  exitCode = 1;
} finally {
  try {
    await browser.close();
    actions.push({ action: "browser.contexts-closed" });
  } catch (e) {
    actions.push({ action: "browser.close failed", error: String(e) });
  }
  writeFileSync(join(evidenceDir, "actions.json"), JSON.stringify(
    { generatedAt: new Date().toISOString(), byHost: Object.fromEntries(hosts.map((h) => [h, results.hosts[h]?.actions ?? []])), runner: actions },
    null, 2,
  ));
}

// ---- cleanup receipt (S8): only resources this run owns ----
writeFileSync(join(evidenceDir, "cleanup.md"), [
  "# DAG viewport QA runner cleanup receipt",
  "",
  `- generated: ${new Date().toISOString()}`,
  `- phase: ${phase}, host: ${host}`,
  "- Chrome: dedicated real Chrome context per host; all contexts closed (browser.close()).",
  "- Server: not owned by this run; the invoking monitor/session owns the Vite server on 127.0.0.1:5193 and performs its own teardown (lsof receipt in .omo/evidence).",
  "- Temp QA resources: none created besides the evidence directory contents.",
  "",
].join("\n"));
process.exit(exitCode);
