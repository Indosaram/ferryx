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

function fitExpected(vp, worldW, worldH) {
  const margin = Math.min(24, vp.width / 4, vp.height / 4);
  const fitScale = Math.min(1, (vp.width - 2 * margin) / worldW, (vp.height - 2 * margin) / worldH);
  const x = (vp.width - worldW * fitScale) / 2;
  const y = (vp.height - worldH * fitScale) / 2;
  return { x: +x.toFixed(2), y: +y.toFixed(2), scale: +fitScale.toFixed(6) };
}

function fitOk(geo, worldW, worldH) {
  if (!geo || !geo.viewport || !geo.camera) return false;
  const w = worldW ?? geo.contentWidth ?? 1160;
  const h = worldH ?? geo.contentHeight ?? 284;
  const expected = fitExpected(geo.viewport, w, h);
  return (
    Math.abs(geo.ctmScale - expected.scale) <= 0.02 &&
    Math.abs(geo.camera.x - expected.x) <= 3 &&
    Math.abs(geo.camera.y - expected.y) <= 3
  );
}

function allInside(geo) {
  if (!geo || !geo.viewport) return false;
  const vp = geo.viewport;
  const allPoints = [
    ...(geo.cards || []).map((c) => ({ x: c.x, y: c.y })),
    ...(geo.cards || []).map((c) => ({ x: c.x + c.width, y: c.y + c.height })),
    ...(geo.endpoints || []),
  ];
  if (allPoints.length === 0) return true;
  return allPoints.every(
    (p) =>
      p.x >= vp.x - 2 &&
      p.x <= vp.x + vp.width + 2 &&
      p.y >= vp.y - 2 &&
      p.y <= vp.y + vp.height + 2,
  );
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
    const card = cardTestId ? graphView.querySelector(`[data-testid="${cardTestId}"]`) : null;
    const path = svg && edgeTestId ? svg.querySelector(`[data-testid="${edgeTestId}"]`) : null;
    if (!header || !svg) return null;
    const world = svg.parentElement;
    const viewport = world.parentElement;
    const rect = (el) => {
      const r = el.getBoundingClientRect();
      return { x: +r.x.toFixed(2), y: +r.y.toFixed(2), width: +r.width.toFixed(2), height: +r.height.toFixed(2) };
    };
    let edgeEndpoint = { x: 0, y: 0 };
    if (path) {
      const len = path.getTotalLength();
      const end = path.getPointAtLength(len);
      const ctm = path.getScreenCTM();
      if (ctm) {
        const endScreen = new DOMPoint(end.x, end.y).matrixTransform(ctm);
        edgeEndpoint = { x: +endScreen.x.toFixed(2), y: +endScreen.y.toFixed(2) };
      }
    }
    const worldMatrix = new DOMMatrix(getComputedStyle(world).transform);
    const paths = [...world.querySelectorAll('[data-testid^="dag-edge-"] path[data-testid]')];
    const endpoints = paths.flatMap((p) => {
      const ctm = p.getScreenCTM();
      if (!ctm) return [];
      return [0, p.getTotalLength()].map((t) => {
        const pt = p.getPointAtLength(t);
        const a = new DOMPoint(pt.x, pt.y).matrixTransform(ctm);
        return { x: +a.x.toFixed(2), y: +a.y.toFixed(2) };
      });
    });
    const cards = [...world.children].filter((e) => e.matches('[data-testid^="dag-node-"]')).map(rect);
    const leaves = [...document.querySelectorAll('[data-testid="qa-standalone-host"] [data-testid="pane-leaf"]')].map((e) => ({
      id: e.getAttribute("data-leaf-id"),
      rect: rect(e),
    }));
    const seam = document.querySelector('[role="separator"]')?.getAttribute("aria-valuenow") ?? null;
    const outBtn = root.querySelector('[aria-label="Zoom out"]');
    const inBtn = root.querySelector('[aria-label="Zoom in"]');

    return {
      header: rect(header),
      card: card ? rect(card) : null,
      cards,
      endpoints,
      edgeEndpoint,
      ctmScale: +worldMatrix.a.toFixed(6),
      ctmTranslate: { e: +worldMatrix.e.toFixed(3), f: +worldMatrix.f.toFixed(3) },
      camera: { x: worldMatrix.e, y: worldMatrix.f, scale: worldMatrix.a },
      contentWidth: parseFloat(world.style.width) || 0,
      contentHeight: parseFloat(world.style.height) || 0,
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
      leaves,
      seam,
      outAria: outBtn?.getAttribute("aria-disabled") ?? null,
      inAria: inBtn?.getAttribute("aria-disabled") ?? null,
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

function summarizeGreen(results, hosts) {
  const passedChecks = [];
  const failedChecks = [];
  const missingSetup = [];
  const errors = [];
  for (const h of hosts) {
    const data = results.hosts[h];
    if (!data) continue;
    for (const c of data.checks) {
      if (c.kind === "setup" && !c.pass) {
        missingSetup.push(`${h}:${c.id}`);
      }
      if (!c.pass) {
        failedChecks.push(`${h}:${c.id}`);
      } else {
        passedChecks.push(`${h}:${c.id}`);
      }
    }
    if (data.scenarioError) errors.push(`${h}: ${data.scenarioError}`);
    if (data.pageErrors && data.pageErrors.length > 0) {
      errors.push(`${h} pageErrors: ${JSON.stringify(data.pageErrors)}`);
    }
  }
  const ok = missingSetup.length === 0 && failedChecks.length === 0 && errors.length === 0 && passedChecks.length > 0;
  return {
    passedChecks,
    failedChecks,
    missingSetup,
    errors,
    ok,
  };
}

async function runHostGreen(browser, host, evidencePaths) {
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

    const shot = async (name) => {
      const path = join(evidenceDir, name);
      await page.screenshot({ path });
      screenshots.push(name);
      actions.push({ action: "screenshot", name });
      evidencePaths.push(name);
      return name;
    };

    // 1. Setup & Initial render
    await openFixture(page, host, FIXTURE_A);
    checks.push(check("existence-real-graph-card-edge", "setup", "pass", true,
      { host, graphRunId: "qa-dag-a", card: FIXTURE_A.card, edge: FIXTURE_A.edge }));

    const geo0 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    if (!geo0) throw new Error("geometry capture returned null after existence proof");
    checks.push(check("existence-viewport-positive-rect", "setup", "pass",
      geo0.viewport.width > 0 && geo0.viewport.height > 0, { viewport: geo0.viewport }));

    await shot(`${host}-initial.png`);
    if (host === "modal") {
      await shot("desktop.png");
    }

    // 2. S1 Pan: Left-button drag on background
    const vp = geo0.viewport;
    const centerX = vp.x + vp.width / 2;
    const centerY = vp.y + vp.height / 2;
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

    checks.push(check("S1-pan-card-moves-120-80", "behavioral", "pass",
      movedBy(cardDelta, 120, 80),
      { expected: { x: 120, y: 80 }, actual: cardDelta }));
    checks.push(check("S1-pan-edge-endpoint-moves-120-80", "behavioral", "pass",
      movedBy(edgeDelta, 120, 80),
      { expected: { x: 120, y: 80 }, actual: edgeDelta }));
    checks.push(check("S1-pan-header-unchanged", "behavioral", "pass",
      movedBy(headerDelta, 0, 0, 0.5), { actual: headerDelta }));
    checks.push(check("S1-pan-cards-and-edges-aligned", "behavioral", "pass",
      Math.abs(cardDelta.x - edgeDelta.x) <= 1 && Math.abs(cardDelta.y - edgeDelta.y) <= 1,
      { cardDelta, edgeDelta }));

    // S1 Variant: Drag starting on a node card
    const cardCx = geo1.card.x + geo1.card.width / 2;
    const cardCy = geo1.card.y + geo1.card.height / 2;
    const cardTarget = await elementFromPointLabel(page, cardCx, cardCy);
    actions.push({ action: "mouse.move", x: cardCx, y: cardCy, target: cardTarget });
    await page.mouse.move(cardCx, cardCy);
    await page.mouse.down();
    actions.push({ action: "mouse.move", x: cardCx + 120, y: cardCy + 80, steps: 8 });
    await page.mouse.move(cardCx + 120, cardCy + 80, { steps: 8 });
    await page.mouse.up();
    await settle(page);

    const geo2 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    const cardDelta2 = delta(geo1.card, geo2.card);
    checks.push(check("S1-pan-from-card-moves-120-80", "behavioral", "pass",
      movedBy(cardDelta2, 120, 80),
      { expected: { x: 120, y: 80 }, actual: cardDelta2, startTarget: cardTarget }));

    // S1 Variant: Pan at 0.5x and 2.0x scales
    const scopeSel = SCOPE[host];
    const zoomOutBtn = page.locator(`${scopeSel} [data-testid="dag-controls"] button[aria-label="Zoom out"]`);
    const zoomInBtn = page.locator(`${scopeSel} [data-testid="dag-controls"] button[aria-label="Zoom in"]`);
    const fitBtn = page.locator(`${scopeSel} [data-testid="dag-controls"] button[aria-label="Fit graph"]`);
    const resetBtn = page.locator(`${scopeSel} [data-testid="dag-controls"] button[aria-label="Reset zoom to 100%"]`);

    // S1 Variant: Pan at exact 0.5x and 2.0x scales via controlled wheel
    await resetBtn.click();
    await settle(page);
    await page.mouse.move(centerX, centerY);
    const delta05 = -Math.log(0.5) / 0.002;
    actions.push({ action: "mouse.wheel", deltaX: 0, deltaY: delta05, targetScale: 0.5 });
    await page.mouse.wheel(0, delta05);
    await settle(page);
    const g05 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    await page.mouse.move(startX, startY);
    await page.mouse.down();
    await page.mouse.move(startX + 120, startY + 80, { steps: 8 });
    await page.mouse.up();
    await settle(page);
    const g05Pan = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S1-pan-at-0.5x-moves-120-80", "behavioral", "pass",
      Math.abs(g05.ctmScale - 0.5) <= 0.05 &&
      movedBy(delta(g05.card, g05Pan.card), 120, 80) &&
      movedBy(delta(g05.edgeEndpoint, g05Pan.edgeEndpoint), 120, 80),
      { scale: g05.ctmScale, cardDelta: delta(g05.card, g05Pan.card), edgeDelta: delta(g05.edgeEndpoint, g05Pan.edgeEndpoint) }));

    await resetBtn.click();
    await settle(page);
    await page.mouse.move(centerX, centerY);
    const delta2x = -Math.log(2.0) / 0.002;
    actions.push({ action: "mouse.wheel", deltaX: 0, deltaY: delta2x, targetScale: 2.0 });
    await page.mouse.wheel(0, delta2x);
    await settle(page);
    const g2x = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    await page.mouse.move(startX, startY);
    await page.mouse.down();
    await page.mouse.move(startX + 120, startY + 80, { steps: 8 });
    await page.mouse.up();
    await settle(page);
    const g2xPan = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S1-pan-at-2x-moves-120-80", "behavioral", "pass",
      Math.abs(g2x.ctmScale - 2.0) <= 0.05 &&
      movedBy(delta(g2x.card, g2xPan.card), 120, 80) &&
      movedBy(delta(g2x.edgeEndpoint, g2xPan.edgeEndpoint), 120, 80),
      { scale: g2x.ctmScale, cardDelta: delta(g2x.card, g2xPan.card), edgeDelta: delta(g2x.edgeEndpoint, g2xPan.edgeEndpoint) }));

    if (host === "standalone") {
      const siblingUnchanged = JSON.stringify(g2xPan.leaves) === JSON.stringify(geo0.leaves);
      checks.push(check("S1-standalone-sibling-leaves-unchanged-during-pan", "behavioral", "pass",
        siblingUnchanged, { leaves: g2xPan.leaves }));
    }

    await fitBtn.click();
    await settle(page);

    // 3. S2 Zoom: Wheel zoom in then out at viewport center
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

    const px = centerX - vp.x;
    const py = centerY - vp.y;
    const anchorError = {
      x: gz1.camera.x + gz1.camera.scale * (px - gz0.camera.x) / gz0.camera.scale - px,
      y: gz1.camera.y + gz1.camera.scale * (py - gz0.camera.y) / gz0.camera.scale - py,
    };

    checks.push(check("S2-zoom-in-increases-scale", "behavioral", "pass",
      gz1.ctmScale > gz0.ctmScale + 1e-4,
      { scaleBefore: gz0.ctmScale, scaleAfter: gz1.ctmScale }));
    checks.push(check("S2-zoom-anchor-world-point-stable", "behavioral", "pass",
      Math.abs(anchorError.x) <= 1.0 && Math.abs(anchorError.y) <= 1.0,
      { anchorError }));
    checks.push(check("S2-zoom-out-returns-scale", "behavioral", "pass",
      Math.abs(gz2.ctmScale - gz0.ctmScale) <= 1e-3,
      { expected: gz0.ctmScale, actual: gz2.ctmScale }));
    checks.push(check("S2-document-and-visual-viewport-unchanged", "behavioral", "pass",
      gz1.document.scrollY === 0 && (gz1.document.vvScale === null || gz1.document.vvScale === 1),
      { afterWheelIn: gz1.document }));

    // S2 Ctrl+wheel has identical scaling factor
    await page.keyboard.down("Control");
    actions.push({ action: "mouse.wheel", deltaX: 0, deltaY: -120, ctrlKey: true });
    await page.mouse.wheel(0, -120);
    await page.keyboard.up("Control");
    await settle(page);
    const gzCtrl = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S2-ctrl-wheel-same-factor", "behavioral", "pass",
      Math.abs(gzCtrl.ctmScale / gz2.ctmScale - Math.exp(0.24)) <= 0.05,
      { ratio: gzCtrl.ctmScale / gz2.ctmScale, expected: Math.exp(0.24) }));

    // S2 Delta modes (0, 1, 2) normalized scaling
    for (const mode of [0, 1, 2]) {
      await fitBtn.click();
      await settle(page);
      const ga = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
      const dy = mode === 0 ? -16 : mode === 1 ? -1 : -16 / ga.viewport.height;
      await page.evaluate(async ({ scopeSel, mode, dy }) => {
        const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
        const r = v.getBoundingClientRect();
        v.dispatchEvent(new WheelEvent("wheel", {
          bubbles: true,
          cancelable: true,
          clientX: r.x + r.width / 2,
          clientY: r.y + r.height / 2,
          deltaY: dy,
          deltaMode: mode,
        }));
      }, { scopeSel, mode, dy });
      await settle(page);
      const gb = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
      checks.push(check(`S2-wheel-deltaMode-${mode}-normalized`, "behavioral", "pass",
        Math.abs(gb.ctmScale / ga.ctmScale - Math.exp(0.032)) <= 0.01,
        { mode, ratio: gb.ctmScale / ga.ctmScale, expected: Math.exp(0.032) }));
    }

    // S2 Horizontal wheel invariant
    const gh0 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    actions.push({ action: "mouse.wheel", deltaX: 120, deltaY: 0 });
    const wheelHorizontalAck = await page.evaluate(({ scopeSel }) => {
      const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
      const r = v.getBoundingClientRect();
      const e = new WheelEvent("wheel", { bubbles: true, cancelable: true, clientX: r.x + 100, clientY: r.y + 100, deltaX: 120, deltaY: 0 });
      v.dispatchEvent(e);
      return { defaultPrevented: e.defaultPrevented };
    }, { scopeSel });
    await settle(page);
    const gh1 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S2-horizontal-wheel-camera-unchanged", "behavioral", "pass",
      gh1.ctmScale === gh0.ctmScale && gh0.worldTransform === gh1.worldTransform && !wheelHorizontalAck.defaultPrevented,
      { scaleBefore: gh0.ctmScale, scaleAfter: gh1.ctmScale, ack: wheelHorizontalAck }));

    // S2 Wheel clamp & no drift at max scale
    await page.mouse.move(centerX, centerY);
    for (let i = 0; i < 15; i++) {
      await page.mouse.wheel(0, -300);
    }
    await settle(page);
    const gClamp0 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S2-zoom-clamp-max-scale", "behavioral", "pass",
      Math.abs(gClamp0.ctmScale - 3.0) <= 0.05,
      { actual: gClamp0.ctmScale }));

    await page.mouse.wheel(0, -300);
    await settle(page);
    const gClamp1 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S2-zoom-clamp-no-drift", "behavioral", "pass",
      gClamp1.ctmScale === gClamp0.ctmScale && movedBy(delta(gClamp0.card, gClamp1.card), 0, 0, 0.5),
      { before: gClamp0.card, after: gClamp1.card }));

    // S2 Min scale clamp
    await page.mouse.move(centerX, centerY);
    for (let i = 0; i < 20; i++) {
      await page.mouse.wheel(0, 300);
    }
    await settle(page);
    const gMinClamp = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S2-zoom-clamp-min-scale", "behavioral", "pass",
      gMinClamp.ctmScale <= 0.15 && gMinClamp.outAria === "true",
      { scale: gMinClamp.ctmScale, outAria: gMinClamp.outAria }));

    await fitBtn.click();
    await settle(page);

    // 4. S3 Controls & Fit
    // Pan far offscreen
    await page.mouse.move(startX, startY);
    await page.mouse.down();
    await page.mouse.move(startX + 500, startY + 400, { steps: 8 });
    await page.mouse.up();
    await settle(page);

    await fitBtn.click();
    await settle(page);
    const gFit = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S3-controls-fit-graph", "behavioral", "pass",
      fitOk(gFit) && allInside(gFit),
      { scale: gFit.ctmScale, world: gFit.world, allInside: allInside(gFit) }));

    await zoomInBtn.click();
    await settle(page);
    const gPlus = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S3-controls-zoom-in-multiplies-1.2", "behavioral", "pass",
      Math.abs(gPlus.ctmScale - gFit.ctmScale * 1.2) <= 0.05,
      { before: gFit.ctmScale, after: gPlus.ctmScale }));

    await zoomOutBtn.click();
    await settle(page);
    const gMinus = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S3-controls-zoom-out-divides-1.2", "behavioral", "pass",
      Math.abs(gMinus.ctmScale - gFit.ctmScale) <= 0.05,
      { afterMinus: gMinus.ctmScale }));

    await resetBtn.click();
    await settle(page);
    const gReset = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S3-controls-reset-zoom-100", "behavioral", "pass",
      Math.abs(gReset.ctmScale - 1.0) <= 0.05,
      { scale: gReset.ctmScale }));

    await zoomInBtn.focus();
    const isPlusFocused = await page.evaluate(({ scopeSel }) => {
      const btn = document.querySelector(`${scopeSel} [data-testid="dag-controls"] button[aria-label="Zoom in"]`);
      return document.activeElement === btn;
    }, { scopeSel });
    checks.push(check("S3-controls-visible-focusable", "behavioral", "pass",
      isPlusFocused === true,
      { plusFocused: isPlusFocused }));

    // Fit on Fixtures (big, tall, empty)
    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildBigDagRun(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T12:30:00Z" });
    });
    await fitBtn.click();
    await settle(page);
    const gBig = await captureGeometry(page, host, FIXTURE_BIG.card, FIXTURE_BIG.edge);
    checks.push(check("S3-fit-big-fixture", "behavioral", "pass",
      gBig.ctmScale < 0.1 && fitOk(gBig, 29124, 144) && allInside(gBig),
      { scale: gBig.ctmScale, allInside: allInside(gBig) }));
    if (host === "standalone") {
      await shot("standalone-big-fit.png");
    }

    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildTallDagRun(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T12:31:00Z" });
    });
    await fitBtn.click();
    await settle(page);
    const gTall = await captureGeometry(page, host, FIXTURE_TALL.card, FIXTURE_TALL.edge);
    checks.push(check("S3-fit-tall-fixture", "behavioral", "pass",
      fitOk(gTall, 284, 7044) && allInside(gTall),
      { scale: gTall.ctmScale, allInside: allInside(gTall) }));
    if (host === "standalone") {
      await shot("standalone-tall-fit.png");
    }

    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildEmptyDagRun(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T12:32:00Z" });
    });
    await fitBtn.click();
    await settle(page);
    const gEmpty = await captureGeometry(page, host, null, null);
    checks.push(check("S3-empty-fixture-handled", "behavioral", "pass",
      gEmpty !== null && Number.isFinite(gEmpty.ctmScale),
      { scale: gEmpty?.ctmScale }));

    // Reset back to Fixture A
    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildQaDagRunA(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T12:33:00Z" });
    });
    await fitBtn.click();
    await settle(page);

    // 5. S4 Gesture Lifecycle: Drag across dialog boundary, release outside, move back
    await page.mouse.move(startX, startY);
    await page.mouse.down();
    await page.mouse.move(4, 4, { steps: 8 });
    await page.mouse.up();
    await settle(page);
    const gAfterRelease = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);

    await page.mouse.move(startX, startY);
    await settle(page);
    const gAfterMoveBack = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);

    checks.push(check("S4-drag-release-outside-camera-fixed", "behavioral", "pass",
      movedBy(delta(gAfterRelease.card, gAfterMoveBack.card), 0, 0, 1),
      { afterRelease: gAfterRelease.card, afterMoveBack: gAfterMoveBack.card }));

    // Right-click ignored
    await page.mouse.move(startX, startY);
    await page.mouse.down({ button: "right" });
    await page.mouse.move(startX + 100, startY + 100, { steps: 8 });
    await page.mouse.up({ button: "right" });
    await settle(page);
    const gAfterRight = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S4-right-click-ignored", "behavioral", "pass",
      movedBy(delta(gAfterMoveBack.card, gAfterRight.card), 0, 0, 1),
      { actual: delta(gAfterMoveBack.card, gAfterRight.card) }));

    // Interruptions during active drag with pressed continuation
    for (const interruption of ["blur", "hidden", "lostcapture", "cancel", "resize"]) {
      await fitBtn.click();
      await settle(page);
      const gBefore = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
      const vx = gBefore.viewport.x + 40;
      const vy = gBefore.viewport.y + 60;
      await page.mouse.move(vx, vy);
      await page.evaluate(({ scopeSel }) => {
        const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
        v.addEventListener("pointerdown", (e) => { window.__pid = e.pointerId; }, { once: true });
      }, { scopeSel });
      await page.mouse.down();
      await page.mouse.move(vx + 10, vy + 10);
      await settle(page);

      await page.evaluate(async ({ scopeSel, interruption }) => {
        const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
        if (interruption === "blur") window.dispatchEvent(new Event("blur"));
        if (interruption === "hidden") {
          Object.defineProperty(document, "hidden", { configurable: true, value: true });
          document.dispatchEvent(new Event("visibilitychange"));
          delete document.hidden;
        }
        if (interruption === "lostcapture") v.releasePointerCapture(window.__pid);
        if (interruption === "cancel") {
          v.dispatchEvent(new PointerEvent("pointercancel", { bubbles: true, pointerId: window.__pid, pointerType: "mouse" }));
        }
        if (interruption === "resize") {
          await new Promise((resolve, reject) => {
            const t = setTimeout(() => reject(new Error("resize timeout")), 3000);
            const ob = new ResizeObserver((entries) => {
              if (entries[0].contentRect.height === 120) {
                ob.disconnect();
                clearTimeout(t);
                requestAnimationFrame(resolve);
              }
            });
            ob.observe(v);
            v.style.flex = "none";
            v.style.height = "120px";
          });
        }
      }, { scopeSel, interruption });
      await settle(page);

      const gInterrupted = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
      // Subsequent pressed move must be inert
      await page.mouse.move(vx + 20, vy + 20);
      await page.mouse.up();
      await page.mouse.move(vx + 30, vy + 30);
      await settle(page);
      const gAfterInterrupted = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);

      const held = await page.locator(`${scopeSel} [data-testid="dag-viewport"]`).evaluate((v) => v.hasPointerCapture(window.__pid));
      checks.push(check(`S4-lifecycle-${interruption}-pressed-inert`, "behavioral", "pass",
        Math.abs(gInterrupted.camera.x - gAfterInterrupted.camera.x) <= 0.5 &&
        Math.abs(gInterrupted.camera.y - gAfterInterrupted.camera.y) <= 0.5 &&
        !held,
        { before: gInterrupted.camera, after: gAfterInterrupted.camera, captureHeld: held }));

      if (interruption === "resize") {
        await page.locator(`${scopeSel} [data-testid="dag-viewport"]`).evaluate((v) => {
          v.style.flex = "";
          v.style.height = "";
        });
        await settle(page);
      }
    }

    // Contenteditable attribute variants all reject pan (B4)
    for (const attr of ["", "plaintext-only", "false", "true"]) {
      await fitBtn.click();
      await settle(page);
      await page.locator(`${scopeSel} [data-testid="dag-viewport"]`).evaluate((v, attr) => {
        const d = document.createElement("div");
        d.id = "qa-editable";
        d.setAttribute("contenteditable", attr);
        d.style.cssText = "position:absolute;left:20px;top:20px;width:80px;height:30px;background:red";
        v.append(d);
      }, attr);
      const gBeforeEd = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
      const bb = await page.locator("#qa-editable").boundingBox();
      await page.mouse.move(bb.x + 10, bb.y + 10);
      await page.mouse.down();
      await page.mouse.move(bb.x + 30, bb.y + 20);
      await page.mouse.up();
      await settle(page);
      const gAfterEd = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
      checks.push(check(`S4-contenteditable-${attr || "empty"}-rejects-pan`, "behavioral", "pass",
        Math.abs(gBeforeEd.camera.x - gAfterEd.camera.x) <= 0.5 &&
        Math.abs(gBeforeEd.camera.y - gAfterEd.camera.y) <= 0.5,
        { before: gBeforeEd.camera, after: gAfterEd.camera }));
      await page.locator("#qa-editable").evaluate((e) => e.remove());
    }

    // 6. S5 Touch / Pinch via Chrome DevTools Protocol
    const cdp = await context.newCDPSession(page);
    await fitBtn.click();
    await settle(page);
    const gP0 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    const p1x = Math.round(vp.x + 100);
    const p1y = Math.round(vp.y + 100);
    const p2x = Math.round(vp.x + 200);
    const p2y = Math.round(vp.y + 100);

    // 1 -> 2 contact transition without jump
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchStart",
      touchPoints: [{ id: 1, x: p1x, y: p1y }],
    });
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchStart",
      touchPoints: [
        { id: 1, x: p1x, y: p1y },
        { id: 2, x: p2x, y: p2y },
      ],
    });
    await settle(page);
    const gPStart = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S5-touch-add-second-no-jump", "behavioral", "pass",
      Math.abs(gPStart.camera.x - gP0.camera.x) <= 0.5 && Math.abs(gPStart.camera.y - gP0.camera.y) <= 0.5,
      { before: gP0.camera, after: gPStart.camera }));

    // Exact 1.6 pinch ratio & midpoint anchor
    const movedTouch = [
      { id: 1, x: p1x - 10, y: p1y + 20 },
      { id: 2, x: p1x + 150, y: p1y + 20 },
    ];
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchMove",
      touchPoints: movedTouch,
    });
    await settle(page);
    const gP1 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);

    const mx = p1x + 50 - vp.x;
    const my = p1y - vp.y;
    const pinchAnchorError = {
      x: gP1.camera.x + gP1.camera.scale * (mx - gP0.camera.x) / gP0.camera.scale - (p1x + 70 - vp.x),
      y: gP1.camera.y + gP1.camera.scale * (my - gP0.camera.y) / gP0.camera.scale - (p1y + 20 - vp.y),
    };
    checks.push(check("S5-pinch-exact-1.6-ratio-and-midpoint-anchor", "behavioral", "pass",
      Math.abs(gP1.camera.scale / gP0.camera.scale - 1.6) <= 0.01 &&
      Math.abs(pinchAnchorError.x) <= 1.0 &&
      Math.abs(pinchAnchorError.y) <= 1.0,
      { ratio: gP1.camera.scale / gP0.camera.scale, anchorError: pinchAnchorError }));

    // Extra contact ignored during pinch
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchStart",
      touchPoints: [...movedTouch, { id: 3, x: p1x + 50, y: p1y + 40 }],
    });
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchMove",
      touchPoints: [...movedTouch, { id: 3, x: p1x + 70, y: p1y + 45 }],
    });
    await settle(page);
    const gPExtra = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S5-pinch-extra-contact-ignored", "behavioral", "pass",
      Math.abs(gPExtra.camera.scale - gP1.camera.scale) <= 0.001,
      { before: gP1.camera, after: gPExtra.camera }));

    // Lift contact 3 then contact 2, move survivor by (20, 10)
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchEnd",
      touchPoints: [{ id: 3, x: p1x + 70, y: p1y + 45 }],
    });
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchEnd",
      touchPoints: [movedTouch[1]],
    });
    await settle(page);
    const gBeforeSurvivor = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);

    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchMove",
      touchPoints: [{ id: 1, x: p1x - 10 + 20, y: p1y + 20 + 10 }],
    });
    await settle(page);
    const gAfterSurvivor = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S5-pinch-survivor-pan-no-jump", "behavioral", "pass",
      movedBy(delta(gBeforeSurvivor.camera, gAfterSurvivor.camera), 20, 10, 1),
      { delta: delta(gBeforeSurvivor.camera, gAfterSurvivor.camera) }));

    await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    await settle(page);

    // Degenerate distance (<1px) deferred baseline
    await fitBtn.click();
    await settle(page);
    const gD0 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    await page.evaluate(({ scopeSel, p1x, p1y }) => {
      const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
      v.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, pointerId: 80, pointerType: "touch", clientX: p1x, clientY: p1y }));
      v.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, pointerId: 81, pointerType: "touch", clientX: p1x + 0.5, clientY: p1y }));
      v.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, pointerId: 81, pointerType: "touch", clientX: p1x + 0.7, clientY: p1y }));
    }, { scopeSel, p1x, p1y });
    await settle(page);
    const gD1 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    await page.evaluate(({ scopeSel, p1x, p1y }) => {
      const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
      v.dispatchEvent(new PointerEvent("pointercancel", { bubbles: true, pointerId: 80, pointerType: "touch" }));
      v.dispatchEvent(new PointerEvent("pointercancel", { bubbles: true, pointerId: 81, pointerType: "touch" }));
    }, { scopeSel, p1x, p1y });
    await settle(page);
    checks.push(check("S5-pinch-degenerate-deferred-baseline", "behavioral", "pass",
      Math.abs(gD1.camera.scale - gD0.camera.scale) <= 0.001,
      { scaleBefore: gD0.camera.scale, scaleAfter: gD1.camera.scale }));

    // Outward pinch at newly raised minimum (B2 regression)
    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildBigDagRun(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T12:40:00Z" });
    });
    await fitBtn.click();
    await settle(page);
    const gBigBeforeShrink = await captureGeometry(page, host, FIXTURE_BIG.card, FIXTURE_BIG.edge);

    // Update to Fixture A under same runId (raises minimum to 0.1, current scale is <0.1)
    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildQaDagRunA(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T12:41:00Z" });
    });
    await settle(page);
    const gSmallRetained = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S6-topology-shrink-preserves-camera", "behavioral", "pass",
      Math.abs(gSmallRetained.camera.scale - gBigBeforeShrink.camera.scale) <= 0.001,
      { scaleBefore: gBigBeforeShrink.camera.scale, scaleAfter: gSmallRetained.camera.scale }));

    // Outward pinch: distance 100 -> 90: scale must remain unchanged
    const bpx = Math.round(vp.x + 30);
    const bpy = Math.round(vp.y + 60);
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchStart",
      touchPoints: [
        { id: 51, x: bpx, y: bpy },
        { id: 52, x: bpx + 100, y: bpy },
      ],
    });
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchMove",
      touchPoints: [
        { id: 51, x: bpx + 5, y: bpy },
        { id: 52, x: bpx + 95, y: bpy },
      ],
    });
    await settle(page);
    const gAfterOutPinch = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    await settle(page);
    checks.push(check("S5-pinch-outward-raised-minimum-recovery", "behavioral", "pass",
      Math.abs(gAfterOutPinch.camera.scale - gSmallRetained.camera.scale) <= 0.001,
      { scaleBefore: gSmallRetained.camera.scale, scaleAfter: gAfterOutPinch.camera.scale }));

    // 7. S6 Live state & host independence
    // Custom camera status update retention
    await resetBtn.click();
    await page.mouse.move(startX, startY);
    await page.mouse.down();
    await page.mouse.move(startX + 60, startY + 40);
    await page.mouse.up();
    await settle(page);
    const gLive0 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);

    await page.locator('[data-testid="qa-update-status-a"]').dispatchEvent("click");
    await settle(page);
    const gLive1 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("S6-same-run-status-update-preserves-camera", "behavioral", "pass",
      Math.abs(gLive1.camera.scale - gLive0.camera.scale) <= 0.001 &&
      movedBy(delta(gLive0.camera, gLive1.camera), 0, 0, 1),
      { scaleBefore: gLive0.camera.scale, scaleAfter: gLive1.camera.scale }));

    // Same-run empty -> first nonempty fits without user Fit (B1)
    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildEmptyDagRun(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T12:50:00Z" });
    });
    await fitBtn.click();
    await settle(page);
    const gEmptyBefore = await captureGeometry(page, host, null, null);

    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildBigDagRun(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T12:51:00Z" });
    });
    await settle(page);
    const gNonEmptyAfter = await captureGeometry(page, host, FIXTURE_BIG.card, FIXTURE_BIG.edge);
    checks.push(check("S6-empty-to-first-nonempty-fits-without-user-fit", "behavioral", "pass",
      fitOk(gNonEmptyAfter, 29124, 144) && allInside(gNonEmptyAfter),
      { emptyScale: gEmptyBefore?.camera.scale, fittedScale: gNonEmptyAfter.camera.scale, allInside: allInside(gNonEmptyAfter) }));

    // Zero-size deferral recovery: 0x0 -> empty -> big -> positive fits (B1)
    await page.evaluate(async ({ scopeSel }) => {
      const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
      await new Promise((resolve, reject) => {
        let t = setTimeout(() => { ob.disconnect(); reject(new Error("resize signal timeout")); }, 3000);
        const ob = new ResizeObserver((entries) => {
          const r = entries[0].contentRect;
          if (r.width === 0 && r.height === 0) {
            clearTimeout(t);
            ob.disconnect();
            requestAnimationFrame(resolve);
          }
        });
        ob.observe(v);
        v.style.flex = "none";
        v.style.width = "0px";
        v.style.height = "0px";
      });
    }, { scopeSel });
    await settle(page);

    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildEmptyDagRun(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T12:55:00Z" });
    });
    await settle(page);

    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildBigDagRun(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T12:56:00Z" });
    });
    await settle(page);

    await page.evaluate(async ({ scopeSel }) => {
      const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
      await new Promise((resolve, reject) => {
        let t = setTimeout(() => { ob.disconnect(); reject(new Error("resize signal timeout")); }, 3000);
        const ob = new ResizeObserver((entries) => {
          const r = entries[0].contentRect;
          if (r.width === 200 && r.height === 200) {
            clearTimeout(t);
            ob.disconnect();
            requestAnimationFrame(resolve);
          }
        });
        ob.observe(v);
        v.style.width = "200px";
        v.style.height = "200px";
      });
    }, { scopeSel });
    await settle(page);
    const gZeroRecovery = await captureGeometry(page, host, FIXTURE_BIG.card, FIXTURE_BIG.edge);
    checks.push(check("S6-zero-deferral-positive-recovery-fits", "behavioral", "pass",
      fitOk(gZeroRecovery, 29124, 144),
      { camera: gZeroRecovery.camera, expected: fitExpected(gZeroRecovery.viewport, 29124, 144) }));

    // Restore viewport style
    await page.evaluate(({ scopeSel }) => {
      const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
      v.style.flex = "";
      v.style.width = "";
      v.style.height = "";
    }, { scopeSel });
    await settle(page);

    // Reset back to Fixture A
    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildQaDagRunA(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T13:00:00Z" });
    });
    await fitBtn.click();
    await settle(page);

    // Resize viewport center world point stability
    const gBeforeResize = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    await page.setViewportSize({ width: 1100, height: 800 });
    await settle(page);
    const gAfterResize = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    const expectedShiftX = (gAfterResize.viewport.width - gBeforeResize.viewport.width) / 2;
    const expectedShiftY = (gAfterResize.viewport.height - gBeforeResize.viewport.height) / 2;
    checks.push(check("S6-resize-center-world-point-stable", "behavioral", "pass",
      gAfterResize.ctmScale === gBeforeResize.ctmScale &&
      Math.abs(gAfterResize.camera.x - gBeforeResize.camera.x - expectedShiftX) <= 1 &&
      Math.abs(gAfterResize.camera.y - gBeforeResize.camera.y - expectedShiftY) <= 1,
      { scaleBefore: gBeforeResize.ctmScale, scaleAfter: gAfterResize.ctmScale, expectedShiftX, expectedShiftY }));
    await page.setViewportSize({ width: 1280, height: 900 });
    await settle(page);

    if (host === "modal") {
      // Modal tab switch to run B
      await page.locator('[data-testid="dag-pane-modal-tab-qa-dag-b"]').click();
      await page.locator(`${scopeSel} [data-testid="dag-node-b-1"]`).waitFor({ state: "visible", timeout: PAGE_TIMEOUT_MS });
      await settle(page);
      const gRunB = await captureGeometry(page, host, "dag-node-b-1", "dag-edge-b-1-b-4");
      checks.push(check("S6-modal-tab-switch-fits-new-run", "behavioral", "pass",
        fitOk(gRunB) && allInside(gRunB),
        { scale: gRunB.ctmScale, allInside: allInside(gRunB) }));

      // Focus loop: Tab and Shift+Tab wrapping
      const firstBtn = page.locator(`${scopeSel} button`).first();
      const lastBtn = page.locator(`${scopeSel} button`).last();
      await firstBtn.focus();
      await page.keyboard.press("Shift+Tab");
      const wrappedBack = await lastBtn.evaluate((el) => el === document.activeElement);
      await page.keyboard.press("Tab");
      const wrappedForward = await firstBtn.evaluate((el) => el === document.activeElement);
      checks.push(check("S6-modal-focus-trap-tab-and-shift-tab", "behavioral", "pass",
        wrappedBack && wrappedForward,
        { wrappedBack, wrappedForward }));

      // Escape key closes modal
      await page.keyboard.press("Escape");
      await settle(page);
      const modalClosed = await page.evaluate(() => document.querySelector('[data-testid="dag-pane-modal"]') === null);
      checks.push(check("S6-modal-escape-closes", "behavioral", "pass",
        modalClosed === true,
        { modalClosed }));

      // Reopen modal twice and verify single wheel factor (no duplicate listeners)
      for (let i = 0; i < 2; i++) {
        await page.locator('[data-testid="qa-modal-host"] [data-testid="dag-pane-badge-button"]').click();
        await page.locator(`${scopeSel} [data-testid="dag-world"]`).waitFor({ state: "visible", timeout: PAGE_TIMEOUT_MS });
        await settle(page);
        const gReopen0 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
        const vpr = gReopen0.viewport;
        await page.mouse.move(vpr.x + vpr.width / 2, vpr.y + vpr.height / 2);
        await page.mouse.wheel(0, -120);
        await settle(page);
        const gReopen1 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
        checks.push(check(`S6-modal-reopen-single-wheel-factor-${i}`, "behavioral", "pass",
          Math.abs(gReopen1.ctmScale / gReopen0.ctmScale - Math.exp(0.24)) <= 0.05,
          { ratio: gReopen1.ctmScale / gReopen0.ctmScale }));
        await page.keyboard.press("Escape");
        await settle(page);
      }
    }

    if (host === "standalone") {
      // Measurable split seam resize (B7)
      const seam = page.locator('[role="separator"]');
      const bb = await seam.boundingBox();
      const gBeforeSeam = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
      actions.push({ action: "seam-drag-start", x: bb.x + 0.5, y: bb.y + bb.height / 2 });
      await page.mouse.move(bb.x + 0.5, bb.y + bb.height / 2);
      await page.mouse.down();
      actions.push({ action: "seam-drag-move", x: bb.x + 50, y: bb.y + bb.height / 2 });
      await page.mouse.move(bb.x + 50, bb.y + bb.height / 2);
      actions.push({ action: "seam-drag-up" });
      await page.mouse.up();
      await settle(page);
      const gAfterSeam = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
      checks.push(check("S6-standalone-split-seam-resize-changes-widths", "behavioral", "pass",
        JSON.stringify(gBeforeSeam.leaves) !== JSON.stringify(gAfterSeam.leaves),
        { before: gBeforeSeam.leaves, after: gAfterSeam.leaves }));
    }
  } catch (e) {
    scenarioError = String(e?.stack ?? e);
    checks.push(check("harness-error", "setup", "pass", false, { error: scenarioError }));
    try {
      const errorShot = join(evidenceDir, `${host}-green-error.png`);
      await page.screenshot({ path: errorShot });
      screenshots.push(`${host}-green-error.png`);
    } catch { /* best effort */ }
  } finally {
    await context.close();
  }

  const pageErrorNoise = pageErrors.filter((e) => isViteHmrClientNoise(e.message));
  const pageErrorReal = pageErrors.filter((e) => !isViteHmrClientNoise(e.message));
  return {
    checks,
    actions,
    consoleErrors,
    pageErrors: pageErrorReal,
    environmentNoise: {
      reason: "ui/vite.config.ts pins hmr clientPort 5173; QA monitor serves 5193.",
      pageErrors: pageErrorNoise,
      consoleErrors: consoleErrors.filter((e) => isViteHmrClientNoise(e.text)),
    },
    screenshots,
    skipNotes,
    scenarioError,
  };
}

async function runHostMobile(browser, host, evidencePaths) {
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 1 });
  const page = await context.newPage();
  const checks = [];
  const actions = [];
  const screenshots = [];

  try {
    await openFixture(page, host, FIXTURE_A);
    const scopeSel = SCOPE[host];

    const controlsBox = await page.locator(`${scopeSel} [data-testid="dag-controls"]`).boundingBox();
    const bodyScrollWidth = await page.evaluate(() => document.body.scrollWidth);

    checks.push(check("mobile-controls-visible-in-bounds", "behavioral", "pass",
      controlsBox !== null && controlsBox.x >= 0 && (controlsBox.x + controlsBox.width) <= 395,
      { controlsBox }));
    checks.push(check("mobile-no-horizontal-page-overflow", "behavioral", "pass",
      bodyScrollWidth <= 395,
      { bodyScrollWidth }));

    // Mobile S1 Pan
    const fitBtn = page.locator(`${scopeSel} [data-testid="dag-controls"] button[aria-label="Fit graph"]`);
    await fitBtn.click();
    await settle(page);
    const gMob0 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    const mvp = gMob0.viewport;
    await page.mouse.move(mvp.x + 50, mvp.y + 50);
    await page.mouse.down();
    await page.mouse.move(mvp.x + 100, mvp.y + 80);
    await page.mouse.up();
    await settle(page);
    const gMobPan = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("mobile-S1-pan-moves", "behavioral", "pass",
      movedBy(delta(gMob0.card, gMobPan.card), 50, 30, 1),
      { delta: delta(gMob0.card, gMobPan.card) }));

    // Mobile S2 Zoom & Anchor
    await fitBtn.click();
    await settle(page);
    const gMobZ0 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    await page.mouse.move(mvp.x + mvp.width / 2, mvp.y + mvp.height / 2);
    await page.mouse.wheel(0, -120);
    await settle(page);
    const gMobZ1 = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("mobile-S2-zoom-increases", "behavioral", "pass",
      gMobZ1.ctmScale > gMobZ0.ctmScale,
      { before: gMobZ0.ctmScale, after: gMobZ1.ctmScale }));

    // Mobile S3 Fit formula and containment
    await fitBtn.click();
    await settle(page);
    const gMobFitA = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    checks.push(check("mobile-S3-fit-A", "behavioral", "pass",
      fitOk(gMobFitA) && allInside(gMobFitA),
      { scale: gMobFitA.ctmScale, allInside: allInside(gMobFitA) }));

    // Mobile big fixture
    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildBigDagRun(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T13:10:00Z" });
    });
    await fitBtn.click();
    await settle(page);
    const gMobBig = await captureGeometry(page, host, FIXTURE_BIG.card, FIXTURE_BIG.edge);
    checks.push(check("mobile-S3-fit-big", "behavioral", "pass",
      gMobBig.ctmScale < 0.1 && fitOk(gMobBig) && allInside(gMobBig),
      { scale: gMobBig.ctmScale, allInside: allInside(gMobBig) }));

    // Mobile tall fixture
    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildTallDagRun(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T13:11:00Z" });
    });
    await fitBtn.click();
    await settle(page);
    const gMobTall = await captureGeometry(page, host, FIXTURE_TALL.card, FIXTURE_TALL.edge);
    checks.push(check("mobile-S3-fit-tall", "behavioral", "pass",
      fitOk(gMobTall) && allInside(gMobTall),
      { scale: gMobTall.ctmScale, allInside: allInside(gMobTall) }));

    // Mobile empty fixture
    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildEmptyDagRun(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T13:12:00Z" });
    });
    await fitBtn.click();
    await settle(page);
    const gMobEmpty = await captureGeometry(page, host, null, null);
    checks.push(check("mobile-S3-fit-empty", "behavioral", "pass",
      gMobEmpty !== null && Number.isFinite(gMobEmpty.ctmScale),
      { scale: gMobEmpty?.ctmScale }));

    // Reset back to Fixture A
    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildQaDagRunA(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T13:13:00Z" });
    });
    await fitBtn.click();
    await settle(page);

    // Mobile S5 CDP touch pinch (exact 1.6 ratio)
    const cdp = await context.newCDPSession(page);
    const px = Math.round(mvp.x + 30);
    const py = Math.round(mvp.y + 60);
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchStart",
      touchPoints: [
        { id: 1, x: px, y: py },
        { id: 2, x: px + 100, y: py },
      ],
    });
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchMove",
      touchPoints: [
        { id: 1, x: px - 10, y: py + 20 },
        { id: 2, x: px + 150, y: py + 20 },
      ],
    });
    await settle(page);
    const gMobPinch = await captureGeometry(page, host, FIXTURE_A.card, FIXTURE_A.edge);
    await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    await settle(page);
    checks.push(check("mobile-S5-cdp-pinch-1.6", "behavioral", "pass",
      Math.abs(gMobPinch.camera.scale / gMobFitA.camera.scale - 1.6) <= 0.05,
      { ratio: gMobPinch.camera.scale / gMobFitA.camera.scale }));

    // Mobile S6 empty -> first nonempty
    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildEmptyDagRun(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T13:20:00Z" });
    });
    await fitBtn.click();
    await settle(page);

    await page.evaluate(async () => {
      const { dagStore } = await import("/src/state/dagStore.ts");
      const f = await import("/src/devtools/dagViewportQaFixtures.ts");
      dagStore.applySnapshot(f.QA_PROJECT_PATH, { ...f.buildBigDagRun(), runId: "qa-dag-a", rootSessionId: "qa-owner", updatedAt: "2026-09-12T13:21:00Z" });
    });
    await settle(page);
    const gMobNonEmpty = await captureGeometry(page, host, FIXTURE_BIG.card, FIXTURE_BIG.edge);
    checks.push(check("mobile-S6-empty-to-first-nonempty-fits", "behavioral", "pass",
      fitOk(gMobNonEmpty, 29124, 144) && allInside(gMobNonEmpty),
      { scale: gMobNonEmpty.camera.scale, allInside: allInside(gMobNonEmpty) }));

    const shot = async (name) => {
      const path = join(evidenceDir, name);
      await page.screenshot({ path });
      screenshots.push(name);
      actions.push({ action: "screenshot", name });
      evidencePaths.push(name);
      return name;
    };
    await shot(`${host}-mobile.png`);
    if (host === "modal") {
      await shot("mobile.png");
    }
  } catch (e) {
    checks.push(check("mobile-error", "setup", "pass", false, { error: String(e?.stack ?? e) }));
  } finally {
    await context.close();
  }
  return { checks, actions, screenshots };
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
  if (phase === "baseline") {
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
  } else if (phase === "green") {
    for (const h of hosts) {
      const hostGreen = await runHostGreen(browser, h, evidencePaths);
      const hostMobile = await runHostMobile(browser, h, evidencePaths);
      hostGreen.checks.push(...hostMobile.checks);
      hostGreen.actions.push(...hostMobile.actions);
      hostGreen.screenshots.push(...hostMobile.screenshots);
      results.hosts[h] = hostGreen;
    }
    results.greenSummary = summarizeGreen(results, hosts);
    results.screenshots = evidencePaths;
    results.finishedAt = new Date().toISOString();
    writeFileSync(join(evidenceDir, "results.json"), JSON.stringify(results, null, 2));
    writeFileSync(join(evidenceDir, "actions.json"), JSON.stringify(
      { generatedAt: new Date().toISOString(), byHost: Object.fromEntries(hosts.map((h) => [h, results.hosts[h]?.actions ?? []])), runner: actions },
      null, 2,
    ));
    const summary = results.greenSummary;
    for (const line of summary.passedChecks) {
      process.stdout.write(`GREEN-PASS ${line}\n`);
    }
    for (const line of summary.failedChecks) {
      process.stdout.write(`GREEN-FAIL ${line}\n`);
    }
    for (const line of summary.missingSetup) {
      process.stdout.write(`SETUP-FAILED ${line}\n`);
    }
    for (const line of summary.errors) {
      process.stdout.write(`ERROR ${line}\n`);
    }
    process.stdout.write(`summary: setup=${summary.missingSetup.length === 0 ? "ok" : "failed"} passed=${summary.passedChecks.length} failed=${summary.failedChecks.length} hosts=${hosts.join(",")}\n`);
    exitCode = summary.ok ? 0 : 1;
  }
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
