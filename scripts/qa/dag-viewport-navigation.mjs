#!/usr/bin/env bun
/**
 * scripts/qa/dag-viewport-navigation.mjs
 *
 * DAG Viewport Navigation Acceptance Test Runner.
 * Supports:
 *   --phase baseline|green
 *   --host modal|standalone|both
 *   --evidence-dir <dir>
 *   [--url <url>]
 *
 * Server lifetime is owned by invoking monitor; runner owns only Chrome context.
 */

import { createRequire } from "node:module";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const repo = join(__filename, "..", "..", "..");
const PAGE_TIMEOUT_MS = 20000;

function printUsageAndExit(code) {
  const usage = [
    "Usage: bun scripts/qa/dag-viewport-navigation.mjs --phase baseline|green --host modal|standalone|both --evidence-dir <directory> [--url <qa page url>]",
    "Server lifetime is owned by the invoking monitor; this runner owns only its Chrome context.",
    "Baseline expects behavioral RED; green requires all declared checks to pass.",
  ].join("\n");
  if (code === 0) {
    console.log(usage);
  } else {
    console.log(usage);
  }
  process.exit(code);
}

function fail(code, message) {
  console.error(`dag-viewport-navigation runner: invalid arguments: ${message}`);
  process.exit(code);
}

const rawArgs = process.argv.slice(2);
if (rawArgs.length === 0 || rawArgs.includes("--help") || rawArgs.includes("-h")) {
  printUsageAndExit(2);
}

let phase = null;
let hostArg = null;
let evidenceDir = null;
let url = "http://127.0.0.1:5193/dag-viewport-qa.html";

for (let i = 0; i < rawArgs.length; i++) {
  const arg = rawArgs[i];
  if (arg === "--phase") {
    phase = rawArgs[++i];
  } else if (arg === "--host") {
    hostArg = rawArgs[++i];
  } else if (arg === "--evidence-dir") {
    evidenceDir = rawArgs[++i];
  } else if (arg === "--url") {
    url = rawArgs[++i];
  } else {
    fail(2, `unrecognized argument: ${arg}`);
  }
}

if (!phase || !hostArg || !evidenceDir) {
  printUsageAndExit(2);
}

if (phase !== "baseline" && phase !== "green") {
  fail(2, "--phase must be 'baseline' or 'green'");
}

if (hostArg !== "modal" && hostArg !== "standalone" && hostArg !== "both") {
  fail(2, "--host must be 'modal', 'standalone' or 'both'");
}

const hosts = hostArg === "both" ? ["modal", "standalone"] : [hostArg];
mkdirSync(evidenceDir, { recursive: true });

function resolvePlaywrightCore() {
  try {
    return { pw: createRequire(join(repo, "package.json"))("playwright-core"), source: "repo node_modules" };
  } catch {
    /* fall through */
  }
  const globalCandidate = "/Users/indo/.bun/install/global/node_modules/playwright-core";
  try {
    return { pw: createRequire(join(repo, "package.json"))(globalCandidate), source: globalCandidate };
  } catch {
    /* fall through */
  }
  fail(2, "playwright-core is unavailable; set PLAYWRIGHT_CORE_PATH to an existing playwright-core package directory");
}

const { pw, source: playwrightSource } = resolvePlaywrightCore();

const FIXTURE_A = {
  runId: "qa-dag-a",
  card: "dag-node-a",
  edge: "dag-edge-a-c",
};

const SCOPE = {
  modal: '[data-testid="dag-pane-modal"]',
  standalone: '[data-testid="qa-standalone-host"]',
};

function check(id, kind, expectation, pass, detail) {
  return { id, kind, expectation, pass, detail };
}

function delta(before, after) {
  return {
    x: +(after.x - before.x).toFixed(2),
    y: +(after.y - before.y).toFixed(2),
  };
}

function movedBy(d, x, y, tol = 1) {
  return Math.abs(d.x - x) <= tol && Math.abs(d.y - y) <= tol;
}

const close = (a, b, t = 0.001) => Math.abs(a - b) <= t;
const same = (a, b, t = 0.001) => close(a.x, b.x, 1.0) && close(a.y, b.y, 1.0) && close(a.scale, b.scale, t);

function fitExpected(vp, worldW, worldH) {
  const margin = Math.min(24, vp.width / 4, vp.height / 4);
  const fitScale = Math.min(1, (vp.width - 2 * margin) / worldW, (vp.height - 2 * margin) / worldH);
  const x = (vp.width - worldW * fitScale) / 2;
  const y = (vp.height - worldH * fitScale) / 2;
  return { x: +x.toFixed(4), y: +y.toFixed(4), scale: +fitScale.toFixed(6) };
}

function fitOk(geo, worldW, worldH) {
  if (!geo || !geo.viewport || !geo.camera) return false;
  const w = worldW ?? geo.contentWidth ?? 1160;
  const h = worldH ?? geo.contentHeight ?? 284;
  const expected = fitExpected(geo.viewport, w, h);
  return (
    Math.abs(geo.ctmScale - expected.scale) <= 0.001 &&
    Math.abs(geo.camera.x - expected.x) <= 1.0 &&
    Math.abs(geo.camera.y - expected.y) <= 1.0
  );
}

function allInside(geo) {
  if (!geo || !geo.viewport) return false;
  const vp = geo.viewport;
  const tol = 1.0;
  if (geo.world) {
    if (
      geo.world.x < vp.x - tol ||
      geo.world.x + geo.world.width > vp.x + vp.width + tol ||
      geo.world.y < vp.y - tol ||
      geo.world.y + geo.world.height > vp.y + vp.height + tol
    ) {
      return false;
    }
  }
  const allPoints = [
    ...(geo.cards || []).map((c) => ({ x: c.x, y: c.y })),
    ...(geo.cards || []).map((c) => ({ x: c.x + c.width, y: c.y + c.height })),
    ...(geo.endpoints || []),
  ];
  if (allPoints.length === 0) return true;
  return allPoints.every(
    (p) =>
      p.x >= vp.x - tol &&
      p.x <= vp.x + vp.width + tol &&
      p.y >= vp.y - tol &&
      p.y <= vp.y + vp.height + tol,
  );
}

async function captureGeometry(page, host, cardTestId = FIXTURE_A.card, edgeTestId = FIXTURE_A.edge) {
  const scopeSel = SCOPE[host];
  return page.evaluate(({ scopeSel, cardTestId, edgeTestId }) => {
    const root = document.querySelector(scopeSel);
    if (!root) return null;
    const header = root.querySelector('[data-testid="dag-header"]');
    const world = root.querySelector('[data-testid="dag-world"]');
    const card = root.querySelector(`[data-testid="${cardTestId}"]`);
    const edge = root.querySelector(`[data-testid="${edgeTestId}"]`);
    const path = edge && edge.tagName.toLowerCase() === "path" ? edge : (edge ? edge.querySelector("path") : null);
    if (!header || !world) return null;

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
      runId: root.querySelector('[data-testid="dag-graph-view"]')?.getAttribute('data-run-id'),
      cardIds: [...world.children].filter(e=>e.matches('[data-testid^="dag-node-"]')).map(e=>e.getAttribute('data-testid')),
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
      style: {
        overflow: getComputedStyle(viewport).overflow,
        touchAction: getComputedStyle(viewport).touchAction,
        overscroll: getComputedStyle(viewport).overscrollBehavior,
      },
    };
  }, { scopeSel, cardTestId, edgeTestId });
}

// --------------------------------------------------------------------------
// Event-bounded acknowledgements without double-rAF settle
// --------------------------------------------------------------------------

const mousePositions = new WeakMap();
const touchContacts = new WeakMap();
let receiptId = 0;

// One pre-trigger subscription: exact native events AND a committed QA subtree.
// Matching all changed contacts prevents the first pointer event acknowledging a
// multi-contact command. The native event object is retained through commit.
async function armReceipt(page, scopeSel, expected = []) {
  const id = ++receiptId;
  await page.evaluate(({ id, scopeSel, expected }) => {
    window.__ackPromise = new Promise((resolve, reject) => {
      const events = [];
      const pending = expected.map(e => ({ ...e }));
      const types = [...new Set(pending.map(e => e.type))];
      const cleanup = () => {
        clearTimeout(timer);
        types.forEach(type => window.removeEventListener(type, onEvent, true));
        window.removeEventListener('qa-render-receipt', committed);
      };
      const timer = setTimeout(() => { cleanup(); reject(Error(`receipt ${id} missing ${JSON.stringify(pending)}`)); }, 5000);
      const snapshot = () => {
        const root = document.querySelector(scopeSel);
        const world = root?.querySelector('[data-testid="dag-world"]');
        const v = world?.parentElement;
        const m = world ? new DOMMatrix(getComputedStyle(world).transform) : null;
        return { runId: root?.querySelector('[data-testid="dag-graph-view"]')?.getAttribute('data-run-id'),
          camera: m ? { x:m.e, y:m.f, scale:m.a } : null,
          viewport: v ? { width:v.clientWidth,height:v.clientHeight } : null,
          topology: world ? { width:world.style.width,height:world.style.height, cards:[...world.children].filter(e=>e.matches('[data-testid^="dag-node-"]')).map(e=>e.getAttribute('data-testid')) } : null,
          active: document.activeElement?.getAttribute('aria-label') };
      };
      const committed = e => {
        if (e.detail.id !== id) return;
        cleanup();
        resolve({ id, committed:true, events:events.map(e=>({type:e.type, trusted:e.isTrusted, pointerId:e.pointerId,
          pointerType:e.pointerType, x:e.clientX,y:e.clientY,button:e.button,buttons:e.buttons,
          deltaX:e.deltaX,deltaY:e.deltaY,key:e.key,defaultPrevented:e.defaultPrevented,
          target:e.target instanceof Element ? e.target.closest('[data-testid]')?.getAttribute('data-testid') : null})), ...snapshot() });
      };
      const request = () => window.dispatchEvent(new CustomEvent('qa-request-receipt', {detail:{id}}));
      const onEvent = e => {
        const i = pending.findIndex(want => Object.entries(want).every(([k,v]) => k === 'selector' ? e.target instanceof Element && !!e.target.closest(v) : typeof v === 'number' ? Math.abs(e[k]-v)<0.02 : e[k]===v));
        if(i<0) return;
        pending.splice(i,1); events.push(e);
        if(!pending.length) request();
      };
      window.addEventListener('qa-render-receipt', committed);
      types.forEach(type => window.addEventListener(type,onEvent,true));
      window.__requestArmedReceipt = request;
    });
  }, { id, scopeSel, expected });
}
async function finishReceipt(page, logAction) {
  const receipt = await page.evaluate(() => window.__ackPromise);
  logAction({ action:'receipt', ...receipt });
  return receipt;
}
async function actionReceipt(page, scopeSel, action, expected, trigger, logAction) {
  await armReceipt(page, scopeSel, expected);
  logAction(action);
  await trigger();
  if (!expected.length) await page.evaluate(() => window.__requestArmedReceipt());
  return finishReceipt(page, logAction);
}
async function mouseMoveWithAck(page, scopeSel, x, y, logAction, options = {}) {
  const old=mousePositions.get(page);
  const expected=old?.x===x && old?.y===y ? [] : [{type:'pointermove',pointerType:'mouse',clientX:x,clientY:y}];
  await actionReceipt(page,scopeSel,{action:'mouse.move',delivery:'trusted CDP',x,y,...options},expected,
    ()=>page.mouse.move(x,y,{steps:options.steps||1}),logAction);
  mousePositions.set(page,{x,y});
}
async function mouseDownWithAck(page, scopeSel, logAction, options = {}) {
  const {x,y}=mousePositions.get(page);
  await actionReceipt(page,scopeSel,{action:'mouse.down',delivery:'trusted CDP',...options},
    [{type:'pointerdown',pointerType:'mouse',clientX:x,clientY:y,button:{left:0,middle:1,right:2}[options.button||'left']}],()=>page.mouse.down(options),logAction);
}
async function mouseUpWithAck(page, scopeSel, logAction, options = {}) {
  const {x,y}=mousePositions.get(page);
  await actionReceipt(page,scopeSel,{action:'mouse.up',delivery:'trusted CDP',...options},
    [{type:'pointerup',pointerType:'mouse',clientX:x,clientY:y,button:{left:0,middle:1,right:2}[options.button||'left']}],()=>page.mouse.up(options),logAction);
}
async function mouseWheelWithAck(page, scopeSel, deltaX, deltaY, logAction, extra = {}) {
  await actionReceipt(page,scopeSel,{action:'mouse.wheel',delivery:'trusted CDP',deltaX,deltaY,...extra},
    [{type:'wheel',deltaX,deltaY,...extra}],()=>page.mouse.wheel(deltaX,deltaY),logAction);
}
async function clickSelectorWithAck(page, scopeSel, selector, logAction) {
  const receipt=await actionReceipt(page,scopeSel,{action:'click',delivery:'trusted CDP',selector},
    [{type:'click',selector}],()=>page.locator(selector).click(),logAction);
  mousePositions.delete(page);
  return receipt;
}
async function clickButtonWithAck(page, scopeSel, ariaLabel, logAction) {
  return clickSelectorWithAck(page,scopeSel,`${scopeSel} [aria-label="${ariaLabel}"]`,logAction);
}
async function cdpTouchWithAck(cdp, page, scopeSel, type, touchPoints, logAction) {
  const old=touchContacts.get(page)||[];
  const changed=type==='touchStart' ? touchPoints.filter(p=>!old.some(q=>q.id===p.id)) : type==='touchMove' ? touchPoints.filter(p=>!old.some(q=>q.id===p.id&&q.x===p.x&&q.y===p.y)) : touchPoints;
  const evType=type==='touchStart'?'pointerdown':type==='touchMove'?'pointermove':'pointerup';
  const receipt=await actionReceipt(page,scopeSel,{action:'cdp.touch',delivery:'trusted CDP',type,touchPoints},
    changed.map(p=>({type:evType,pointerType:'touch',clientX:p.x,clientY:p.y})),
    ()=>cdp.send('Input.dispatchTouchEvent',{type,touchPoints}),logAction);
  touchContacts.set(page,type==='touchEnd'?old.filter(p=>!touchPoints.some(q=>q.id===p.id)):touchPoints);
  return receipt;
}

async function applyFixtureWithAck(page, scopeSel, kind, logAction, runId = "qa-dag-a") {
  await armReceipt(page, scopeSel);
  logAction({ action: "fixture.apply", delivery: "synthetic store publication", kind, runId });
  await page.evaluate(async ({ kind, runId }) => {
    const { flushSync } = (await import("/node_modules/.vite/deps/react-dom.js")).default;
    const { dagStore } = await import("/src/state/dagStore.ts");
    const f = await import("/src/devtools/dagViewportQaFixtures.ts");
    const make = {
      A: f.buildQaDagRunA,
      big: f.buildBigDagRun,
      tall: f.buildTallDagRun,
      empty: f.buildEmptyDagRun,
      updated: f.buildQaDagRunAUpdated,
    }[kind];
    flushSync(() => {
      dagStore.applySnapshot(f.QA_PROJECT_PATH, {
        ...make(),
        runId,
        rootSessionId: "qa-owner",
        updatedAt: new Date().toISOString(),
      });
    });
  }, { kind, runId });
  await page.evaluate(() => window.__requestArmedReceipt());
  await finishReceipt(page, logAction);
}

async function dispatchSyntheticWheelWithAck(page, scopeSel, eventInit, logAction) {
  await armReceipt(page, scopeSel, [{type:'wheel',...eventInit}]);
  logAction({ action: "synthetic.wheel", delivery: "synthetic DOM", ...eventInit });
  const result = await page.evaluate(async ({ scopeSel, eventInit }) => {
    const { flushSync } = (await import("/node_modules/.vite/deps/react-dom.js")).default;
    const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
    const r = v.getBoundingClientRect();
    const e = new WheelEvent("wheel", {
      bubbles: true,
      cancelable: true,
      clientX: r.x + r.width / 2,
      clientY: r.y + r.height / 2,
      ...eventInit,
    });
    flushSync(() => v.dispatchEvent(e));
    return { defaultPrevented: e.defaultPrevented };
  }, { scopeSel, eventInit });
  await finishReceipt(page, logAction);
  return result;
}

async function measuredAction(page, scopeSel, action, trigger, logAction, target = null) {
  await armReceipt(page, scopeSel);
  await page.evaluate(action=>{window.__qaPageResizeTarget=action.action.startsWith('page.setViewportSize')?(action.size||{width:action.width,height:action.height}):null;},action);
  await page.evaluate(({scopeSel,target}) => {
    const v=document.querySelector(scopeSel+' [data-testid="dag-viewport"]');
    const before=v.getBoundingClientRect();
    window.__measurePromise=new Promise((resolve,reject)=>{
      const timer=setTimeout(()=>{ob.disconnect();reject(Error('exact resize delivery timeout'));},5000);
      const ob=new ResizeObserver(entries=>{
        const r=entries[0].contentRect;
        const matches=target ? Math.abs(r.width-target.width)<.1&&Math.abs(r.height-target.height)<.1 : Math.abs(r.width-before.width)>.1||Math.abs(r.height-before.height)>.1;
        if(!matches)return;
        clearTimeout(timer);ob.disconnect();resolve({width:r.width,height:r.height});
        window.__requestArmedReceipt();
      });
      if(window.__qaPageResizeTarget) {
        const onResize=()=>{
          const t=window.__qaPageResizeTarget;
          if(innerWidth!==t.width||innerHeight!==t.height)return;
          window.removeEventListener('resize',onResize);
          const r=v.getBoundingClientRect();
          if(Math.abs(r.width-before.width)<.1&&Math.abs(r.height-before.height)<.1){
            clearTimeout(timer);ob.disconnect();resolve({width:r.width,height:r.height,unchanged:true});window.__requestArmedReceipt();
          }
        };
        window.addEventListener('resize',onResize);
      }
      ob.observe(v);
    });
  },{scopeSel,target});
  logAction(action);
  await trigger();
  const dimensions=await page.evaluate(()=>window.__measurePromise);
  logAction({action:'resize.delivery',dimensions});
  return finishReceipt(page,logAction);
}
async function resizeViewportWithAck(page, scopeSel, width, height, logAction) {
  return measuredAction(page,scopeSel,{action:'viewport.resize',delivery:'synthetic CSS',width,height},
    ()=>page.evaluate(({scopeSel,width,height})=>{
      const v=document.querySelector(scopeSel+' [data-testid="dag-viewport"]');
      v.style.flex='none';v.style.width=`${width}px`;v.style.height=`${height}px`;
    },{scopeSel,width,height}),logAction,{width,height});
}
async function restoreViewportWithAck(page, scopeSel, logAction) {
  return measuredAction(page,scopeSel,{action:'viewport.restore',delivery:'synthetic CSS'},
    ()=>page.evaluate(scopeSel=>{
      const v=document.querySelector(scopeSel+' [data-testid="dag-viewport"]');
      v.style.flex='';v.style.width='';v.style.height='';
    },scopeSel),logAction);
}
async function keyPressWithAck(page, key, logAction) {
  return actionReceipt(page,SCOPE.modal,{action:'keyboard.press',delivery:'trusted CDP',key},
    [{type:'keyup',key}],()=>page.keyboard.press(key),logAction);
}

// Position a real card through a real pan, not by clamping an offscreen point.
async function visibleCard(page,host,logAction) {
  const scopeSel=SCOPE[host];
  let g=await captureGeometry(page,host);
  const x=Math.round(g.viewport.x+30),y=Math.round(g.viewport.y+80);
  const dx=x-(g.card.x+Math.min(g.card.width/2,40));
  const dy=y-(g.card.y+g.card.height/2);
  const sx=Math.round(g.viewport.x+g.viewport.width/2),sy=Math.round(g.viewport.y+g.viewport.height/2);
  await mouseMoveWithAck(page,scopeSel,sx,sy,logAction);
  await mouseDownWithAck(page,scopeSel,logAction);
  await mouseMoveWithAck(page,scopeSel,sx+dx,sy+dy,logAction);
  await mouseUpWithAck(page,scopeSel,logAction);
  const hit=await page.evaluate(({x,y})=>document.elementFromPoint(x,y)?.closest('[data-testid^="dag-node-"]')?.getAttribute('data-testid'),{x,y});
  logAction({action:'card.hit-test',x,y,expected:'dag-node-a',actual:hit});
  if(hit!=='dag-node-a')throw Error(`card start hit ${hit}, not dag-node-a`);
  g=await captureGeometry(page,host);
  return {x,y,geometry:g};
}

async function openFixture(page, host, fixture, logAction) {
  if (logAction) logAction({ action: "openFixture", host, fixture });
  await page.goto(url, { waitUntil: "load" });
  if (host === "modal") {
    const badge = page.locator('[data-testid="qa-modal-host"] [data-testid="dag-pane-badge-button"]');
    await badge.waitFor({ state: "visible", timeout: PAGE_TIMEOUT_MS });
    await clickSelectorWithAck(page,SCOPE[host],'[data-testid="qa-modal-host"] [data-testid="dag-pane-badge-button"]',logAction);
  }
  const scopeSel = SCOPE[host];
  await page.locator(`${scopeSel} [data-testid="dag-graph-view"][data-run-id="${fixture.runId}"]`)
    .waitFor({ state: "visible", timeout: PAGE_TIMEOUT_MS });
  await page.locator(`${scopeSel} [data-testid="dag-header"]`).waitFor({ state: "visible", timeout: PAGE_TIMEOUT_MS });
  await page.locator(`${scopeSel} [data-testid="dag-edge-layer"]`).waitFor({ state: "visible", timeout: PAGE_TIMEOUT_MS });
  await page.locator(`${scopeSel} [data-testid="${fixture.card}"]`).waitFor({ state: "visible", timeout: PAGE_TIMEOUT_MS });
  await page.locator(`${scopeSel} [data-testid="${fixture.edge}"]`).waitFor({ state: "visible", timeout: PAGE_TIMEOUT_MS });
}

function isViteHmrClientNoise(message) {
  return (
    message.includes("ws://127.0.0.1:5173") ||
    message.includes("[vite] failed to connect to websocket") ||
    message.includes("WebSocket closed without opened")
  );
}

// --------------------------------------------------------------------------
// Baseline Runner
// --------------------------------------------------------------------------

function summarizeBaseline(results, hosts) {
  const passedChecks = [];
  const failedChecks = [];
  const missingSetup = [];
  const errors = [];
  for (const h of hosts) {
    const data = results.hosts[h];
    if (!data) continue;
    for (const c of data.checks) {
      if (c.kind === "setup" && !c.pass) missingSetup.push(`${h}:${c.id}`);
      if (c.expectation === "fail-red") {
        if (!c.pass) passedChecks.push(`${h}:${c.id}`);
        else failedChecks.push(`${h}:${c.id}`);
      } else {
        if (!c.pass) failedChecks.push(`${h}:${c.id}`);
        else passedChecks.push(`${h}:${c.id}`);
      }
    }
    if (data.scenarioError) errors.push(`${h}: ${data.scenarioError}`);
    if (data.pageErrors && data.pageErrors.length > 0) {
      errors.push(`${h} pageErrors: ${JSON.stringify(data.pageErrors)}`);
    }
  }
  const ok = missingSetup.length === 0 && failedChecks.length === 0 && errors.length === 0 && passedChecks.length > 0;
  return { passedChecks, failedChecks, missingSetup, errors, ok };
}

async function runHostBaseline(browser, host, evidencePaths) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, deviceScaleFactor: 1 });
  const page = await context.newPage();
  const consoleErrors = [];
  const pageErrors = [];
  const actions = [];
  const checks = [];
  const environmentNoise = [];

  const logAction = (entry) => actions.push({ ...entry, at: new Date().toISOString() });

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      const text = msg.text();
      if (isViteHmrClientNoise(text)) environmentNoise.push({ source: "console", text });
      else consoleErrors.push(text);
    }
  });
  page.on("pageerror", (err) => {
    const text = String(err);
    if (isViteHmrClientNoise(text)) environmentNoise.push({ source: "pageerror", text });
    else pageErrors.push(text);
  });

  try {
    await openFixture(page, host, FIXTURE_A, logAction);
    const scopeSel = SCOPE[host];
    const geo0 = await captureGeometry(page, host);
    checks.push(check("setup-graph-exists", "setup", "pass", Boolean(geo0 && geo0.viewport), { geo0 }));

    const vp = geo0.viewport;
    const startX = vp.x + Math.min(60, vp.width / 4);
    const startY = vp.y + Math.min(60, vp.height / 4);

    await mouseMoveWithAck(page, scopeSel, startX, startY, logAction);
    await mouseDownWithAck(page, scopeSel, logAction);
    await mouseMoveWithAck(page, scopeSel, startX + 120, startY + 80, logAction);
    await mouseUpWithAck(page, scopeSel, logAction);
    const geo1 = await captureGeometry(page, host);
    const cardDelta = delta(geo0.card, geo1.card);
    const edgeDelta = delta(geo0.edgeEndpoint, geo1.edgeEndpoint);

    checks.push(check("S1-pan-card-moves-120-80", "behavioral", "fail-red", movedBy(cardDelta, 120, 80, 1), { cardDelta }));
    checks.push(check("S1-pan-edge-endpoint-moves-120-80", "behavioral", "fail-red", movedBy(edgeDelta, 120, 80, 1), { edgeDelta }));

    const centerX = vp.x + vp.width / 2;
    const centerY = vp.y + vp.height / 2;
    await mouseMoveWithAck(page, scopeSel, centerX, centerY, logAction);
    await mouseWheelWithAck(page, scopeSel, 0, -120, logAction);
    const geo2 = await captureGeometry(page, host);
    checks.push(check("S2-zoom-in-increases-scale", "behavioral", "fail-red", geo2.ctmScale > geo1.ctmScale + 0.001, {
      before: geo1.ctmScale,
      after: geo2.ctmScale,
    }));

    await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
    const geoFit = await captureGeometry(page, host);
    checks.push(check("S3-controls-fit-graph", "behavioral", "fail-red", fitOk(geoFit), { geoFit }));

    const ssPath = join(evidenceDir, `${host}-baseline.png`);
    await page.screenshot({ path: ssPath });
    logAction({ action: "screenshot", path: ssPath });
    evidencePaths.push(ssPath);

    return { checks, actions, screenshots: evidencePaths, pageErrors, consoleErrors, environmentNoise };
  } catch (err) {
    return {
      checks,
      actions,
      screenshots: evidencePaths,
      pageErrors,
      consoleErrors,
      environmentNoise,
      scenarioError: String(err && err.stack ? err.stack : err),
    };
  } finally {
    await context.close();
  }
}

// --------------------------------------------------------------------------
// Green Runner Matrix (Desktop 1280x900 & Mobile 390x844 across Real Hosts)
// --------------------------------------------------------------------------

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
  return { passedChecks, failedChecks, missingSetup, errors, ok };
}

async function runHostScenarioMatrix(browser, host, size, evidencePaths, runnerActions, hostResults) {
  const isDesktop = size.width >= 1000;
  const sizeTag = isDesktop ? "desktop" : "mobile";
  const prefix = `${sizeTag}`;

  const context = await browser.newContext({
    viewport: { width: size.width, height: size.height },
    deviceScaleFactor: 1,
    hasTouch: !isDesktop,
  });
  const page = await context.newPage();
  const cdp = await context.newCDPSession(page);

  const consoleErrors = hostResults.consoleErrors;
  const pageErrors = hostResults.pageErrors;
  const environmentNoise = hostResults.environmentNoise;
  const checks = hostResults.checks;

  const logAction = (entry) => {
    const item = { ...entry, host, size: `${size.width}x${size.height}`, at: new Date().toISOString() };
    hostResults.actions.push(item);
    runnerActions.push(item);
  };

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      const text = msg.text();
      if (isViteHmrClientNoise(text)) environmentNoise.push({ source: "console", text });
      else consoleErrors.push(text);
    }
  });
  page.on("pageerror", (err) => {
    const text = String(err);
    if (isViteHmrClientNoise(text)) environmentNoise.push({ source: "pageerror", text });
    else pageErrors.push(text);
  });

  const scopeSel = SCOPE[host];

  try {
    await openFixture(page, host, FIXTURE_A, logAction);
    await page.evaluate(()=>{
      window.__qaDelivered=[];
      for(const type of ['pointerdown','pointermove','pointerup','pointercancel','lostpointercapture','click','focusin','keydown','keyup','blur','visibilitychange'])
        window.addEventListener(type,e=>window.__qaDelivered.push({type,delivery:e.isTrusted?'trusted browser':'synthetic DOM',pointerId:e.pointerId,pointerType:e.pointerType,x:e.clientX,y:e.clientY,buttons:e.buttons,key:e.key,target:e.target instanceof Element?e.target.closest('[data-testid]')?.getAttribute('data-testid'):null}),true);
    });

    // Initial screenshot
    const ssInitName = `${host}-${size.width}-A.png`;
    const ssInitPath = join(evidenceDir, ssInitName);
    await page.screenshot({ path: ssInitPath });
    logAction({ action: "screenshot", path: ssInitPath });
    evidencePaths.push(ssInitPath);

    if (isDesktop) {
      const ssDesktopName = `${host}-initial.png`;
      const ssDesktopPath = join(evidenceDir, ssDesktopName);
      await page.screenshot({ path: ssDesktopPath });
      evidencePaths.push(ssDesktopPath);
      if (host === "modal") {
        const dPath = join(evidenceDir, "desktop.png");
        await page.screenshot({ path: dPath });
        evidencePaths.push(dPath);
      }
    } else {
      const ssMobileName = `${host}-mobile.png`;
      const ssMobilePath = join(evidenceDir, ssMobileName);
      await page.screenshot({ path: ssMobilePath });
      evidencePaths.push(ssMobilePath);
      if (host === "modal") {
        const mPath = join(evidenceDir, "mobile.png");
        await page.screenshot({ path: mPath });
        evidencePaths.push(mPath);
      }
    }

    const geo0 = await captureGeometry(page, host);
    checks.push(check(`${prefix}-setup-graph-exists`, "setup", "pass", Boolean(geo0 && geo0.viewport), { geo0 }));
    checks.push(check(`${prefix}-no-camera-scrollbars-initial`, "behavioral", "pass",
      geo0.style.overflow === "hidden" && geo0.style.touchAction === "none" && geo0.scroll.top === 0 && geo0.scroll.left === 0,
      { scroll: geo0.scroll, style: geo0.style },
    ));

    const vp = geo0.viewport;

    // ------------------------------------------------------------------------
    // Scenario S1: Pan
    // ------------------------------------------------------------------------
    const startX = Math.round(vp.x + (vp.width >= 250 ? 100 : 30));
    const startY = Math.round(vp.y + 100);
    const panDx = 120;
    const panDy = 80;

    // 1. Pan on background
    await mouseMoveWithAck(page, scopeSel, startX, startY, logAction);
    await mouseDownWithAck(page, scopeSel, logAction);
    await mouseMoveWithAck(page, scopeSel, startX + panDx, startY + panDy, logAction, { steps: 5 });
    await mouseUpWithAck(page, scopeSel, logAction);

    const geo1 = await captureGeometry(page, host);
    const cardDelta = delta(geo0.card, geo1.card);
    const edgeDelta = delta(geo0.edgeEndpoint, geo1.edgeEndpoint);
    const headerDelta = delta(geo0.header, geo1.header);

    checks.push(check(`${prefix}-S1-pan-card-moves-120-80`, "behavioral", "pass", movedBy(cardDelta, panDx, panDy, 1), { cardDelta }));
    checks.push(check(`${prefix}-S1-pan-edge-endpoint-moves-120-80`, "behavioral", "pass", movedBy(edgeDelta, panDx, panDy, 1), { edgeDelta }));
    checks.push(check(`${prefix}-S1-pan-cards-and-edges-aligned`, "behavioral", "pass",
      Math.abs(cardDelta.x - edgeDelta.x) <= 1 && Math.abs(cardDelta.y - edgeDelta.y) <= 1,
      { cardDelta, edgeDelta },
    ));
    checks.push(check(`${prefix}-S1-pan-header-unchanged`, "behavioral", "pass", movedBy(headerDelta, 0, 0, 0.5), { headerDelta }));
    checks.push(check(`${prefix}-S1-pan-scroll-unchanged`, "behavioral", "pass", geo1.scroll.top === 0 && geo1.scroll.left === 0, { scroll: geo1.scroll }));
    checks.push(check(`${prefix}-S1-pan-no-camera-scrollbars`, "behavioral", "pass",
      geo1.style.overflow === "hidden" && geo1.style.touchAction === "none" && geo1.scroll.top === 0 && geo1.scroll.left === 0,
      { scroll: geo1.scroll, style: geo1.style },
    ));

    if (isDesktop) {
      const ssPanPath = join(evidenceDir, `${host}-after-pan.png`);
      await page.screenshot({ path: ssPanPath });
      evidencePaths.push(ssPanPath);
    }

    // 2. Pan starting on a card
    const cardCenter = await visibleCard(page, host, logAction);
    const cardStartGeo = cardCenter.geometry;
    await mouseMoveWithAck(page, scopeSel, cardCenter.x, cardCenter.y, logAction);
    await mouseDownWithAck(page, scopeSel, logAction);
    await mouseMoveWithAck(page, scopeSel, cardCenter.x + panDx, cardCenter.y + panDy, logAction, { steps: 5 });
    await mouseUpWithAck(page, scopeSel, logAction);
    const geoCardPan = await captureGeometry(page, host);
    const cardPanCardDelta = delta(cardStartGeo.card, geoCardPan.card);
    const cardPanEdgeDelta = delta(cardStartGeo.edgeEndpoint, geoCardPan.edgeEndpoint);
    const cardPanHeaderDelta = delta(cardStartGeo.header, geoCardPan.header);

    checks.push(check(`${prefix}-S1-pan-from-card-card-moves-120-80`, "behavioral", "pass", movedBy(cardPanCardDelta, panDx, panDy, 1), { cardPanCardDelta }));
    checks.push(check(`${prefix}-S1-pan-from-card-edge-moves-120-80`, "behavioral", "pass", movedBy(cardPanEdgeDelta, panDx, panDy, 1), { cardPanEdgeDelta }));
    checks.push(check(`${prefix}-S1-pan-from-card-header-unchanged`, "behavioral", "pass", movedBy(cardPanHeaderDelta, 0, 0, 0.5), { cardPanHeaderDelta }));
    checks.push(check(`${prefix}-S1-pan-from-card-no-camera-scrollbars`, "behavioral", "pass",
      geoCardPan.style.overflow === "hidden" && geoCardPan.style.touchAction === "none" && geoCardPan.scroll.top === 0 && geoCardPan.scroll.left === 0,
      { scroll: geoCardPan.scroll, style: geoCardPan.style },
    ));

    // 3. Pan at exact scale 0.5x (both background and card-start)
    await clickButtonWithAck(page, scopeSel, "Reset zoom to 100%", logAction);
    let curG = await captureGeometry(page, host);
    const deltaFor05 = -Math.log(0.5 / curG.ctmScale) / 0.002;
    await dispatchSyntheticWheelWithAck(page, scopeSel, { deltaY: deltaFor05, deltaMode: 0 }, logAction);
    const g05 = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S1-pan-exact-0.5x-scale`, "behavioral", "pass", Math.abs(g05.ctmScale - 0.5) <= 0.005, { scale: g05.ctmScale }));

    await mouseMoveWithAck(page, scopeSel, startX, startY, logAction);
    await mouseDownWithAck(page, scopeSel, logAction);
    await mouseMoveWithAck(page, scopeSel, startX + panDx, startY + panDy, logAction, { steps: 5 });
    await mouseUpWithAck(page, scopeSel, logAction);
    const g05Pan1 = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S1-pan-at-0.5x-moves-120-80`, "behavioral", "pass",
      movedBy(delta(g05.card, g05Pan1.card), panDx, panDy, 1) &&
      movedBy(delta(g05.edgeEndpoint, g05Pan1.edgeEndpoint), panDx, panDy, 1) &&
      movedBy(delta(g05.header, g05Pan1.header), 0, 0, 0.5) &&
      g05Pan1.style.overflow === "hidden" && g05Pan1.style.touchAction === "none" && g05Pan1.scroll.top === 0 && g05Pan1.scroll.left === 0,
      { deltaCard: delta(g05.card, g05Pan1.card), deltaEdge: delta(g05.edgeEndpoint, g05Pan1.edgeEndpoint) },
    ));

    const card05Center = await visibleCard(page, host, logAction);
    const g05CardStart = card05Center.geometry;
    await mouseMoveWithAck(page, scopeSel, card05Center.x, card05Center.y, logAction);
    await mouseDownWithAck(page, scopeSel, logAction);
    await mouseMoveWithAck(page, scopeSel, card05Center.x + panDx, card05Center.y + panDy, logAction, { steps: 5 });
    await mouseUpWithAck(page, scopeSel, logAction);
    const g05Pan2 = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S1-pan-from-card-at-0.5x-moves-120-80`, "behavioral", "pass",
      movedBy(delta(g05CardStart.card, g05Pan2.card), panDx, panDy, 1) &&
      movedBy(delta(g05CardStart.edgeEndpoint, g05Pan2.edgeEndpoint), panDx, panDy, 1) &&
      movedBy(delta(g05CardStart.header, g05Pan2.header), 0, 0, 0.5) &&
      g05Pan2.style.overflow === "hidden" && g05Pan2.style.touchAction === "none" && g05Pan2.scroll.top === 0 && g05Pan2.scroll.left === 0,
      { deltaCard: delta(g05CardStart.card, g05Pan2.card) },
    ));

    // 4. Pan at exact scale 2.0x (both background and card-start)
    await clickButtonWithAck(page, scopeSel, "Reset zoom to 100%", logAction);
    curG = await captureGeometry(page, host);
    const deltaFor20 = -Math.log(2.0 / curG.ctmScale) / 0.002;
    await dispatchSyntheticWheelWithAck(page, scopeSel, { deltaY: deltaFor20, deltaMode: 0 }, logAction);
    const g20 = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S1-pan-exact-2.0x-scale`, "behavioral", "pass", Math.abs(g20.ctmScale - 2.0) <= 0.005, { scale: g20.ctmScale }));

    await mouseMoveWithAck(page, scopeSel, startX, startY, logAction);
    await mouseDownWithAck(page, scopeSel, logAction);
    await mouseMoveWithAck(page, scopeSel, startX + panDx, startY + panDy, logAction, { steps: 5 });
    await mouseUpWithAck(page, scopeSel, logAction);
    const g20Pan1 = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S1-pan-at-2.0x-moves-120-80`, "behavioral", "pass",
      movedBy(delta(g20.card, g20Pan1.card), panDx, panDy, 1) &&
      movedBy(delta(g20.edgeEndpoint, g20Pan1.edgeEndpoint), panDx, panDy, 1) &&
      movedBy(delta(g20.header, g20Pan1.header), 0, 0, 0.5) &&
      g20Pan1.style.overflow === "hidden" && g20Pan1.style.touchAction === "none" && g20Pan1.scroll.top === 0 && g20Pan1.scroll.left === 0,
      { deltaCard: delta(g20.card, g20Pan1.card), deltaEdge: delta(g20.edgeEndpoint, g20Pan1.edgeEndpoint) },
    ));

    const card20Center = await visibleCard(page, host, logAction);
    const g20CardStart = card20Center.geometry;
    await mouseMoveWithAck(page, scopeSel, card20Center.x, card20Center.y, logAction);
    await mouseDownWithAck(page, scopeSel, logAction);
    await mouseMoveWithAck(page, scopeSel, card20Center.x + panDx, card20Center.y + panDy, logAction, { steps: 5 });
    await mouseUpWithAck(page, scopeSel, logAction);
    const g20Pan2 = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S1-pan-from-card-at-2.0x-moves-120-80`, "behavioral", "pass",
      movedBy(delta(g20CardStart.card, g20Pan2.card), panDx, panDy, 1) &&
      movedBy(delta(g20CardStart.edgeEndpoint, g20Pan2.edgeEndpoint), panDx, panDy, 1) &&
      movedBy(delta(g20CardStart.header, g20Pan2.header), 0, 0, 0.5) &&
      g20Pan2.style.overflow === "hidden" && g20Pan2.style.touchAction === "none" && g20Pan2.scroll.top === 0 && g20Pan2.scroll.left === 0,
      { deltaCard: delta(g20CardStart.card, g20Pan2.card) },
    ));

    if (host === "standalone") {
      checks.push(check(`${prefix}-S1-standalone-sibling-leaves-unchanged-during-pan`, "behavioral", "pass",
        JSON.stringify(g20Pan2.leaves) === JSON.stringify(geo0.leaves),
        { before: geo0.leaves, after: g20Pan2.leaves },
      ));
    }

    // ------------------------------------------------------------------------
    // Scenario S2: Zoom
    // ------------------------------------------------------------------------
    await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
    const gz0 = await captureGeometry(page, host);
    const centerX = Math.round(vp.x + vp.width / 2);
    const centerY = Math.round(vp.y + vp.height / 2);

    await mouseMoveWithAck(page, scopeSel, centerX, centerY, logAction);
    await mouseWheelWithAck(page, scopeSel, 0, -120, logAction);
    const gz1 = await captureGeometry(page, host);

    const worldAnchorX = (centerX - vp.x - gz0.camera.x) / gz0.camera.scale;
    const worldAnchorY = (centerY - vp.y - gz0.camera.y) / gz0.camera.scale;
    const projectedAfterX = vp.x + gz1.camera.x + worldAnchorX * gz1.camera.scale;
    const projectedAfterY = vp.y + gz1.camera.y + worldAnchorY * gz1.camera.scale;
    const anchorErrX = Math.abs(projectedAfterX - centerX);
    const anchorErrY = Math.abs(projectedAfterY - centerY);

    checks.push(check(`${prefix}-S2-zoom-in-increases-scale`, "behavioral", "pass", gz1.ctmScale > gz0.ctmScale + 0.001, {
      before: gz0.ctmScale,
      after: gz1.ctmScale,
    }));
    checks.push(check(`${prefix}-S2-zoom-in-anchor-world-point-stable`, "behavioral", "pass",
      anchorErrX <= 1.0 && anchorErrY <= 1.0,
      { anchorErrX, anchorErrY },
    ));
    checks.push(check(`${prefix}-S2-document-and-visual-viewport-unchanged`, "behavioral", "pass",
      gz1.document.scrollX === 0 && gz1.document.scrollY === 0 && (gz1.document.vvScale === null || gz1.document.vvScale === 1),
      { document: gz1.document },
    ));

    if (isDesktop) {
      const ssZIn = join(evidenceDir, `${host}-after-wheel-in.png`);
      await page.screenshot({ path: ssZIn });
      evidencePaths.push(ssZIn);
    }

    await mouseWheelWithAck(page, scopeSel, 0, 120, logAction);
    const gz2 = await captureGeometry(page, host);
    const projReturnX = vp.x + gz2.camera.x + worldAnchorX * gz2.camera.scale;
    const projReturnY = vp.y + gz2.camera.y + worldAnchorY * gz2.camera.scale;

    checks.push(check(`${prefix}-S2-zoom-out-returns-scale`, "behavioral", "pass", Math.abs(gz2.ctmScale - gz0.ctmScale) <= 0.002, {
      expected: gz0.ctmScale,
      actual: gz2.ctmScale,
    }));
    checks.push(check(`${prefix}-S2-zoom-out-anchor-world-point-stable`, "behavioral", "pass",
      Math.abs(projReturnX - centerX) <= 1.0 && Math.abs(projReturnY - centerY) <= 1.0,
      { errX: Math.abs(projReturnX - centerX), errY: Math.abs(projReturnY - centerY) },
    ));

    if (isDesktop) {
      const ssZOut = join(evidenceDir, `${host}-after-wheel-out.png`);
      await page.screenshot({ path: ssZOut });
      evidencePaths.push(ssZOut);
    }

    // Ctrl+wheel
    await page.keyboard.down("Control");
    await mouseWheelWithAck(page, scopeSel, 0, -120, logAction, { ctrlKey: true });
    await page.keyboard.up("Control");
    const gzCtrl = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S2-ctrl-wheel-same-factor`, "behavioral", "pass",
      Math.abs(gzCtrl.ctmScale / gz2.ctmScale - Math.exp(0.24)) <= 0.02,
      { ratio: gzCtrl.ctmScale / gz2.ctmScale, expected: Math.exp(0.24) },
    ));

    // DeltaModes normalized: 0 (pixel), 1 (line = 16px), 2 (page = vp.height)
    await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
    const gBeforeMode = await captureGeometry(page, host);
    await dispatchSyntheticWheelWithAck(page, scopeSel, { deltaY: 16, deltaMode: 0 }, logAction);
    const gMode0 = await captureGeometry(page, host);
    await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
    await dispatchSyntheticWheelWithAck(page, scopeSel, { deltaY: 1, deltaMode: 1 }, logAction);
    const gMode1 = await captureGeometry(page, host);
    await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
    await dispatchSyntheticWheelWithAck(page, scopeSel, { deltaY: 16 / vp.height, deltaMode: 2 }, logAction);
    const gMode2 = await captureGeometry(page, host);

    const expRatioMode = Math.exp(-0.002 * 16);
    checks.push(check(`${prefix}-S2-wheel-deltaMode-0-normalized`, "behavioral", "pass",
      Math.abs(gMode0.ctmScale / gBeforeMode.ctmScale - expRatioMode) <= 0.005,
      { ratio: gMode0.ctmScale / gBeforeMode.ctmScale, expRatioMode },
    ));
    checks.push(check(`${prefix}-S2-wheel-deltaMode-1-normalized`, "behavioral", "pass",
      Math.abs(gMode1.ctmScale / gBeforeMode.ctmScale - expRatioMode) <= 0.005,
      { ratio: gMode1.ctmScale / gBeforeMode.ctmScale, expRatioMode },
    ));
    checks.push(check(`${prefix}-S2-wheel-deltaMode-2-normalized`, "behavioral", "pass",
      Math.abs(gMode2.ctmScale / gBeforeMode.ctmScale - expRatioMode) <= 0.005,
      { ratio: gMode2.ctmScale / gBeforeMode.ctmScale, expRatioMode },
    ));

    // Horizontal wheel invariant: default not prevented, camera unchanged
    const gBeforeH = await captureGeometry(page, host);
    const hRes = await dispatchSyntheticWheelWithAck(page, scopeSel, { deltaX: 120, deltaY: 0, deltaMode: 0 }, logAction);
    const gAfterH = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S2-horizontal-wheel-camera-unchanged`, "behavioral", "pass",
      same(gBeforeH.camera, gAfterH.camera) && hRes.defaultPrevented === false,
      { defaultPrevented: hRes.defaultPrevented, before: gBeforeH.camera, after: gAfterH.camera },
    ));

    // Zoom clamp at max scale (3.0)
    for (let i = 0; i < 8; i++) {
      await dispatchSyntheticWheelWithAck(page, scopeSel, { deltaY: -500, deltaMode: 0 }, logAction);
    }
    const gClampMax0 = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S2-zoom-clamp-max-scale`, "behavioral", "pass",
      Math.abs(gClampMax0.ctmScale - 3.0) <= 0.005 && gClampMax0.inAria === "true",
      { scale: gClampMax0.ctmScale, inAria: gClampMax0.inAria },
    ));
    const extraMaxRes = await dispatchSyntheticWheelWithAck(page, scopeSel, { deltaY: -300, deltaMode: 0 }, logAction);
    const gClampMax1 = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S2-zoom-clamp-max-no-drift`, "behavioral", "pass",
      same(gClampMax0.camera, gClampMax1.camera) && extraMaxRes.defaultPrevented === true,
      { before: gClampMax0.camera, after: gClampMax1.camera, defaultPrevented: extraMaxRes.defaultPrevented },
    ));

    // Zoom clamp at min scale
    for (let i = 0; i < 12; i++) {
      await dispatchSyntheticWheelWithAck(page, scopeSel, { deltaY: 500, deltaMode: 0 }, logAction);
    }
    const gClampMin0 = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S2-zoom-clamp-min-scale`, "behavioral", "pass",
      gClampMin0.outAria === "true",
      { scale: gClampMin0.ctmScale, outAria: gClampMin0.outAria },
    ));
    const extraMinRes = await dispatchSyntheticWheelWithAck(page, scopeSel, { deltaY: 300, deltaMode: 0 }, logAction);
    const gClampMin1 = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S2-zoom-clamp-min-no-drift`, "behavioral", "pass",
      same(gClampMin0.camera, gClampMin1.camera) && extraMinRes.defaultPrevented === true,
      { before: gClampMin0.camera, after: gClampMin1.camera, defaultPrevented: extraMinRes.defaultPrevented },
    ));

    // ------------------------------------------------------------------------
    // Scenario S3: Controls, Fit, Limit Focusability, Header Clipping
    // ------------------------------------------------------------------------
    // Fit after far pan
    await mouseMoveWithAck(page, scopeSel, startX, startY, logAction);
    await mouseDownWithAck(page, scopeSel, logAction);
    await mouseMoveWithAck(page, scopeSel, startX + 500, startY + 400, logAction, { steps: 5 });
    await mouseUpWithAck(page, scopeSel, logAction);
    await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
    const gFit = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S3-controls-fit-graph-formula`, "behavioral", "pass", fitOk(gFit), { gFit }));
    checks.push(check(`${prefix}-S3-controls-fit-graph-all-inside`, "behavioral", "pass", allInside(gFit), { gFit }));

    // Zoom in, out, reset buttons
    await clickButtonWithAck(page, scopeSel, "Zoom in", logAction);
    const gPlus = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S3-controls-zoom-in-button`, "behavioral", "pass",
      Math.abs(gPlus.ctmScale - gFit.ctmScale * 1.2) <= 0.005,
      { expected: gFit.ctmScale * 1.2, actual: gPlus.ctmScale },
    ));
    await clickButtonWithAck(page, scopeSel, "Zoom out", logAction);
    const gMinus = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S3-controls-zoom-out-button`, "behavioral", "pass",
      Math.abs(gMinus.ctmScale - gFit.ctmScale) <= 0.005,
      { expected: gFit.ctmScale, actual: gMinus.ctmScale },
    ));
    await clickButtonWithAck(page, scopeSel, "Reset zoom to 100%", logAction);
    const gReset = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S3-controls-reset-zoom-button`, "behavioral", "pass",
      Math.abs(gReset.ctmScale - 1.0) <= 0.005,
      { actual: gReset.ctmScale },
    ));

    // Prove the named limit BEFORE each individual focus action.
    for (const [limit,deltaY] of [['min',1000],['max',-1000]]) {
      await mouseMoveWithAck(page,scopeSel,centerX,centerY,logAction);
      for(let i=0;i<8;i++) await mouseWheelWithAck(page,scopeSel,0,deltaY,logAction);
      for(const label of ['Zoom out','Reset zoom to 100%','Zoom in','Fit graph']) {
        const before=await captureGeometry(page,host);
        const expected=limit==='max'?3:Math.min(.1,fitExpected(before.viewport,before.contentWidth,before.contentHeight).scale);
        const atLimit=close(before.camera.scale,expected,.00001);
        const selector=`${scopeSel} [aria-label="${label}"]`;
        await actionReceipt(page,scopeSel,{action:'focus',delivery:'DOM focus()',selector},[],()=>page.locator(selector).focus(),logAction);
        const focused=await page.locator(selector).evaluate(e=>e===document.activeElement);
        checks.push(check(`${prefix}-S3-${limit}-${label}-focus`, 'behavioral','pass',atLimit&&focused,{before:before.camera,expected,focused}));
      }
    }

    // Fit and Header Clipping across fixtures A, big, tall
    for (const kind of ["A", "big", "tall"]) {
      await applyFixtureWithAck(page, scopeSel, kind, logAction);
      await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
      const gFixture = await captureGeometry(page, host);
      const isFitOk = fitOk(gFixture);
      const isInside = allInside(gFixture);
      checks.push(check(`${prefix}-S3-fit-${kind}`, "behavioral", "pass",
        isFitOk && isInside && (kind !== "big" || gFixture.ctmScale < 0.1),
        { fitOk: isFitOk, allInside: isInside, scale: gFixture.ctmScale, camera: gFixture.camera },
      ));

      const ssFixtureName = `${host}-${size.width}-${kind}.png`;
      const ssFixturePath = join(evidenceDir, ssFixtureName);
      await page.screenshot({ path: ssFixturePath });
      evidencePaths.push(ssFixturePath);

      if (isDesktop && host === "standalone") {
        if (kind === "big") {
          const p = join(evidenceDir, "standalone-big-fit.png");
          await page.screenshot({ path: p });
          evidencePaths.push(p);
        } else if (kind === "tall") {
          const p = join(evidenceDir, "standalone-tall-fit.png");
          await page.screenshot({ path: p });
          evidencePaths.push(p);
        }
      }

      const controlBounds=await page.evaluate(scopeSel=>{
        const graph=document.querySelector(scopeSel+' [data-testid="dag-graph-view"]').getBoundingClientRect();
        return {overflow:document.documentElement.scrollWidth-document.documentElement.clientWidth,
          controls:[...document.querySelectorAll(scopeSel+' [data-testid="dag-controls"] button')].map(e=>{
            const r=e.getBoundingClientRect();return {label:e.getAttribute('aria-label'),x:r.x,y:r.y,width:r.width,height:r.height,inBounds:r.left>=graph.left&&r.right<=graph.right&&r.top>=0&&r.bottom<=innerHeight};})};
      },scopeSel);
      checks.push(check(`${prefix}-S3-control-bounds-${kind}`,'behavioral','pass',controlBounds.overflow===0&&controlBounds.controls.length===4&&controlBounds.controls.every(c=>c.inBounds),controlBounds));

      // Check header clipping (Contract 12)
      const clipping = await page.locator(`${scopeSel} [data-testid="dag-header"]`).evaluate((h) => {
        const count = [...h.querySelectorAll("span")].find((e) => e.textContent.includes(" done,"));
        const graph = h.closest('[data-testid="dag-graph-view"]');
        const r = graph.getBoundingClientRect();
        const c = count ? count.getBoundingClientRect() : { right: 0 };
        let textRight = c.right;
        if (count) {
          const range = document.createRange();
          range.selectNodeContents(count);
          textRight = range.getBoundingClientRect().right;
        }
        return {
          text: count ? count.textContent : "",
          graphRight: r.right,
          textRight,
          overflowPixels: Math.max(0, textRight - r.right),
          headerWidth: h.clientWidth,
          headerScrollWidth: h.scrollWidth,
        };
      });

      checks.push(check(`${prefix}-S3-header-no-clipping-${kind}`, "behavioral", "pass",
        clipping.overflowPixels <= 0,
        { ...clipping },
      ));
    }

    // Empty fixture handling
    await applyFixtureWithAck(page, scopeSel, "empty", logAction);
    await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
    const gEmpty = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S3-empty-fixture-handled`, "behavioral", "pass",
      Number.isFinite(gEmpty.ctmScale) && gEmpty.ctmScale > 0,
      { scale: gEmpty.ctmScale },
    ));
    await applyFixtureWithAck(page, scopeSel, "A", logAction);
    await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);

    // ------------------------------------------------------------------------
    // Scenario S4: Gesture Lifecycle
    // ------------------------------------------------------------------------
    // Outside release
    await mouseMoveWithAck(page, scopeSel, startX, startY, logAction);
    await mouseDownWithAck(page, scopeSel, logAction);
    await mouseMoveWithAck(page, scopeSel, 4, 4, logAction, { steps: 5 });
    await mouseUpWithAck(page, scopeSel, logAction);
    const gAtRelease = await captureGeometry(page, host);
    await mouseMoveWithAck(page, scopeSel, startX, startY, logAction);
    const gAfterOut = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S4-outside-release-preserves-camera`, "behavioral", "pass",
      same(gAtRelease.camera, gAfterOut.camera),
      { atRelease: gAtRelease.camera, after: gAfterOut.camera },
    ));

    // Middle and Right click ignored
    const gBeforeMid = await captureGeometry(page, host);
    await mouseMoveWithAck(page, scopeSel, startX, startY, logAction);
    await mouseDownWithAck(page, scopeSel, logAction, {button:"middle"});
    await mouseMoveWithAck(page, scopeSel, startX + 100, startY + 100, logAction);
    await mouseUpWithAck(page, scopeSel, logAction, {button:"middle"});
    const gAfterMid = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S4-middle-click-ignored`, "behavioral", "pass",
      same(gBeforeMid.camera, gAfterMid.camera),
      { before: gBeforeMid.camera, after: gAfterMid.camera },
    ));

    const gBeforeRight = await captureGeometry(page, host);
    await mouseMoveWithAck(page, scopeSel, startX, startY, logAction);
    await mouseDownWithAck(page, scopeSel, logAction, {button:"right"});
    await mouseMoveWithAck(page, scopeSel, startX + 100, startY + 100, logAction);
    await mouseUpWithAck(page, scopeSel, logAction, {button:"right"});
    const gAfterRight = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S4-right-click-ignored`, "behavioral", "pass",
      same(gBeforeRight.camera, gAfterRight.camera),
      { before: gBeforeRight.camera, after: gAfterRight.camera },
    ));

    // Active interruptions with IMMEDIATE capture-release BEFORE mouseup
    const interruptions = ["blur", "hidden", "lostcapture", "cancel", "run-switch"];
    for (const inter of interruptions) {
      await mouseMoveWithAck(page, scopeSel, startX, startY, logAction);
      await page.evaluate((scopeSel) => {
        const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
        v.addEventListener("pointerdown", (e) => {
          window.__testPid = e.pointerId;
        }, { once: true });
      }, scopeSel);
      await mouseDownWithAck(page, scopeSel, logAction);
      await mouseMoveWithAck(page, scopeSel, startX + 10, startY + 10, logAction);

      const heldBefore = await page.evaluate((scopeSel) => {
        const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
        return v.hasPointerCapture(window.__testPid);
      }, scopeSel);

      logAction({ action: "interruption.trigger", delivery: inter==='lostcapture'?'native capture API':'synthetic lifecycle/store', type: inter });
      if (inter === "blur") {
        await page.evaluate(() => window.dispatchEvent(new Event("blur")));
      } else if (inter === "hidden") {
        await page.evaluate(() => {
          Object.defineProperty(document, "hidden", { configurable: true, value: true });
          document.dispatchEvent(new Event("visibilitychange"));
          delete document.hidden;
        });
      } else if (inter === "lostcapture") {
        await page.evaluate((scopeSel) => {
          const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
          window.__lossPromise=new Promise((resolve,reject)=>{
            const t=setTimeout(()=>reject(Error('lostpointercapture delivery timeout')),5000);
            v.addEventListener('lostpointercapture',e=>{if(e.pointerId===window.__testPid){clearTimeout(t);resolve();}},{once:true});
          });
          v.releasePointerCapture(window.__testPid);
        }, scopeSel);
        await mouseMoveWithAck(page,scopeSel,startX+11,startY+11,logAction);
        await page.evaluate(()=>window.__lossPromise);
      } else if (inter === "cancel") {
        await page.evaluate((scopeSel) => {
          const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
          v.dispatchEvent(new PointerEvent("pointercancel", { bubbles: true, pointerId: window.__testPid, pointerType: "mouse" }));
        }, scopeSel);
      } else if (inter === "run-switch") {
        const selector=host==='modal'?`${scopeSel} [data-testid="dag-pane-modal-tab-qa-dag-b"]`:'[data-testid="qa-load-tall"]';
        await actionReceipt(page,scopeSel,{action:'run-switch-during-capture',delivery:'synthetic DOM click',selector},[{type:'click',selector}],()=>page.locator(selector).evaluate(e=>e.click()),logAction);
      }

      await page.evaluate(async () => {
        const { flushSync } = (await import("/node_modules/.vite/deps/react-dom.js")).default;
        flushSync(() => {});
      });

      // Check capture release IMMEDIATELY BEFORE mouseup
      const heldAfter = await page.evaluate((scopeSel) => {
        const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
        return v ? v.hasPointerCapture(window.__testPid) : false;
      }, scopeSel);

      const gInterBefore = await captureGeometry(page, host);
      await mouseMoveWithAck(page, scopeSel, startX + 20, startY + 20, logAction);
      await page.evaluate(async () => {
        const { flushSync } = (await import("/node_modules/.vite/deps/react-dom.js")).default;
        flushSync(() => {});
      });
      const gInterCont = await captureGeometry(page, host);
      await mouseUpWithAck(page, scopeSel, logAction);
      await mouseMoveWithAck(page, scopeSel, startX + 30, startY + 30, logAction);
      await page.evaluate(async () => {
        const { flushSync } = (await import("/node_modules/.vite/deps/react-dom.js")).default;
        flushSync(() => {});
      });
      const gInterAfter = await captureGeometry(page, host);

      checks.push(check(`${prefix}-S4-interruption-${inter}-capture-released-before-mouseup`, "behavioral", "pass",
        heldBefore === true && heldAfter === false,
        { heldBefore, heldAfter },
      ));
      checks.push(check(`${prefix}-S4-interruption-${inter}-pressed-move-no-drift`, "behavioral", "pass",
        same(gInterBefore.camera, gInterCont.camera) && same(gInterCont.camera, gInterAfter.camera),
        { before: gInterBefore.camera, cont: gInterCont.camera, after: gInterAfter.camera },
      ));

      if (inter === "run-switch") {
        if (host === "modal") {
          const tabA = page.locator(`${scopeSel} [data-testid="dag-pane-modal-tab-qa-dag-a"]`);
          if (await tabA.count() > 0) await clickSelectorWithAck(page,scopeSel,`${scopeSel} [data-testid="dag-pane-modal-tab-qa-dag-a"]`,logAction);
        } else {
          const btnA = page.locator('[data-testid="qa-load-a"]');
          if (await btnA.count() > 0) await clickSelectorWithAck(page,scopeSel,'[data-testid="qa-load-a"]',logAction);
        }
        await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
      }
    }

    // Mixed ownership: the accepted mouse remains the primary owner; another
    // mouse id cannot steal it. Synthetic identity injection is explicitly not
    // browser capture proof; continuation is trusted input from the real owner.
    await mouseMoveWithAck(page,scopeSel,startX,startY,logAction);
    await mouseDownWithAck(page,scopeSel,logAction);
    const mixedBefore=await captureGeometry(page,host);
    await actionReceipt(page,scopeSel,{action:'mixed-second-mouse',delivery:'synthetic DOM',pointerId:99},[{type:'pointermove',pointerId:99,clientX:startX+70,clientY:startY+70}],()=>page.evaluate(({scopeSel,startX,startY})=>{
      const v=document.querySelector(scopeSel+' [data-testid="dag-viewport"]');
      v.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,pointerType:'mouse',pointerId:99,button:0,buttons:1,clientX:startX,clientY:startY}));
      v.dispatchEvent(new PointerEvent('pointermove',{bubbles:true,pointerType:'mouse',pointerId:99,buttons:1,clientX:startX+70,clientY:startY+70}));
    },{scopeSel,startX,startY}),logAction);
    const mixedIgnored=await captureGeometry(page,host);
    await mouseMoveWithAck(page,scopeSel,startX+20,startY+10,logAction);
    const mixedOwner=await captureGeometry(page,host);
    await mouseUpWithAck(page,scopeSel,logAction);
    checks.push(check(`${prefix}-S4-mixed-mouse-ownership`,'behavioral','pass',same(mixedBefore.camera,mixedIgnored.camera)&&movedBy(delta(mixedIgnored.camera,mixedOwner.camera),20,10),{before:mixedBefore.camera,ignored:mixedIgnored.camera,owner:mixedOwner.camera}));

    // Contenteditable variants reject pan
    for (const attr of ["", "plaintext-only", "false", "true"]) {
      const gCE0 = await captureGeometry(page, host);
    await armReceipt(page,scopeSel);
      await page.evaluate(async ({ scopeSel, attr }) => {
        const { flushSync } = (await import("/node_modules/.vite/deps/react-dom.js")).default;
        const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
        const el = document.createElement("div");
        el.setAttribute("contenteditable", attr);
        v.append(el);
        flushSync(() => {
          el.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, pointerId: 88, pointerType: "mouse", button: 0, buttons: 1, clientX: 20, clientY: 20 }));
          v.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, pointerId: 88, pointerType: "mouse", buttons: 1, clientX: 50, clientY: 50 }));
          v.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, pointerId: 88, pointerType: "mouse" }));
        });
        el.remove();
      }, { scopeSel, attr });
    await page.evaluate(()=>window.__requestArmedReceipt());
    await finishReceipt(page,logAction);
      const gCE1 = await captureGeometry(page, host);
      checks.push(check(`${prefix}-S4-contenteditable-${attr || "empty"}-rejected`, "behavioral", "pass",
        same(gCE0.camera, gCE1.camera),
        { before: gCE0.camera, after: gCE1.camera },
      ));
    }

    // Positive real click proves both hit target and listener before drag.
    const clickStart=await visibleCard(page,host,logAction);
    await page.evaluate(scopeSel=>{
      window.__viewportClicks=0;
      document.querySelector(scopeSel+' [data-testid="dag-viewport"]').addEventListener('click',()=>window.__viewportClicks++);
    },scopeSel);
    await page.evaluate((scopeSel) => {
      const c = document.querySelector(scopeSel + ' [data-testid="dag-node-a"]');
      if (c) {
        c.addEventListener("click", () => {
          window.__cardClicked = true;
        });
      }
    }, scopeSel);
    await mouseMoveWithAck(page,scopeSel,clickStart.x,clickStart.y,logAction);
    await mouseDownWithAck(page,scopeSel,logAction);
    await mouseUpWithAck(page,scopeSel,logAction);
    const positiveClick=await page.evaluate(()=>window.__viewportClicks===1);
    await actionReceipt(page,scopeSel,{action:'card-listener-positive-control',delivery:'synthetic DOM click'},[{type:'click',selector:scopeSel+' [data-testid="dag-node-a"]'}],()=>page.locator(scopeSel+' [data-testid="dag-node-a"]').evaluate(e=>e.click()),logAction);
    const cardListenerLive=await page.evaluate(()=>window.__cardClicked===true);
    checks.push(check(`${prefix}-S4-positive-card-click`,'behavioral','pass',positiveClick&&cardListenerLive,{positiveClick,cardListenerLive}));
    await page.evaluate(()=>{window.__cardClicked=false;window.__viewportClicks=0;});
    const cRect = (await captureGeometry(page, host)).card;
    await mouseMoveWithAck(page, scopeSel, clickStart.x, clickStart.y, logAction);
    await mouseDownWithAck(page, scopeSel, logAction);
    await mouseMoveWithAck(page, scopeSel, clickStart.x + 50, clickStart.y + 50, logAction);
    await mouseUpWithAck(page, scopeSel, logAction);
    const wasCardClicked = await page.evaluate(() => (window.__cardClicked ?? false) || window.__viewportClicks > 0);
    checks.push(check(`${prefix}-S4-drag-click-suppression`, "behavioral", "pass", wasCardClicked === false, { wasCardClicked }));

    // ------------------------------------------------------------------------
    // Scenario S5: Touch / Pinch (CDP)
    // ------------------------------------------------------------------------
    await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
    const gPinch0 = await captureGeometry(page, host);
    const pVp = gPinch0.viewport;

    // Determine literal points or narrow host adaptation
    let p1Start, p2Start, p1Moved, p2Moved, expectedFactor = 1.6;
    let adapted = false;
    if (pVp.width >= 250) {
      p1Start = { id: 1, x: Math.round(pVp.x + 100), y: Math.round(pVp.y + 100) };
      p2Start = { id: 2, x: Math.round(pVp.x + 200), y: Math.round(pVp.y + 100) };
      p1Moved = { id: 1, x: Math.round(pVp.x + 80), y: Math.round(pVp.y + 120) };
      p2Moved = { id: 2, x: Math.round(pVp.x + 240), y: Math.round(pVp.y + 120) };
    } else {
      adapted = true;
      p1Start = { id: 1, x: Math.round(pVp.x + 20), y: Math.round(pVp.y + 60) };
      p2Start = { id: 2, x: Math.round(pVp.x + 100), y: Math.round(pVp.y + 60) };
      p1Moved = { id: 1, x: Math.round(pVp.x + 12), y: Math.round(pVp.y + 80) };
      p2Moved = { id: 2, x: Math.round(pVp.x + 140), y: Math.round(pVp.y + 80) };
    }

    const startDist = Math.hypot(p2Start.x - p1Start.x, p2Start.y - p1Start.y);
    const movedDist = Math.hypot(p2Moved.x - p1Moved.x, p2Moved.y - p1Moved.y);
    const exactRatio = movedDist / startDist;
    const startMid = { x: (p1Start.x + p2Start.x) / 2, y: (p1Start.y + p2Start.y) / 2 };
    const movedMid = { x: (p1Moved.x + p2Moved.x) / 2, y: (p1Moved.y + p2Moved.y) / 2 };

    // 1 -> 2 contact transition: no jump
    await cdpTouchWithAck(cdp, page, scopeSel, "touchStart", [p1Start], logAction);
    const gTouch1 = await captureGeometry(page, host);
    await cdpTouchWithAck(cdp, page, scopeSel, "touchStart", [p1Start, p2Start], logAction);
    const gTouch2 = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S5-touch-1-to-2-contact-no-jump`, "behavioral", "pass",
      same(gTouch1.camera, gTouch2.camera),
      { one: gTouch1.camera, two: gTouch2.camera },
    ));

    // Pinch move: ratio and midpoint anchor
    await cdpTouchWithAck(cdp, page, scopeSel, "touchMove", [p1Moved, p2Moved], logAction);
    const gPinch1 = await captureGeometry(page, host);

    const worldMidX = (startMid.x - pVp.x - gPinch0.camera.x) / gPinch0.camera.scale;
    const worldMidY = (startMid.y - pVp.y - gPinch0.camera.y) / gPinch0.camera.scale;
    const projMidX = pVp.x + gPinch1.camera.x + worldMidX * gPinch1.camera.scale;
    const projMidY = pVp.y + gPinch1.camera.y + worldMidY * gPinch1.camera.scale;
    const pinchRatio = gPinch1.ctmScale / gPinch0.ctmScale;

    checks.push(check(`${prefix}-S5-pinch-ratio-1.6`, "behavioral", "pass",
      Math.abs(pinchRatio - exactRatio) <= 0.015,
      { ratio: pinchRatio, exactRatio, adapted },
    ));
    checks.push(check(`${prefix}-S5-pinch-midpoint-anchor-stable`, "behavioral", "pass",
      Math.abs(projMidX - movedMid.x) <= 1.0 && Math.abs(projMidY - movedMid.y) <= 1.0,
      { errX: Math.abs(projMidX - movedMid.x), errY: Math.abs(projMidY - movedMid.y), adapted },
    ));

    // Extra contact (contact 3) full camera unchanged
    const p3Extra = { id: 3, x: Math.round(pVp.x + 50), y: Math.round(pVp.y + 50) };
    await cdpTouchWithAck(cdp, page, scopeSel, "touchStart", [p1Moved, p2Moved, p3Extra], logAction);
    const gExtra = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S5-pinch-extra-contact-fullcamera-unchanged`, "behavioral", "pass",
      same(gPinch1.camera, gExtra.camera),
      { before: gPinch1.camera, after: gExtra.camera },
    ));

    const p3Moved={...p3Extra,x:p3Extra.x+10,y:p3Extra.y+10};
    await cdpTouchWithAck(cdp,page,scopeSel,'touchMove',[p1Moved,p2Moved,p3Moved],logAction);
    const gThirdMoved=await captureGeometry(page,host);
    await cdpTouchWithAck(cdp,page,scopeSel,'touchEnd',[p3Moved],logAction);
    const gThirdLift=await captureGeometry(page,host);
    checks.push(check(`${prefix}-S5-third-move-independent-lift`,'behavioral','pass',same(gExtra.camera,gThirdMoved.camera)&&same(gThirdMoved.camera,gThirdLift.camera),{before:gExtra.camera,moved:gThirdMoved.camera,lift:gThirdLift.camera}));

    // Survivor 1: lift contact 3 & contact 2, move survivor contact 1
    await cdpTouchWithAck(cdp, page, scopeSel, "touchEnd", [p2Moved], logAction);
    const gSurv1Before = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S5-lift-to-survivor-1-no-jump`,'behavioral','pass',same(gThirdLift.camera,gSurv1Before.camera),{before:gThirdLift.camera,after:gSurv1Before.camera}));
    const p1SurvMove = { id: 1, x: p1Moved.x + 20, y: p1Moved.y + 10 };
    await cdpTouchWithAck(cdp, page, scopeSel, "touchMove", [p1SurvMove], logAction);
    const gSurv1After = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S5-pinch-survivor-1-translation-move`, "behavioral", "pass",
      movedBy(delta(gSurv1Before.camera, gSurv1After.camera), 20, 10, 1) &&
      Math.abs(gSurv1After.camera.scale - gSurv1Before.camera.scale) <= 1e-4,
      { delta: delta(gSurv1Before.camera, gSurv1After.camera) },
    ));
    await cdpTouchWithAck(cdp, page, scopeSel, "touchEnd", [p1SurvMove], logAction);

    // Survivor 2: lift contact 1, move survivor contact 2
    await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
    await cdpTouchWithAck(cdp, page, scopeSel, "touchStart", [p1Start, p2Start], logAction);
    const gBeforeLift2=await captureGeometry(page,host);
    await cdpTouchWithAck(cdp, page, scopeSel, "touchEnd", [p1Start], logAction);
    const gSurv2Before = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S5-lift-to-survivor-2-no-jump`,'behavioral','pass',same(gBeforeLift2.camera,gSurv2Before.camera),{before:gBeforeLift2.camera,after:gSurv2Before.camera}));
    const p2SurvMove = { id: 2, x: p2Start.x + 20, y: p2Start.y + 10 };
    await cdpTouchWithAck(cdp, page, scopeSel, "touchMove", [p2SurvMove], logAction);
    const gSurv2After = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S5-pinch-survivor-2-translation-move`, "behavioral", "pass",
      movedBy(delta(gSurv2Before.camera, gSurv2After.camera), 20, 10, 1) &&
      Math.abs(gSurv2After.camera.scale - gSurv2Before.camera.scale) <= 1e-4,
      { delta: delta(gSurv2Before.camera, gSurv2After.camera) },
    ));
    await cdpTouchWithAck(cdp, page, scopeSel, "touchEnd", [p2SurvMove], logAction);

    // Degenerate distance (<1px) deferred baseline: full camera unchanged
    await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
    const gDegBefore = await captureGeometry(page, host);
    const degX = Math.round(pVp.x + 30);
    const degY = Math.round(pVp.y + 60);
    await armReceipt(page,scopeSel);
    await page.evaluate(async ({ scopeSel, degX, degY }) => {
      const { flushSync } = (await import("/node_modules/.vite/deps/react-dom.js")).default;
      const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
      flushSync(() => {
        v.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, pointerId: 80, pointerType: "touch", clientX: degX, clientY: degY }));
        v.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, pointerId: 81, pointerType: "touch", clientX: degX + 0.5, clientY: degY }));
        v.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, pointerId: 81, pointerType: "touch", clientX: degX + 0.7, clientY: degY }));
      });
    }, { scopeSel, degX, degY });
    await page.evaluate(()=>window.__requestArmedReceipt());
    await finishReceipt(page,logAction);
    const gDegSubpixel = await captureGeometry(page, host);
    await armReceipt(page,scopeSel);
    await page.evaluate(async ({ scopeSel, degX, degY }) => {
      const { flushSync } = (await import("/node_modules/.vite/deps/react-dom.js")).default;
      const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
      flushSync(() => {
        v.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, pointerId: 81, pointerType: "touch", clientX: degX + 10, clientY: degY }));
      });
    }, { scopeSel, degX, degY });
    await page.evaluate(()=>window.__requestArmedReceipt());
    await finishReceipt(page,logAction);
    const gDegBaseline = await captureGeometry(page, host);
    await armReceipt(page,scopeSel);
    await page.evaluate(async ({ scopeSel, degX, degY }) => {
      const { flushSync } = (await import("/node_modules/.vite/deps/react-dom.js")).default;
      const v = document.querySelector(scopeSel + ' [data-testid="dag-viewport"]');
      flushSync(() => {
        v.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, pointerId: 81, pointerType: "touch", clientX: degX + 20, clientY: degY }));
        v.dispatchEvent(new PointerEvent("pointercancel", { bubbles: true, pointerId: 80, pointerType: "touch" }));
        v.dispatchEvent(new PointerEvent("pointercancel", { bubbles: true, pointerId: 81, pointerType: "touch" }));
      });
    }, { scopeSel, degX, degY });
    await page.evaluate(()=>window.__requestArmedReceipt());
    await finishReceipt(page,logAction);
    const gDegAfter = await captureGeometry(page, host);

    checks.push(check(`${prefix}-S5-pinch-degenerate-distance-fullcamera-unchanged`, "behavioral", "pass",
      same(gDegBefore.camera, gDegSubpixel.camera) &&
      same(gDegSubpixel.camera, gDegBaseline.camera) &&
      close(gDegAfter.camera.scale / gDegBaseline.camera.scale, 2.0, 0.01) &&
      close(gDegAfter.camera.x, (degX+10-pVp.x)-2*((degX+5-pVp.x)-gDegBaseline.camera.x),1) &&
      close(gDegAfter.camera.y, (degY-pVp.y)-2*((degY-pVp.y)-gDegBaseline.camera.y),1),
      { before: gDegBefore.camera, subpixel: gDegSubpixel.camera, baseline: gDegBaseline.camera, after: gDegAfter.camera },
    ));

    await resizeViewportWithAck(page, scopeSel, Math.round(pVp.width), 300, logAction);
    // Changed-min outward no-jump then inward recovery (B2)
    await applyFixtureWithAck(page, scopeSel, "big", logAction);
    await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
    const gBigFitted = await captureGeometry(page, host);
    await applyFixtureWithAck(page, scopeSel, "A", logAction);
    await page.evaluate(async () => {
      const { flushSync } = (await import("/node_modules/.vite/deps/react-dom.js")).default;
      flushSync(() => {});
    });

    const gAAfterBig = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S5-changed-min-camera-preserved-on-fixture-shrink`, "behavioral", "pass",
      Math.abs(gBigFitted.camera.scale - gAAfterBig.camera.scale) <= 1e-4 &&
      Math.abs(gBigFitted.camera.x - gAAfterBig.camera.x) <= 1.0,
      { before: gBigFitted.camera, after: gAAfterBig.camera },
    ));

    const cmX = Math.round(pVp.x + 30);
    const cmY = Math.round(pVp.y + 60);
    const cmT1 = { id: 21, x: cmX, y: cmY };
    const cmT2 = { id: 22, x: cmX + 100, y: cmY };
    await cdpTouchWithAck(cdp, page, scopeSel, "touchStart", [cmT1, cmT2], logAction);
    // Outward move: dist 100 -> 90
    const cmT1Out = { id: 21, x: cmX + 5, y: cmY };
    const cmT2Out = { id: 22, x: cmX + 95, y: cmY };
    await cdpTouchWithAck(cdp, page, scopeSel, "touchMove", [cmT1Out, cmT2Out], logAction);
    const gOutward = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S5-changed-min-outward-fullcamera-no-jump`, "behavioral", "pass",
      same(gAAfterBig.camera, gOutward.camera),
      { before: gAAfterBig.camera, after: gOutward.camera },
    ));

    // Inward recovery: dist -> 110
    const cmT1In = { id: 21, x: cmX - 5, y: cmY };
    const cmT2In = { id: 22, x: cmX + 105, y: cmY };
    await cdpTouchWithAck(cdp, page, scopeSel, "touchMove", [cmT1In, cmT2In], logAction);
    const gInward = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S5-changed-min-inward-recovery`, "behavioral", "pass",
      close(gInward.camera.scale, gAAfterBig.camera.scale * 1.1, 0.0001) &&
      close(gInward.camera.x,(cmX+50-pVp.x)-1.1*((cmX+50-pVp.x)-gAAfterBig.camera.x),1) &&
      close(gInward.camera.y,(cmY-pVp.y)-1.1*((cmY-pVp.y)-gAAfterBig.camera.y),1),
      { before: gAAfterBig.camera.scale, after: gInward.camera.scale, expected: gAAfterBig.camera.scale * 1.1 },
    ));
    await cdpTouchWithAck(cdp, page, scopeSel, "touchEnd", [cmT1In, cmT2In], logAction);

    // ------------------------------------------------------------------------
    // Scenario S6: Live State & Host Independence
    // ------------------------------------------------------------------------
    // Custom status update fullcamera preserved
    await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
    await dispatchSyntheticWheelWithAck(page,scopeSel,{deltaY:-120},logAction);
    await mouseMoveWithAck(page,scopeSel,startX,startY,logAction);
    await mouseDownWithAck(page,scopeSel,logAction);
    await mouseMoveWithAck(page,scopeSel,startX+25,startY+15,logAction);
    await mouseUpWithAck(page,scopeSel,logAction);
    const gLive0 = await captureGeometry(page, host);
    await applyFixtureWithAck(page, scopeSel, "updated", logAction);
    const gLive1 = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S6-custom-status-update-fullcamera-preserved`, "behavioral", "pass",
      same(gLive0.camera, gLive1.camera),
      { before: gLive0.camera, after: gLive1.camera },
    ));

    // Topology shrink fullcamera preserved
    await applyFixtureWithAck(page, scopeSel, "big", logAction);
    await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
    const gShrink0 = await captureGeometry(page, host);
    await applyFixtureWithAck(page, scopeSel, "A", logAction);
    const gShrink1 = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S6-topology-shrink-fullcamera-preserved`, "behavioral", "pass",
      same(gShrink0.camera, gShrink1.camera),
      { before: gShrink0.camera, after: gShrink1.camera },
    ));

    // Empty -> first nonempty: fits without user Fit
    await applyFixtureWithAck(page, scopeSel, "empty", logAction);
    await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
    await applyFixtureWithAck(page, scopeSel, "big", logAction);

    const gFirstNonEmpty = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S6-empty-to-first-nonempty-auto-fits`, "behavioral", "pass",
      fitOk(gFirstNonEmpty),
      { gFirstNonEmpty },
    ));

    // Zero-size deferral -> positive: fits without user Fit
    await resizeViewportWithAck(page, scopeSel, 0, 0, logAction);
    await applyFixtureWithAck(page, scopeSel, "empty", logAction);
    await applyFixtureWithAck(page, scopeSel, "big", logAction);
    await resizeViewportWithAck(page, scopeSel, 200, 200, logAction);
    const gZeroPositive = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S6-zero-size-deferral-auto-fits`, "behavioral", "pass",
      fitOk(gZeroPositive),
      { gZeroPositive },
    ));

    // Zero-size new-run deferral -> positive: fits without user Fit
    await resizeViewportWithAck(page, scopeSel, 0, 0, logAction);
    await applyFixtureWithAck(page, scopeSel, "tall", logAction, "qa-dag-tall");
    await clickSelectorWithAck(page,scopeSel,host==='modal'?`${scopeSel} [data-testid="dag-pane-modal-tab-qa-dag-tall"]`:'[data-testid="qa-load-tall"]',logAction);
    await resizeViewportWithAck(page, scopeSel, 200, 200, logAction);
    const gZeroNew = await captureGeometry(page, host);
    checks.push(check(`${prefix}-S6-zero-size-new-run-auto-fits`, "behavioral", "pass",
      gZeroNew.runId === "qa-dag-tall" && gZeroNew.contentWidth === 284 && gZeroNew.cardIds.length === 100 && gZeroNew.contentHeight > 7000 && fitOk(gZeroNew),
      { gZeroNew },
    ));
    if (host === "standalone") {
      // Retain the measured 200x200 viewport: status wrapping cannot confound
      // the new-run Fit-once oracle with a legitimate center-preserving resize.
      const x = Math.round(gZeroNew.viewport.x + 60);
      const y = Math.round(gZeroNew.viewport.y + 60);
      await mouseMoveWithAck(page, scopeSel, x, y, logAction);
      await mouseWheelWithAck(page, scopeSel, 0, -120, logAction);
      await mouseDownWithAck(page, scopeSel, logAction);
      await mouseMoveWithAck(page, scopeSel, x + 25, y + 15, logAction);
      await mouseUpWithAck(page, scopeSel, logAction);
      const custom = await captureGeometry(page, host);
      checks.push(check(`${prefix}-S6-standalone-new-run-custom-camera`, "behavioral", "pass",
        custom.runId === "qa-dag-tall" && fitOk(gZeroNew) &&
        custom.camera.scale > gZeroNew.camera.scale && !fitOk(custom),
        { initial: gZeroNew, custom },
      ));
      for (let update = 0; update < 2; update++) {
        let published;
        await actionReceipt(page, scopeSel,
          { action: "standalone.new-run.status", delivery: "synthetic store", runId: "qa-dag-tall", update },
          [], async () => {
            published = await page.evaluate(async (update) => {
              const { dagStore } = await import("/src/state/dagStore.ts");
              const f = await import("/src/devtools/dagViewportQaFixtures.ts");
              const { deriveDagRunCounts } = await import("/src/lib/dagTypes.ts");
              const before = dagStore.getState().runsByProject[f.QA_PROJECT_PATH]["qa-dag-tall"];
              const nodeId = `t${update}`;
              const nodes = before.nodes.map(n => n.id === nodeId ? { ...n, state: "completed" } : n);
              dagStore.applySnapshot(f.QA_PROJECT_PATH, {
                ...before, nodes, counts: deriveDagRunCounts(nodes),
                status: "running", updatedAt: new Date().toISOString(),
              });
              const after = dagStore.getState().runsByProject[f.QA_PROJECT_PATH]["qa-dag-tall"];
              return { runId: after.runId, status: after.status, nodeId,
                beforeState: before.nodes.find(n => n.id === nodeId).state,
                afterState: after.nodes.find(n => n.id === nodeId).state,
                completed: after.counts.completed };
            }, update);
          }, logAction);
        const after = await captureGeometry(page, host);
        logAction({ action: "standalone.new-run.status-observed", update, published, after });
        checks.push(check(`${prefix}-S6-standalone-new-run-fit-once-status-${update}`, "behavioral", "pass",
          published.runId === "qa-dag-tall" && published.status === "running" &&
          published.beforeState !== published.afterState && published.afterState === "completed" &&
          published.completed === update + 1 && after.runId === "qa-dag-tall" &&
          after.contentWidth === 284 && after.contentHeight === 7074 &&
          JSON.stringify(after.cardIds) === JSON.stringify(gZeroNew.cardIds) &&
          after.viewport.width === custom.viewport.width && after.viewport.height === custom.viewport.height &&
          same(custom.camera, after.camera),
          { published, initial: gZeroNew, custom, after },
        ));
      }
    }
    await restoreViewportWithAck(page, scopeSel, logAction);

    // Viewport resize: half-delta center world point stability
    await applyFixtureWithAck(page, scopeSel, "A", logAction);
    if(host === "standalone") await clickSelectorWithAck(page,scopeSel,'[data-testid="qa-load-a"]',logAction);
    if (host === "modal") {
      const tabA = page.locator(`${scopeSel} [data-testid="dag-pane-modal-tab-qa-dag-a"]`);
      if (await tabA.count() > 0) await clickSelectorWithAck(page,scopeSel,`${scopeSel} [data-testid="dag-pane-modal-tab-qa-dag-a"]`,logAction);
    }
    await clickButtonWithAck(page, scopeSel, "Fit graph", logAction);
    const gBeforeResize = await captureGeometry(page, host);
    const targetW = Math.round(gBeforeResize.viewport.width - 40);
    const targetH = Math.round(gBeforeResize.viewport.height - 30);
    await measuredAction(page,scopeSel,{action:'page.setViewportSize',delivery:'CDP viewport',width:1100,height:800},()=>page.setViewportSize({width:1100,height:800}),logAction);
    const gAfterResize = await captureGeometry(page, host);
    const deltaW = gAfterResize.viewport.width - gBeforeResize.viewport.width;
    const deltaH = gAfterResize.viewport.height - gBeforeResize.viewport.height;

    checks.push(check(`${prefix}-S6-viewport-resize-center-half-delta`, "behavioral", "pass",
      Math.abs(gAfterResize.ctmScale - gBeforeResize.ctmScale) <= 1e-4 &&
      Math.abs(gAfterResize.camera.x - (gBeforeResize.camera.x + deltaW / 2)) <= 1.0 &&
      Math.abs(gAfterResize.camera.y - (gBeforeResize.camera.y + deltaH / 2)) <= 1.0,
      { before: gBeforeResize.camera, after: gAfterResize.camera, deltaW, deltaH },
    ));
    await measuredAction(page,scopeSel,{action:'page.setViewportSize.restore',size},()=>page.setViewportSize(size),logAction);

    // Modal-specific: Tab switch, Focus trap Tab/Shift+Tab, Escape, Reopen twice
    if (host === "modal") {
      // Tab switch to run B fits new run once
      const tabB = page.locator(`${scopeSel} [data-testid="dag-pane-modal-tab-qa-dag-b"]`);
      if (await tabB.count() > 0) {
        await tabB.scrollIntoViewIfNeeded();
        await clickSelectorWithAck(page,scopeSel,`${scopeSel} [data-testid="dag-pane-modal-tab-qa-dag-b"]`,logAction);
        logAction({ action: "tab.click", run: "qa-dag-b" });
        await page.locator(`${scopeSel} [data-testid="dag-node-b-1"]`).waitFor({ state: "visible" });
        await page.evaluate(async () => {
          const { flushSync } = (await import("/node_modules/.vite/deps/react-dom.js")).default;
          flushSync(() => {});
        });
        const gTabB = await captureGeometry(page, host, "dag-node-b-1", "dag-edge-b-1-b-4");
        checks.push(check(`${prefix}-S6-modal-tab-switch-fits-run-once`, "behavioral", "pass",
          fitOk(gTabB) && allInside(gTabB),
          { gTabB },
        ));
        await dispatchSyntheticWheelWithAck(page,scopeSel,{deltaY:-120},logAction);
        const bCustom=await captureGeometry(page,host);
        for(let update=0;update<2;update++) {
          let published;
          await actionReceipt(page,scopeSel,{action:'B.status',delivery:'synthetic store',runId:'qa-dag-b',update},[],async()=>{
            published = await page.evaluate(async update=>{
              const {dagStore}=await import('/src/state/dagStore.ts');const f=await import('/src/devtools/dagViewportQaFixtures.ts');
              const before = dagStore.getState().runsByProject[f.QA_PROJECT_PATH]['qa-dag-b'];
              const nodeId = before.nodes[update].id;
              const {deriveDagRunCounts}=await import('/src/lib/dagTypes.ts');
              const nodes=before.nodes.map(n=>n.id===nodeId?{...n,state:'completed'}:n);
              dagStore.applySnapshot(f.QA_PROJECT_PATH,{
                ...before,nodes,counts:deriveDagRunCounts(nodes),
                updatedAt:new Date().toISOString(),status:'running',
              });
              const after = dagStore.getState().runsByProject[f.QA_PROJECT_PATH]['qa-dag-b'];
              return { runId: after.runId, status: after.status, nodeId,
                beforeState: before.nodes.find(n=>n.id===nodeId).state,
                afterState: after.nodes.find(n=>n.id===nodeId).state,
                beforeCompleted: before.counts.completed, completed: after.counts.completed,
                states: after.nodes.map(n=>({id:n.id,state:n.state})) };
            },update);
          },logAction);
          const bAfter=await captureGeometry(page,host);
          logAction({action:'B.status-observed',update,published,after:bAfter});
          checks.push(check(`${prefix}-S6-B-fit-once-status-${update}`,'behavioral','pass',
            published.runId==='qa-dag-b' && published.status==='running' &&
            published.beforeState!==published.afterState && published.afterState==='completed' &&
            published.beforeCompleted===update && published.completed===update+1 &&
            published.states.slice(0,update+1).every(n=>n.state==='completed') &&
            bAfter.runId==='qa-dag-b' && same(bCustom.camera,bAfter.camera) &&
            bAfter.viewport.width===bCustom.viewport.width && bAfter.viewport.height===bCustom.viewport.height &&
            bAfter.contentWidth===bCustom.contentWidth && bAfter.contentHeight===bCustom.contentHeight &&
            JSON.stringify(bAfter.cardIds)===JSON.stringify(bCustom.cardIds),
            {published,before:bCustom,after:bAfter}));
        }
        const tabA = page.locator(`${scopeSel} [data-testid="dag-pane-modal-tab-qa-dag-a"]`);
        if (await tabA.count() > 0) {
          await tabA.scrollIntoViewIfNeeded();
          await clickSelectorWithAck(page,scopeSel,`${scopeSel} [data-testid="dag-pane-modal-tab-qa-dag-a"]`,logAction);
          await page.locator(`${scopeSel} [data-testid="dag-node-a"]`).waitFor({ state: "visible" });
          await page.evaluate(async () => {
            const { flushSync } = (await import("/node_modules/.vite/deps/react-dom.js")).default;
            flushSync(() => {});
          });
        }
      }

      // Focus trap Tab & Shift+Tab
      const trapOk = await page.evaluate((scopeSel) => {
        const modal = document.querySelector(scopeSel);
        const focusables = [...modal.querySelectorAll('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])')]
          .filter((el) => !el.hasAttribute("disabled") && el.getAttribute("aria-disabled") !== "true");
        if (focusables.length < 2) return false;
        focusables[focusables.length - 1].focus();
        return true;
      }, scopeSel);
      await keyPressWithAck(page, "Tab", logAction);
      const tabWrappedForward = await page.evaluate((scopeSel) => {
        const modal = document.querySelector(scopeSel);
        const focusables = [...modal.querySelectorAll('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])')];
        return document.activeElement === focusables[0];
      }, scopeSel);
      await page.keyboard.down("Shift");
      await keyPressWithAck(page, "Tab", logAction);
      await page.keyboard.up("Shift");
      const tabWrappedBack = await page.evaluate((scopeSel) => {
        const modal = document.querySelector(scopeSel);
        const focusables = [...modal.querySelectorAll('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])')];
        return document.activeElement === focusables[focusables.length - 1];
      }, scopeSel);

      checks.push(check(`${prefix}-S6-modal-focus-trap-wraps-both-directions`, "behavioral", "pass",
        trapOk && tabWrappedForward && tabWrappedBack,
        { trapOk, tabWrappedForward, tabWrappedBack },
      ));

      // Escape closes modal, reopen twice with single wheel factor
      await clickButtonWithAck(page, scopeSel, "Reset zoom to 100%", logAction);
      await keyPressWithAck(page, "Escape", logAction);
      const modalClosed = await page.locator(scopeSel).count() === 0;
      checks.push(check(`${prefix}-S6-modal-escape-closes-modal`, "behavioral", "pass", modalClosed, { modalClosed }));

      // Reopen 1
      await clickSelectorWithAck(page,scopeSel,'[data-testid="qa-modal-host"] [data-testid="dag-pane-badge-button"]',logAction);
      logAction({ action: "badge.click", reopen: 1 });
      await page.locator(`${scopeSel} [data-testid="dag-node-a"]`).waitFor({ state: "visible" });

      const gReopen1Before = await captureGeometry(page, host);
      await dispatchSyntheticWheelWithAck(page, scopeSel, { deltaY: -120, deltaMode: 0 }, logAction);
      const gReopen1After = await captureGeometry(page, host);
      const reopen1Factor = gReopen1After.ctmScale / gReopen1Before.ctmScale;

      // Reopen 2
      await keyPressWithAck(page, "Escape", logAction);
      await clickSelectorWithAck(page,scopeSel,'[data-testid="qa-modal-host"] [data-testid="dag-pane-badge-button"]',logAction);
      logAction({ action: "badge.click", reopen: 2 });
      await page.locator(`${scopeSel} [data-testid="dag-node-a"]`).waitFor({ state: "visible" });

      const gReopen2Before = await captureGeometry(page, host);
      await dispatchSyntheticWheelWithAck(page, scopeSel, { deltaY: -120, deltaMode: 0 }, logAction);
      const gReopen2After = await captureGeometry(page, host);
      const reopen2Factor = gReopen2After.ctmScale / gReopen2Before.ctmScale;

      const expFactor = Math.exp(0.24);
      checks.push(check(`${prefix}-S6-modal-reopen-twice-single-wheel-factor`, "behavioral", "pass",
        Math.abs(reopen1Factor - expFactor) <= 0.01 && Math.abs(reopen2Factor - expFactor) <= 0.01,
        { before1: gReopen1Before.ctmScale, after1: gReopen1After.ctmScale, before2: gReopen2Before.ctmScale, after2: gReopen2After.ctmScale, reopen1Factor, reopen2Factor, expFactor },
      ));
    }

    // Standalone-specific: split seam drag
    if (host === "standalone") {
      const seamHandle = page.locator('[data-testid="qa-standalone-host"] [role="separator"]');
      if (await seamHandle.count() > 0) {
        const seamBox = await seamHandle.boundingBox();
        if (seamBox) {
          const gBeforeSeam = await captureGeometry(page, host);
          await mouseMoveWithAck(page, scopeSel, seamBox.x + seamBox.width / 2, seamBox.y + seamBox.height / 2, logAction);
          await mouseDownWithAck(page, scopeSel, logAction);
          await mouseMoveWithAck(page, scopeSel, seamBox.x + seamBox.width / 2 + 50, seamBox.y + seamBox.height / 2, logAction);
          await mouseUpWithAck(page, scopeSel, logAction);
          await page.evaluate(async () => {
            const { flushSync } = (await import("/node_modules/.vite/deps/react-dom.js")).default;
            flushSync(() => {});
          });
          const gAfterSeam = await captureGeometry(page, host);
          const beforeLeaves=gBeforeSeam.leaves,afterLeaves=gAfterSeam.leaves;
          const leafDiffers=beforeLeaves.length===2&&afterLeaves.length===2&&beforeLeaves.every((leaf,i)=>{
            const after=afterLeaves[i];const dx=i===0?0:50,dw=i===0?50:-50;
            return leaf.id===after.id&&close(after.rect.x-leaf.rect.x,dx,1)&&close(after.rect.width-leaf.rect.width,dw,1)&&close(after.rect.y,leaf.rect.y,.1)&&close(after.rect.height,leaf.rect.height,.1);
          });
          checks.push(check(`${prefix}-S6-standalone-seam-drag-updates-leaves`, "behavioral", "pass", leafDiffers, {
            beforeLeaves: gBeforeSeam.leaves,
            afterLeaves: gAfterSeam.leaves,
          }));
        }
      }
    }
  } finally {
    if(!page.isClosed()) for(const event of await page.evaluate(()=>window.__qaDelivered||[])) logAction({action:'delivered-event',...event});
    await context.close();
  }
}

async function runHostGreen(browser, host, evidencePaths, runnerActions) {
  const hostResults = {
    checks: [],
    actions: [],
    screenshots: evidencePaths,
    pageErrors: [],
    consoleErrors: [],
    environmentNoise: [],
  };

  const VIEWPORTS = [
    { width: 1280, height: 900 },
    { width: 390, height: 844 },
  ];

  try {
    for (const size of VIEWPORTS) {
      await runHostScenarioMatrix(browser, host, size, evidencePaths, runnerActions, hostResults);
    }
    return hostResults;
  } catch (err) {
    hostResults.scenarioError = String(err && err.stack ? err.stack : err);
    return hostResults;
  }
}

// --------------------------------------------------------------------------
// Main Execution
// --------------------------------------------------------------------------

let browser = null;
let browserSource = "unknown";
let browserPids = [];
const runnerActions = [];
const chromeBin = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

try {
  try {
    browser = await pw.chromium.launch({ channel: "chrome", headless: true });
    browserSource = "channel:chrome (real Google Chrome, headless)";
  } catch (channelError) {
    if (!existsSync(chromeBin)) {
      fail(1, `Google Chrome channel unavailable and ${chromeBin} missing: ${String(channelError)}`);
    }
    browser = await pw.chromium.launch({ headless: true, executablePath: chromeBin });
    browserSource = `executablePath:${chromeBin} (headless)`;
  }

  const browserCdp=await browser.newBrowserCDPSession();
  browserPids=(await browserCdp.send('SystemInfo.getProcessInfo')).processInfo.map(p=>p.id);
  await browserCdp.detach();
  const results = {
    browserPids,
    phase,
    hostArg,
    hosts: {},
    startedAt: new Date().toISOString(),
    playwrightSource,
    browserSource,
  };

  const evidencePaths = [];

  for (const host of hosts) {
    if (phase === "baseline") {
      results.hosts[host] = await runHostBaseline(browser, host, evidencePaths);
    } else {
      results.hosts[host] = await runHostGreen(browser, host, evidencePaths, runnerActions);
    }
  }

  results.finishedAt = new Date().toISOString();
  results.screenshots = evidencePaths;

  const summary = phase === "baseline" ? summarizeBaseline(results, hosts) : summarizeGreen(results, hosts);
  results.summary = summary;

  writeFileSync(join(evidenceDir, "results.json"), JSON.stringify(results, null, 2));
  writeFileSync(
    join(evidenceDir, "actions.json"),
    JSON.stringify(
      {
        generatedAt: new Date().toISOString(),
        byHost: Object.fromEntries(hosts.map((h) => [h, results.hosts[h]?.actions ?? []])),
        runner: runnerActions,
      },
      null,
      2,
    ),
  );

  const cleanupLines = [
    "# Browser and Server Cleanup Receipt",
    "",
    `- Finished at: ${results.finishedAt}`,
    `- Browser source: ${browserSource}`,
    `- Hosts tested: ${hosts.join(", ")}`,
    `- Evidence directory: ${evidenceDir}`,
  ];
  writeFileSync(join(evidenceDir, "cleanup.md"), cleanupLines.join("\n") + "\n");

  if (phase === "baseline") {
    console.log(`BASELINE-${summary.ok ? "PASS" : "FAIL"}`);
    for (const p of summary.passedChecks) console.log(`  passed: ${p}`);
    for (const f of summary.failedChecks) console.log(`  failed: ${f}`);
    for (const m of summary.missingSetup) console.log(`  missing-setup: ${m}`);
    for (const e of summary.errors) console.log(`  error: ${e}`);
    process.exitCode = summary.ok ? 0 : 1;
  } else {
    console.log(`GREEN-${summary.ok ? "PASS" : "FAIL"}`);
    for (const p of summary.passedChecks) console.log(`  passed: ${p}`);
    for (const f of summary.failedChecks) console.log(`  failed: ${f}`);
    for (const m of summary.missingSetup) console.log(`  missing-setup: ${m}`);
    for (const e of summary.errors) console.log(`  error: ${e}`);
    process.exitCode = summary.ok ? 0 : 1;
  }
} finally {
  if (browser) {
    await browser.close();
    const ps=spawnSync('ps',['-p',browserPids.join(','),'-o','pid,ppid,command'],{encoding:'utf8'});
    const cleanup={closedAt:new Date().toISOString(),awaitedBrowserClose:true,browserPids,ps:{status:ps.status,stdout:ps.stdout,stderr:ps.stderr},serverOwnedBy:'invoking monitor'};
    writeFileSync(join(evidenceDir,'browser-cleanup.json'),JSON.stringify(cleanup,null,2));
    writeFileSync(join(evidenceDir,'cleanup.md'),'# Browser cleanup\n\nAwaited browser.close() in finally before exitCode takes effect.\n\n```json\n'+JSON.stringify(cleanup,null,2)+'\n```\nServer PID and port cleanup belong to the invoking monitor.\n');
  }
}
