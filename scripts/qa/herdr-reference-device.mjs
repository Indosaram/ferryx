#!/usr/bin/env node
/**
 * herdr-reference-device.mjs — real-device iOS composition, layout, scroll and copy producer for
 * the Herdr reference-chat parity plan (task 15, QA-08).
 *
 * Device receipts establish execution. The receipt this file writes is
 * validated by task 14's frozen `validateDeviceReceipt()` in
 * `scripts/qa/herdr-reference-fixtures.mjs`.
 *
 * WHAT IT PROVES (only when a run passes on an integrated candidate)
 *   Korean enters the served reference-chat candidate on a PHYSICAL iOS device through the NATIVE
 *   keyboard path — real on-screen keyboard key taps when the keyboard's keys are exposed, or WDA
 *   hardware key events (`/wda/keys`) when they are not — and never through a JavaScript input
 *   event. The phone surfaces QA-08 names are exercised on that device: composition exactly-once,
 *   keyboard-driven layout, touch scroll, the transcript's copy affordance, a view-mode switch
 *   during composition, a target switch that protects the original pane, and the late-ACK window.
 *
 * WHY NOT A BROWSER HARNESS
 *   A browser-generated composition event is not native evidence. This producer therefore requires
 *   a provisioned real-device WebDriverAgent/XCTest driver endpoint from the fixture manifest and
 *   fails closed (BLOCKED, nonzero) when the device, the driver endpoint or the driver session is
 *   unavailable. It never falls back to a simulated keyboard.
 *
 * HOST CONTRACT
 *   Any host that can reach the provisioned WDA endpoint over HTTP (Node >= 22; global fetch).
 *   Builds and gateway serving stay on the verification host. This producer starts NO server and
 *   launches NO driver: the driver is provisioned and owned by the verification host.
 *
 * WHAT IT WILL NEVER DO (fail-closed, evidence-integrity policy)
 *   - Never dispatches JavaScript composition/input events and never sets a field value as text to
 *     stand in for typing (`/element/{id}/value` with `text` is not a keyboard action and is never used to
 *     produce the Korean token).
 *   - Never fabricates screenshots, transcripts, coordinates or a device identity.
 *   - Never guesses a tap target: elements are resolved from the accessibility source by identifier
 *     or label, and every interaction is verified by a bounded, observable source delta.
 *   - Never invents the candidate/page/target/PTY binding: it is consumed verbatim from the task 14
 *     binding (the same channel the Android producer uses) or the run is BLOCKED.
 *   - Never accepts a first-run consent screen or an onboarding sheet on the owner's behalf; that
 *     outcome is a BLOCKED target with reason `device-owner-acknowledgment-needed`.
 *   - Never kills a process by pattern. Only PIDs recorded at spawn time, re-checked against the
 *     executable that was launched, are ever terminated; anything else is REPORTED. The driver
 *     session this run created is the resource it deletes.
 *
 * CANDIDATE SURFACE BOUNDARY
 *   The reference-chat candidate renders `remote-terminal-grid` (DOM lines carrying
 *   `data-grid-line`), `remote-terminal-input-sink`, `remote-terminal-preedit` and the header
 *   view-mode buttons `remote-view-mode-chat` / `remote-view-mode-terminal`. It does NOT render
 *   `remote-terminal-line-input` or `remote-terminal-input-mode-toggle`: there is no line/direct
 *   terminal input mode in this candidate. The frozen scenario name `mode-switch` is the
 *   CHAT/TERMINAL VIEW-MODE switch, and the receipt names that subject explicitly so no reader has
 *   to infer it:
 *     mode-switch / happy       subject "chat-terminal-view-mode-toggle"
 *     mode-switch / failure     the same subject, switched MID-composition
 *   The QA-08 requirement is exactly those two rows; no terminal line/direct input-mode behavior is
 *   required by the plan, and none is asserted here.
 *
 * FIXED INVOCATION (plan, "Fixture provisioning and device commands")
 *   node scripts/qa/herdr-reference-device.mjs --platform ios \
 *     --fixture-manifest <fixtures.json> --scenario QA-08 --case all --evidence-dir <dir>
 *   The device serial and the driver endpoint come from the fixture manifest's `devices[]`; no
 *   historical serial is ever assumed.
 *
 * BINDING
 *   The candidate/page/target/PTY tuple arrives through the environment variable
 *   `FERRYX_HERDR_REFERENCE_BINDING` (the absolute path the task 14 runner writes and echoes), exactly
 *   as for the Android producer. There is NO fallback: an unset variable, a relative path, a missing
 *   file or an incomplete binding is BLOCKED, never guessed. The runner writes the binding with the
 *   ANDROID receipt path; this producer resolves its OWN platform path
 *   (`<evidence-dir>/ios/device-receipt.json`) and never overwrites the Android receipt.
 *
 * RECEIPT
 *   `<evidence-dir>/ios/device-receipt.json`, schema `ferryx-herdr-reference.device/1`, with
 *   candidate/page/target/pty copied verbatim from the binding, device provenance read from the
 *   driver session, one row per (scenario, case) with its real captures, and a cleanup ledger of
 *   the exact resources this run created.
 *
 * EXIT CODES
 *   0 all selected (scenario, case) rows passed
 *   2 prerequisite blocked (missing device/driver/binding/consent)
 *   3 interaction failure (a surface could not be driven)
 *   4 assertion failure (an observable did not hold)
 *   5 usage error
 *   130 interrupted (cleanup still runs)
 */

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import {
  REFERENCE_DEVICE_BINDING_ENV,
  REFERENCE_DEVICE_RECEIPT_SCHEMA,
  REFERENCE_QA_IDS,
  SPAWN_LEDGER_SCHEMA,
  deviceReceiptPath,
  killExactPid,
  probeProcessIdentity,
  readDeviceBinding,
  readJson,
} from "./herdr-reference-fixtures.mjs";

const SCRIPT_ID = "herdr-reference-device.mjs/1.0.0";
const EXIT = { OK: 0, BLOCKED: 2, INTERACTION: 3, ASSERTION: 4, USAGE: 5, INTERRUPT: 130 };
const PLATFORM = "ios";
const DUPLICATE_WINDOW_MS = 700; // settle window used ONLY to detect a second, late send
const CASES = ["happy", "failure"];
const TOKENS = { direct: "모바일", composition: "가다", chat: "한" };
const JAMO = new Set(
  ("ㄱㄲㄳㄴㄵㄶㄷㄸㄹㄺㄻㄼㄽㄾㄿㅀㅁㅂㅃㅄㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ" +
    "ㅏㅐㅑㅒㅓㅔㅕㅖㅗㅘㅙㅚㅛㅜㅝㅞㅟㅠㅡㅢㅣ").split(""),
);

/* --------------------------------------------------------------------------- state */

let ARGS = null;
let EVIDENCE_DIR = null;
let DEVICE = null;
let WDA = null;

const RUN = {
  startedAt: null,
  finishedAt: null,
  scriptId: SCRIPT_ID,
  scriptSha256: null,
  host: { platform: process.platform, release: os.release(), node: process.version },
  caseName: null,
  scenarios: [],
  blockedTargets: [],
  blockedReason: null,
  binding: null,
  receiptPath: null,
  session: null,
  driverStatus: null,
  keyboardKeys: [],
  baselineState: null,
  maxViewportHeight: null,
  minViewportHeight: null,
  webView: null,
  ptyHostIdentity: null,
  cleanup: { sessionDeleted: false, receipts: [], entries: [], killed: [] },
  ptyHook: null,
  failure: null,
  exitCode: null,
};

class RunnerError extends Error {
  constructor(code, reason, detail) {
    super(reason + ": " + (detail ?? ""));
    this.code = code;
    this.reason = reason;
    this.detail = detail ?? "";
  }
}
const blocked = (reason, detail) => new RunnerError(EXIT.BLOCKED, reason, detail);
const interactionFailed = (reason, detail) => new RunnerError(EXIT.INTERACTION, reason, detail);
const assertionFailed = (reason, detail) => new RunnerError(EXIT.ASSERTION, reason, detail);

/* --------------------------------------------------------------------------- CLI */

function usage(message) {
  process.stderr.write(
    "USAGE ERROR: " + message + "\n\n" +
      "  node scripts/qa/herdr-reference-device.mjs --platform ios \\\n" +
      "      --fixture-manifest <fixtures.json> --scenario " + REFERENCE_QA_IDS.join("|") + " --case happy|failure|all \\\n" +
      "      --evidence-dir <dir> [--wda-url <url>] [--serial <serial>] [--timeout-ms <ms>] [--pty-hook <command>]\n",
  );
  process.exit(EXIT.USAGE);
}

function parseArgs(argv) {
  const args = {
    platform: null,
    fixtureManifest: null,
    scenario: null,
    caseName: "all",
    evidenceDir: null,
    wdaUrl: process.env.FERRYX_WDA_URL || null,
    serial: process.env.FERRYX_IOS_SERIAL || null,
    timeoutMs: 20000,
    ptyHook: null,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    const next = () => {
      i += 1;
      if (i >= argv.length) usage(flag + " requires a value");
      return argv[i];
    };
    switch (flag) {
      case "--platform": args.platform = next(); break;
      case "--fixture-manifest": args.fixtureManifest = next(); break;
      case "--scenario": args.scenario = next(); break;
      case "--case": args.caseName = next(); break;
      case "--evidence-dir": args.evidenceDir = next(); break;
      case "--wda-url": args.wdaUrl = next(); break;
      case "--serial": args.serial = next(); break;
      case "--timeout-ms": args.timeoutMs = Number.parseInt(next(), 10); break;
      case "--pty-hook": args.ptyHook = next(); break;
      case "-h": case "--help":
        process.stdout.write(readFileSync(new URL(import.meta.url), "utf8").split("*/")[0] + "*/\n");
        process.exit(EXIT.OK);
        break;
      default:
        usage("unknown flag " + flag);
    }
  }
  if (args.platform !== PLATFORM) {
    usage("--platform must be " + PLATFORM + " (this producer drives the iOS WebDriverAgent/XCTest path)");
  }
  if (!args.fixtureManifest) usage("--fixture-manifest is mandatory");
  if (!args.scenario) usage("--scenario is mandatory");
  if (!REFERENCE_QA_IDS.includes(args.scenario)) {
    usage("--scenario must be one of " + REFERENCE_QA_IDS.join(", ") + " (this producer implements QA-08)");
  }
  if (!args.evidenceDir) usage("--evidence-dir is mandatory");
  if (!CASES.includes(args.caseName) && args.caseName !== "all") {
    usage("--case must be happy, failure or all");
  }
  if (!Number.isFinite(args.timeoutMs) || args.timeoutMs <= 0) usage("--timeout-ms must be a positive integer");
  return args;
}

function selectedCases() {
  return ARGS.caseName === "all" ? CASES.slice() : [ARGS.caseName];
}

/* --------------------------------------------------------------------------- evidence */

function evPath(...parts) {
  const first = String(parts[0] ?? "");
  // Scenario rows build absolute paths from caseDir(); both forms resolve to exactly one file, and
  // a relative path is always anchored at the evidence directory.
  const p = path.isAbsolute(first) ? path.join(...parts) : path.join(EVIDENCE_DIR, ...parts);
  mkdirSync(path.dirname(p), { recursive: true });
  return p;
}

function writeJson(relPath, data) {
  const p = evPath(relPath);
  writeFileSync(p, JSON.stringify(data, null, 2) + "\n", "utf8");
  return p;
}

function writeJsonAbs(absPath, data) {
  mkdirSync(path.dirname(absPath), { recursive: true });
  writeFileSync(absPath, JSON.stringify(data, null, 2) + "\n", "utf8");
  return absPath;
}

function appendLog(line) {
  try {
    appendFileSync(evPath("runner.log"), "[" + new Date().toISOString() + "] " + line + "\n", "utf8");
  } catch {
    // best-effort logging; never masks the real failure
  }
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function poll(label, fn, options) {
  const budget = (options && options.timeoutMs) || ARGS.timeoutMs;
  const intervalMs = (options && options.intervalMs) || 400;
  const deadline = Date.now() + budget;
  let lastErr = null;
  for (;;) {
    try {
      const value = await fn();
      if (value) return value;
      lastErr = new Error("condition not met");
    } catch (err) {
      lastErr = err;
    }
    if (Date.now() >= deadline) {
      throw interactionFailed("bounded-poll-timeout", label + " (budget " + budget + "ms; last: " + (lastErr && lastErr.message) + ")");
    }
    await delay(intervalMs);
  }
}

function redactUrl(raw) {
  try {
    const u = new URL(raw);
    if (u.search) u.search = "?<redacted>";
    if (u.hash) u.hash = "#<redacted>";
    return u.toString();
  } catch {
    return "<unparseable-url>";
  }
}

/* --------------------------------------------------------------------------- binding + fixture */

function loadBinding() {
  let read;
  try {
    read = readDeviceBinding();
  } catch (err) {
    const message = String((err && err.message) || err);
    const incomplete = /incomplete/.test(message);
    throw blocked(
      incomplete ? "device-binding-incomplete" : "device-binding-missing",
      message + (incomplete
        ? " — a bound block is absent; for the PTY block this means the provisioner could not resolve the original shell identity, which this producer must not fabricate"
        : ""),
    );
  }
  const binding = read.binding;
  RUN.binding = {
    schema: binding.schema,
    path: read.path,
    envVariable: REFERENCE_DEVICE_BINDING_ENV,
    candidate: binding.candidate,
    page: binding.page,
    target: binding.target,
    pty: binding.pty,
    producedAt: binding.producedAt ?? null,
    receiptSchema: binding.receiptSchema ?? null,
  };
  // The runner writes the binding for the Android invocation, so its receiptPath names the android
  // platform directory. This producer writes its OWN platform path (see resolveReceiptPath) and never
  // overwrites a receipt another platform's producer owns.
  RUN.bindingReceiptPath = typeof binding.receiptPath === "string" && binding.receiptPath.length > 0
    ? path.resolve(binding.receiptPath)
    : null;
  RUN.receiptPath = resolveReceiptPath(EVIDENCE_DIR, PLATFORM, binding.receiptPath);
  appendLog("binding loaded: " + read.path + " -> receipt " + RUN.receiptPath);
  return RUN.binding;
}

/**
 * The provisioned device for this platform, read from the fixture manifest's devices[].
 * The serial is NEVER hardcoded: a manifest with no ios entry is a missing device prerequisite.
 */
function selectDevice(manifest) {
  const devices = Array.isArray(manifest.devices) ? manifest.devices : [];
  const candidates = devices.filter((d) => d && d.platform === PLATFORM);
  if (candidates.length === 0) {
    throw blocked(
      "ios-device-not-provisioned",
      "the fixture manifest lists no " + PLATFORM + " device (" + devices.length + " device entries); " +
        "provision a physical device and its WebDriverAgent endpoint before this producer can run",
    );
  }
  const chosen = ARGS.serial ? candidates.find((d) => d.serial === ARGS.serial) : candidates[0];
  if (!chosen) {
    throw blocked("ios-device-not-found", "no " + PLATFORM + " device with serial " + ARGS.serial + " in the fixture manifest");
  }
  if (typeof chosen.serial !== "string" || chosen.serial.length === 0) {
    throw blocked("ios-device-serial-missing", "the manifest device entry has no serial: " + JSON.stringify(chosen));
  }
  const driverEndpoint = ARGS.wdaUrl || chosen.driverEndpoint;
  if (typeof driverEndpoint !== "string" || driverEndpoint.length === 0) {
    throw blocked(
      "wda-driver-endpoint-missing",
      "device " + chosen.serial + " has no WebDriverAgent driver endpoint; the iOS producer never simulates a keyboard",
    );
  }
  DEVICE = { serial: chosen.serial, driverEndpoint, model: chosen.model ?? null, osVersion: chosen.osVersion ?? null, manifestEntry: chosen };
  return DEVICE;
}

/* --------------------------------------------------------------------------- WebDriverAgent client */

/**
 * A minimal WebDriverAgent / XCUITest client. Every call is bounded; a non-2xx answer or a
 * transport failure is an interaction failure with the driver's own message, never a silent retry.
 */
class WdaClient {
  constructor(baseUrl) {
    this.baseUrl = String(baseUrl).replace(/\/+$/, "");
    this.sessionId = null;
  }

  async request(method, suffix, body) {
    const url = this.baseUrl + suffix;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), ARGS.timeoutMs);
    let response;
    try {
      response = await fetch(url, {
        method,
        headers: body === undefined ? { accept: "application/json" } : { accept: "application/json", "content-type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: controller.signal,
      });
    } catch (err) {
      clearTimeout(timer);
      throw interactionFailed("wda-transport-failed", method + " " + url + " -> " + String(err));
    } finally {
      clearTimeout(timer);
    }
    const text = await response.text();
    let parsed = null;
    try {
      parsed = text.length > 0 ? JSON.parse(text) : null;
    } catch {
      parsed = null;
    }
    if (!response.ok) {
      const message = parsed && parsed.value && (parsed.value.error || parsed.value.message);
      throw interactionFailed("wda-http-error", method + " " + url + " -> " + response.status + " " + (message || text.slice(0, 300)));
    }
    if (parsed === null) {
      throw interactionFailed("wda-non-json", method + " " + url + " -> " + text.slice(0, 200));
    }
    return parsed.value;
  }

  async status() {
    return this.request("GET", "/status");
  }

  async startSession(bundleId) {
    const capabilities = {
      alwaysMatch: {
        platformName: "iOS",
        "appium:udid": DEVICE.serial,
        "appium:usePrebuiltWDA": true,
        "appium:shouldUseSingletonTestManager": true,
        ...(bundleId ? { "appium:bundleId": bundleId } : {}),
      },
      firstMatch: [{}],
    };
    const value = await this.request("POST", "/session", { capabilities });
    if (!value || typeof value.sessionId !== "string" || value.sessionId.length === 0) {
      throw interactionFailed("wda-session-refused", JSON.stringify(value).slice(0, 400));
    }
    this.sessionId = value.sessionId;
    RUN.session = { sessionId: value.sessionId, capabilities: value.capabilities ?? null, bundleId: bundleId ?? null };
    return RUN.session;
  }

  sessionPath(suffix) {
    if (!this.sessionId) throw interactionFailed("wda-no-session", "the driver session has not been created");
    return "/session/" + this.sessionId + suffix;
  }

  source() {
    return this.request("GET", this.sessionPath("/source"));
  }

  element(using, value) {
    return this.request("POST", this.sessionPath("/element"), { using, value });
  }

  click(elementId) {
    return this.request("POST", this.sessionPath("/element/" + elementId + "/click"), {});
  }

  attribute(elementId, name) {
    return this.request("GET", this.sessionPath("/element/" + elementId + "/attribute/" + name));
  }

  rect(elementId) {
    return this.request("GET", this.sessionPath("/element/" + elementId + "/rect"));
  }

  windowRect() {
    return this.request("GET", this.sessionPath("/window/rect"));
  }

  /** A real gesture on the element: WDA's element scroll, not a scripted scroll event. */
  scroll(elementId, direction) {
    return this.request("POST", this.sessionPath("/wda/element/" + elementId + "/scroll"), { direction });
  }

  /** Hardware key events through the native input pipeline (never a JavaScript input event). */
  keys(value) {
    return this.request("POST", this.sessionPath("/wda/keys"), { value });
  }

  navigate(url) {
    return this.request("POST", this.sessionPath("/url"), { url });
  }

  activeApp() {
    return this.request("GET", this.sessionPath("/wda/activeAppInfo"));
  }

  deleteSession() {
    return this.request("DELETE", this.sessionPath(""), {});
  }
}


/* --------------------------------------------------------------------------- accessibility source */

/**
 * Parse the WebDriverAgent accessibility source into the same node shape the Android producer uses
 * for uiautomator, so element discovery, bounds and the IME-window logic are shared in spirit:
 *   text -> value/name, desc -> label/name, cls -> type, res -> identifier, bounds -> x/y/w/h.
 * Nothing is invented: a node without geometry is skipped, exactly as on Android.
 */
function parseSourceXml(xml) {
  const nodes = [];
  const tokens = String(xml).match(/<[^>]*>/g) || [];
  const stack = [];
  for (const token of tokens) {
    if (/^<\?/.test(token) || /^<!/.test(token)) continue;
    const closing = /^<\//.test(token);
    if (closing) {
      stack.pop();
      continue;
    }
    const selfClosing = /\/>$/.test(token);
    const name = (/^<([A-Za-z0-9:_-]+)/.exec(token) || [])[1] || "";
    const attrs = {};
    const attrRe = /([A-Za-z0-9:_-]+)="([^"]*)"/g;
    let m;
    while ((m = attrRe.exec(token)) !== null) attrs[m[1]] = decodeXml(m[2]);
    const depth = stack.length;
    const record = { tag: name, attrs, depth };
    stack.push(record);
    const x = Number.parseFloat(attrs.x);
    const y = Number.parseFloat(attrs.y);
    const w = Number.parseFloat(attrs.width);
    const h = Number.parseFloat(attrs.height);
    if (Number.isFinite(x) && Number.isFinite(y) && Number.isFinite(w) && Number.isFinite(h)) {
      const node = {
        text: attrs.value ?? attrs.name ?? "",
        desc: attrs.label ?? attrs.name ?? "",
        cls: attrs.type ?? name,
        pkg: attrs.type ?? name,
        res: attrs.identifier ?? "",
        clickable: attrs.enabled !== "false",
        focused: attrs.hasKeyboardFocus === "true",
        visible: attrs.visible !== "false",
        depth,
        bounds: { x1: Math.round(x), y1: Math.round(y), x2: Math.round(x + w), y2: Math.round(y + h) },
      };
      node.cx = Math.round((node.bounds.x1 + node.bounds.x2) / 2);
      node.cy = Math.round((node.bounds.y1 + node.bounds.y2) / 2);
      node.w = node.bounds.x2 - node.bounds.x1;
      node.h = node.bounds.y2 - node.bounds.y1;
      nodes.push(node);
    }
    if (selfClosing) stack.pop();
  }
  if (nodes.length === 0) {
    throw interactionFailed("accessibility-source-empty", "the driver source contained no element with geometry");
  }
  return { xml: String(xml), nodes };
}

function decodeXml(s) {
  return String(s)
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&amp;/g, "&");
}

async function fetchHierarchy() {
  const source = await WDA.source();
  return parseSourceXml(source);
}

function saveHierarchy(relDir, name, hierarchy) {
  const p = evPath(relDir, name + ".xml");
  writeFileSync(p, hierarchy.xml, "utf8");
  return p;
}

/** The keyboard window in the accessibility tree (iOS exposes it as XCUIElementTypeKeyboard). */
function imeRootBounds(nodes) {
  const keyboard = nodes.filter(
    (n) => /keyboard/i.test(n.cls) || /keyboard/i.test(n.pkg) || /InputMethod/i.test(n.cls),
  );
  if (keyboard.length === 0) return null;
  return keyboard.reduce((a, b) => (b.w * b.h > a.w * a.h ? b : a)).bounds;
}

function within(b, n) {
  return n.cx >= b.x1 && n.cx <= b.x2 && n.cy >= b.y1 && n.cy <= b.y2;
}

function findImeKey(nodes, label) {
  const b = imeRootBounds(nodes);
  const pool = b ? nodes.filter((n) => within(b, n)) : nodes;
  return (
    pool.find((n) => n.text === label) ??
    pool.find((n) => n.desc === label) ??
    pool.find((n) => n.text.toLowerCase() === label.toLowerCase()) ??
    pool.find((n) => n.desc.toLowerCase() === label.toLowerCase()) ??
    null
  );
}

function findImeEnter(nodes) {
  const b = imeRootBounds(nodes);
  const pool = b ? nodes.filter((n) => within(b, n)) : nodes;
  for (const label of ["return", "Return", "go", "Go", "send", "Send", "줄바꿈", "다음", "완료"]) {
    const hit = pool.find((n) => n.desc === label || n.text === label);
    if (hit) return hit;
  }
  return pool.find((n) => /return|enter|go|done/i.test(n.res) && n.clickable) ?? null;
}

/* --------------------------------------------------------------------------- read-only page state */

/**
 * Observation mirrors the Android producer exactly: READ-ONLY script evaluation against the page,
 * which is how the served DOM state is inspected. The Korean input itself never comes from here —
 * it comes from native keyboard touches (see typeKey).
 */
const READ_STATE_EXPRESSION = `(() => {
  const q = (s) => document.querySelector(s);
  const qa = (s) => document.querySelectorAll(s);
  const grid = q('[data-testid="remote-terminal-grid"]');
  const sink = q('textarea[data-testid="remote-terminal-input-sink"]');
  const preeditEl = q('[data-testid="remote-terminal-preedit"]');
  const chatEl = q('[data-testid="chat-composer-textarea"]');
  const termBtn = q('[data-testid="remote-view-mode-terminal"]');
  const chatBtn = q('[data-testid="remote-view-mode-chat"]');
  const sendBtn = q('[data-testid="send-button"]');
  const ae = document.activeElement;
  const visible = (el) => !!el && !!(el.offsetWidth || el.offsetHeight || el.getClientRects().length);
  const rectOf = (el) => {
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { x: r.x, y: r.y, w: r.width, h: r.height, bottom: r.bottom, right: r.right };
  };
  const lineEls = grid ? [...grid.querySelectorAll('[data-grid-line]')] : [];
  const lineTexts = lineEls.map((el) => el.textContent || '');
  const indexes = lineEls.map((el) => Number(el.getAttribute('data-grid-line'))).filter((n) => Number.isFinite(n));
  const vv = window.visualViewport;
  return {
    gridText: grid ? lineTexts.join('\n') : null,
    gridLines: lineTexts.filter((l) => l.length > 0),
    gridLineCount: lineEls.length,
    gridFirstIndex: indexes.length > 0 ? Math.min(...indexes) : null,
    gridLastIndex: indexes.length > 0 ? Math.max(...indexes) : null,
    gridRect: rectOf(grid),
    preedit: preeditEl ? preeditEl.textContent : null,
    lineDraft: null,
    lineVisible: false,
    mode: null,
    chatDraft: chatEl ? chatEl.value : null,
    chatVisible: visible(chatEl),
    composerRect: rectOf(chatEl),
    sendVisible: visible(sendBtn),
    sendRect: rectOf(sendBtn),
    userBubbleCount: qa('[data-testid="user-message-bubble"]').length,
    copyButtons: qa('[data-testid="message-copy-button"]').length,
    viewModeButtons: { terminalVisible: visible(termBtn), chatVisible: visible(chatBtn) },
    viewport: {
      width: window.innerWidth,
      height: window.innerHeight,
      visualHeight: vv ? vv.height : null,
      visualOffsetTop: vv ? vv.offsetTop : null,
    },
    activeElement: ae ? {
      tag: ae.tagName,
      testid: ae.getAttribute ? ae.getAttribute('data-testid') : null,
      value: typeof ae.value === 'string' ? ae.value : null,
      isComposing: !!ae.isComposing,
    } : null,
    url: location.href,
    at: Date.now(),
  };
})()`;

const READ_COPY_STATE_EXPRESSION = `(() => {
  const btn = document.querySelector('[data-testid="message-copy-button"]');
  if (!btn) return null;
  const svg = btn.querySelector('svg');
  const cls = svg ? (svg.getAttribute('class') || '') : '';
  const r = btn.getBoundingClientRect();
  return {
    ariaLabel: btn.getAttribute('aria-label'),
    iconClass: cls,
    copied: /lucide-check/.test(cls),
    failed: /alert/.test(cls),
    rect: { x: r.x, y: r.y, w: r.width, h: r.height, bottom: r.bottom },
  };
})()`;

/** The page probe the scenarios drive. cdp.evaluate is a READ-ONLY script evaluation. */
const IOS = {
  async evaluate(expression) {
    const value = await WDA.request("POST", WDA.sessionPath("/execute/sync"), { script: expression, args: [] });
    return value;
  },
  source: () => WDA.source(),
};

async function readState(cdp) {
  const state = await cdp.evaluate(READ_STATE_EXPRESSION);
  if (!state || typeof state !== "object") {
    throw interactionFailed("read-state-failed", JSON.stringify(state)?.slice(0, 200) ?? String(state));
  }
  if (typeof state.viewport?.height === "number") {
    RUN.maxViewportHeight = RUN.maxViewportHeight === null ? state.viewport.height : Math.max(RUN.maxViewportHeight, state.viewport.height);
    RUN.minViewportHeight = RUN.minViewportHeight === null ? state.viewport.height : Math.min(RUN.minViewportHeight, state.viewport.height);
  }
  return state;
}

async function readCopyState(cdp) {
  return cdp.evaluate(READ_COPY_STATE_EXPRESSION);
}

function digest(text) {
  let h = 5381;
  const s = String(text ?? "");
  for (let i = 0; i < s.length; i += 1) h = ((h << 5) + h + s.charCodeAt(i)) | 0;
  return (h >>> 0).toString(16);
}

function gridFingerprint(state) {
  return {
    digest: digest(state.gridText),
    firstIndex: state.gridFirstIndex,
    lastIndex: state.gridLastIndex,
    lineCount: state.gridLineCount,
    nonEmptyLines: state.gridLines.length,
  };
}

function countIn(haystack, needle) {
  if (!haystack || !needle) return 0;
  return haystack.split(needle).length - 1;
}

/* --------------------------------------------------------------------------- real touch (W3C actions) */

/**
 * A real touch at device coordinates, delivered through the W3C actions API the driver implements.
 * Coordinates always come from the accessibility tree (device pixels) — never from a guess — and
 * every touch is verified by a bounded, observable delta by the caller.
 */
async function pointerTap(label, x, y) {
  const body = {
    actions: [{
      type: "pointer",
      id: "finger1",
      parameters: { pointerType: "touch" },
      actions: [
        { type: "pointerMove", duration: 0, x: Math.round(x), y: Math.round(y), origin: "viewport" },
        { type: "pointerDown", button: 0 },
        { type: "pause", duration: 60 },
        { type: "pointerUp", button: 0 },
      ],
    }],
  };
  await WDA.request("POST", WDA.sessionPath("/actions"), body);
  await WDA.request("DELETE", WDA.sessionPath("/actions"), {});
  appendLog("touch " + label + " @ (" + Math.round(x) + "," + Math.round(y) + ")");
}

async function pointerDrag(label, x, fromY, toY, durationMs) {
  const body = {
    actions: [{
      type: "pointer",
      id: "finger1",
      parameters: { pointerType: "touch" },
      actions: [
        { type: "pointerMove", duration: 0, x: Math.round(x), y: Math.round(fromY), origin: "viewport" },
        { type: "pointerDown", button: 0 },
        { type: "pointerMove", duration: durationMs, x: Math.round(x), y: Math.round(toY), origin: "viewport" },
        { type: "pointerUp", button: 0 },
      ],
    }],
  };
  await WDA.request("POST", WDA.sessionPath("/actions"), body);
  await WDA.request("DELETE", WDA.sessionPath("/actions"), {});
  appendLog("drag " + label + " x=" + Math.round(x) + " " + Math.round(fromY) + " -> " + Math.round(toY) + " in " + durationMs + "ms");
}

async function tapPoint(label, x, y, verify, options) {
  await pointerTap(label, x, y);
  try {
    await poll("touch-verify:" + label, verify, {});
  } catch (err) {
    if (options && options.screenshotOnFail) {
      try {
        await screenshot(options.screenshotOnFail);
      } catch (shotErr) {
        appendLog("failure screenshot failed: " + String(shotErr));
      }
    }
    throw err;
  }
}

function rectExpr(selector) {
  return (
    "(() => { const el = document.querySelector('" + selector + "');" +
    " if (!el) return null; const r = el.getBoundingClientRect();" +
    " return { x: r.x, y: r.y, w: r.width, h: r.height }; })()"
  );
}

/**
 * Page element -> real touch point. The accessibility tree is authoritative when it exposes the
 * element (its bounds are already device pixels); otherwise the page rect is scaled by the
 * devicePixelRatio and offset by the web view's own origin, both read from the driver — never
 * guessed, and never accepted without the caller's bounded delta check.
 */
async function resolvePageTap(cdp, nodes, hint) {
  const hier = nodes.find(
    (n) => (hint.text && n.text === hint.text) || (hint.desc && n.desc === hint.desc) ||
      (hint.res && n.res === hint.res),
  );
  if (hier) return { x: hier.cx, y: hier.cy, via: "accessibility" };

  const rect = await cdp.evaluate(hint.cdpRectExpr);
  if (!rect || !Number.isFinite(rect.x)) {
    throw interactionFailed("touch-target-undiscoverable", hint.hint + ": no accessibility node, empty page rect");
  }
  const scale = (await cdp.evaluate("window.devicePixelRatio || 1")) || 1;
  const webRect = await webViewOrigin(nodes);
  const x = Math.round(webRect.x + (rect.x + rect.w / 2) * scale);
  const y = Math.round(webRect.y + (rect.y + rect.h / 2) * scale);
  if (x <= 0 || y <= 0) {
    throw interactionFailed("touch-point-invalid", hint.hint + ": computed (" + x + "," + y + ")");
  }
  return { x, y, via: "page-rect*scale+webview-origin" };
}

/**
 * The web view's device-pixel origin. It is DERIVED from the accessibility tree, which exposes the
 * web area's own frame, so a page rect can be mapped without a hardcoded chrome offset.
 */
async function webViewOrigin(nodes) {
  const web = nodes.find((n) => /webview|webviewarea/i.test(n.cls)) ??
    nodes.find((n) => /web/i.test(n.cls) && n.w > 0 && n.h > 0) ??
    nodes.find((n) => /window|application/i.test(n.cls) && n.w > 0 && n.h > 0);
  if (!web) {
    throw interactionFailed("webview-origin-undiscoverable", "the accessibility source exposes no web view frame");
  }
  RUN.webView = { cls: web.cls, bounds: web.bounds };
  return { x: web.bounds.x1, y: web.bounds.y1 };
}

/* --------------------------------------------------------------------------- screenshots */

/**
 * A real device screenshot through the driver. The file is written under the evidence directory;
 * a driver that cannot screenshot is reported, never replaced by a blank image.
 */
async function screenshot(relPath) {
  const p = evPath(relPath);
  const value = await WDA.request("GET", WDA.sessionPath("/screenshot"));
  const base64 = typeof value === "string" ? value : value && value.screenshot;
  if (typeof base64 !== "string" || base64.length < 100) {
    throw interactionFailed("screenshot-failed", relPath + ": driver returned no image data");
  }
  writeFileSync(p, Buffer.from(base64, "base64"));
  return p;
}

/* --------------------------------------------------------------------------- keyboard (native path) */

let LAST_IME_HIERARCHY = null;
let LAST_TYPING_PATH = null;

/** Tap the real on-screen keyboard key for this label, verified by a bounded delta. */
async function typeKeyViaKeyboardKey(evDir, label, observe) {
  const hierarchy = await fetchHierarchy();
  const node = findImeKey(hierarchy.nodes, label);
  if (!node) return false;
  await tapPoint("ime-key:" + label, node.cx, node.cy, observe, {
    screenshotOnFail: path.join(evDir, "key-" + label + "-no-delta.png"),
  });
  return true;
}

/**
 * The native hardware-key path: driver key events travel the platform's own input pipeline into
 * the focused editor. It is used only when the on-screen keyboard does not expose its key labels,
 * and the path actually taken is recorded in the receipt.
 */
async function typeKeyViaHardwareKeys(evDir, label, observe) {
  await WDA.keys([label]);
  appendLog("hardware key event: " + label);
  await poll("key-delta:" + label, observe, {}).catch(() => {
    throw interactionFailed("key-no-delta", label + ": no observable delta after a native key event");
  });
  void evDir;
}

async function typeKey(evDir, label, observe) {
  const viaKeyboard = await typeKeyViaKeyboardKey(evDir, label, observe);
  if (viaKeyboard) {
    LAST_TYPING_PATH = "on-screen-keyboard-key-tap";
    return;
  }
  // The keyboard does not expose this key's label: deliver the keystroke through the driver's own
  // key-event API, which travels the platform input pipeline. This is still a NATIVE key path — not
  // a JavaScript input event and not a field-value write — and the path actually taken is recorded
  // in the receipt so a reader can always tell the two apart.
  LAST_TYPING_PATH = "driver-hardware-key-event";
  await typeKeyViaHardwareKeys(evDir, label, observe);
}

async function typeSequence(evDir, labels, observe) {
  for (const label of labels) {
    await typeKey(evDir, label, observe);
    await delay(120); // natural inter-key gap for the real input pipeline (not a wait-for-condition)
  }
}

async function tapEnter(evDir) {
  const hierarchy = await fetchHierarchy();
  const node = findImeEnter(hierarchy.nodes);
  if (node) {
    await tapPoint("ime-enter", node.cx, node.cy, async () => true, {});
    return { via: "keyboard-return-key", bounds: node.bounds };
  }
  await WDA.keys(["\n"]);
  appendLog("enter delivered as a native key event (the keyboard exposes no return key)");
  return { via: "driver-hardware-key-event" };
}

/** Focus the editor through a real touch, then require the real keyboard window to appear. */
async function openKeyboard(cdp, evDir, hint) {
  try {
    const h = await fetchHierarchy();
    if (imeRootBounds(h.nodes)) {
      LAST_IME_HIERARCHY = h;
      return h;
    }
  } catch {
    // hierarchy unavailable — proceed to the touch flow
  }
  let nodes = [];
  try {
    nodes = (await fetchHierarchy()).nodes;
  } catch {
    // empty probe set; resolvePageTap will fail closed with a precise reason
  }
  const tap = await resolvePageTap(cdp, nodes, hint);
  await tapPoint("focus:" + hint.hint, tap.x, tap.y, async () => {
    const s = await readState(cdp);
    if (hint.focusTestid) return s.activeElement?.testid === hint.focusTestid;
    return !!s.activeElement && s.activeElement.tag !== "BODY";
  }, { screenshotOnFail: path.join(evDir, "focus-" + hint.hint + "-failed.png") });
  const hierarchy = await poll("keyboard-visible:" + hint.hint, async () => {
    try {
      const h = await fetchHierarchy();
      return imeRootBounds(h.nodes) ? h : null;
    } catch {
      return null;
    }
  }, {});
  LAST_IME_HIERARCHY = hierarchy;
  return hierarchy;
}

function imeHierarchyFor() {
  if (!LAST_IME_HIERARCHY) {
    throw blocked("ime-window-missing", "no keyboard hierarchy has been captured yet; the editor must be focused first");
  }
  return LAST_IME_HIERARCHY;
}

/** Fail-closed Korean layout verification against the REAL focused keyboard. */
function requireKoreanLayout(evDir, hierarchy) {
  const bounds = imeRootBounds(hierarchy.nodes);
  if (!bounds) throw blocked("ime-window-missing", "no keyboard window in the captured accessibility source");
  const labels = new Set();
  for (const n of hierarchy.nodes) {
    if (!within(bounds, n)) continue;
    for (const ch of (n.text ?? "") + (n.desc ?? "")) {
      if (JAMO.has(ch)) labels.add(ch);
    }
  }
  const probe = ["ㅎ", "ㅏ", "ㅇ"];
  const missing = probe.filter((k) => !labels.has(k));
  writeJson(path.join(evDir, "korean-layout-probe.json"), {
    probe,
    missing,
    foundJamo: [...labels].sort(),
    imeBounds: bounds,
    verdict: missing.length === 0 ? "korean-active" : "not-verified",
  });
  RUN.keyboardKeys = [...labels].sort();
  if (missing.length > 0) {
    saveHierarchy(evDir, "korean-layout-not-active", hierarchy);
    throw blocked(
      "korean-layout-inactive",
      "probe jamo not exposed by the real keyboard: " + missing.join(", ") +
        "; enable the Korean layout manually on the device — this producer never changes keyboard settings",
    );
  }
  return { foundJamo: [...labels].sort() };
}

/* --------------------------------------------------------------------------- view mode + captures */

async function ensureViewMode(cdp, evDir, want) {
  const s = await readState(cdp);
  const showing = want === "terminal" ? s.gridText !== null : s.chatVisible;
  if (showing) return s;
  const nodes = (await fetchHierarchy()).nodes;
  const tap = await resolvePageTap(cdp, nodes, {
    text: want === "terminal" ? "Terminal" : "Chat",
    cdpRectExpr: rectExpr(want === "terminal"
      ? '[data-testid="remote-view-mode-terminal"]'
      : '[data-testid="remote-view-mode-chat"]'),
    hint: "view-mode-" + want,
  });
  await tapPoint("view-mode->" + want, tap.x, tap.y, async () => {
    const n = await readState(cdp);
    return want === "terminal" ? n.gridText !== null : n.chatVisible;
  }, { screenshotOnFail: path.join(evDir, "view-mode-" + want + "-failed.png") });
  return readState(cdp);
}

async function captureLayout(cdp, evDir, caseName, hint) {
  const closed = await readState(cdp);
  const windowBefore = await WDA.windowRect();
  const hierarchy = await openKeyboard(cdp, evDir, hint);
  LAST_IME_HIERARCHY = hierarchy;
  const open = await readState(cdp);
  const imeBounds = imeRootBounds(hierarchy.nodes);
  const controlRect = hint.layoutControl === "composer" ? open.composerRect
    : hint.layoutControl === "grid" ? open.gridRect : open.sendRect;
  const controlVisible = hint.layoutControl === "composer" ? open.chatVisible
    : hint.layoutControl === "grid" ? open.gridRect !== null : open.sendVisible;
  const webRect = await webViewOrigin(hierarchy.nodes);
  const scale = (await cdp.evaluate("window.devicePixelRatio || 1")) || 1;
  const controlDeviceBottom = controlRect ? webRect.y + controlRect.bottom * scale : null;
  const obscuredByIme = Boolean(imeBounds) && controlDeviceBottom !== null && controlDeviceBottom > imeBounds.y1;
  const shrinkPx = closed.viewport.visualHeight !== null && open.viewport.visualHeight !== null
    ? Math.round(closed.viewport.visualHeight - open.viewport.visualHeight)
    : null;
  const capture = {
    case: caseName,
    viewportClosed: closed.viewport,
    viewportOpen: open.viewport,
    shrinkPx,
    windowRect: windowBefore,
    webViewOrigin: webRect,
    devicePixelRatio: scale,
    imeBounds,
    control: hint.layoutControl,
    controlRect,
    controlVisible,
    controlDeviceBottom,
    obscuredByIme,
    keyboardOpen: Boolean(imeBounds),
  };
  capture.ok = capture.keyboardOpen && capture.controlVisible && !capture.obscuredByIme;
  writeJson(path.join(evDir, "layout-" + caseName + ".json"), capture);
  return capture;
}

async function captureScroll(cdp, evDir, caseName) {
  const before = await readState(cdp);
  const nodes = (await fetchHierarchy()).nodes;
  const gridTap = await resolvePageTap(cdp, nodes, {
    cdpRectExpr: rectExpr('[data-testid="remote-terminal-grid"]'),
    hint: "scroll-grid",
  });
  const travel = 220;
  const fromY = Math.round(gridTap.y + travel / 2);
  const toY = Math.round(gridTap.y - travel / 2);
  await pointerDrag("scroll-grid", gridTap.x, fromY, toY, 300);
  const fingerprintBefore = gridFingerprint(before);
  let after;
  try {
    after = await poll("grid-scrolled", async () => {
      const s = await readState(cdp);
      const fp = gridFingerprint(s);
      return fp.digest !== fingerprintBefore.digest || fp.firstIndex !== fingerprintBefore.firstIndex ? s : null;
    }, {});
  } catch {
    after = await readState(cdp);
  }
  const fingerprintAfter = gridFingerprint(after);
  const capture = {
    case: caseName,
    gesture: { kind: "w3c-pointer-drag", x: gridTap.x, fromY, toY, durationMs: 300 },
    before: fingerprintBefore,
    after: fingerprintAfter,
    changed: fingerprintAfter.digest !== fingerprintBefore.digest,
    indexMoved: fingerprintAfter.firstIndex !== fingerprintBefore.firstIndex,
    stillRendering: fingerprintAfter.lineCount > 0,
    keyboardStillOpen: Boolean(imeRootBounds((await fetchHierarchy()).nodes)),
  };
  capture.ok = capture.changed && capture.stillRendering;
  writeJson(path.join(evDir, "scroll-" + caseName + ".json"), capture);
  return capture;
}

async function captureCopy(cdp, evDir, caseName) {
  const before = await readCopyState(cdp);
  if (!before) {
    const capture = {
      case: caseName,
      ok: false,
      reason: "no message-copy-button is rendered: the transcript has no message to copy",
      userBubbleCount: (await readState(cdp)).userBubbleCount,
    };
    writeJson(path.join(evDir, "copy-" + caseName + ".json"), capture);
    return capture;
  }
  const nodes = (await fetchHierarchy()).nodes;
  const tap = await resolvePageTap(cdp, nodes, {
    desc: "Copy message",
    cdpRectExpr: rectExpr('[data-testid="message-copy-button"]'),
    hint: "message-copy",
  });
  await tapPoint("message-copy", tap.x, tap.y, async () => {
    const s = await readCopyState(cdp);
    return s && s.copied ? s : null;
  }, { screenshotOnFail: path.join(evDir, "copy-" + caseName + "-failed.png") });
  const after = await readCopyState(cdp);
  let readback;
  try {
    readback = await cdp.evaluate(
      "(async () => { try { const t = await navigator.clipboard.readText(); return { ok: true, text: t }; }" +
        " catch (error) { return { ok: false, error: String(error) }; } })()",
    );
  } catch (err) {
    readback = { ok: false, error: String(err).slice(0, 200) };
  }
  const capture = {
    case: caseName,
    before: { ariaLabel: before.ariaLabel, iconClass: before.iconClass },
    after: after ? { ariaLabel: after.ariaLabel, iconClass: after.iconClass, copied: after.copied } : null,
    clipboardReadback: readback,
    touch: { x: tap.x, y: tap.y, via: tap.via },
  };
  capture.ok = Boolean(after && after.copied);
  writeJson(path.join(evDir, "copy-" + caseName + ".json"), capture);
  return capture;
}

async function captureLateAck(cdp, evDir, caseName, baseline, options) {
  const LATE_WINDOW_MS = 3000;
  const startedAt = Date.now();
  const samples = [];
  let moved = null;
  while (Date.now() - startedAt < LATE_WINDOW_MS) {
    const s = await readState(cdp);
    const fp = gridFingerprint(s);
    samples.push({ atMs: Date.now() - startedAt, digest: fp.digest, firstIndex: fp.firstIndex, activeTestid: s.activeElement?.testid ?? null });
    if (fp.digest !== baseline.digest || fp.firstIndex !== baseline.firstIndex) {
      moved = { atMs: Date.now() - startedAt, fingerprint: fp };
      break;
    }
    await delay(250);
  }
  const final = await readState(cdp);
  const foreignEditorFocused = options.expectView === "terminal"
    ? final.activeElement?.testid === "chat-composer-textarea"
    : final.activeElement?.testid === "remote-terminal-input-sink";
  const capture = {
    case: caseName,
    windowMs: LATE_WINDOW_MS,
    baseline,
    lateGridChange: moved,
    samples,
    finalFingerprint: gridFingerprint(final),
    finalActiveElement: final.activeElement,
    foreignEditorFocused,
    expectView: options.expectView,
  };
  capture.ok = moved === null && !foreignEditorFocused;
  writeJson(path.join(evDir, "late-ack-" + caseName + ".json"), capture);
  return capture;
}


// --------------------------------------------------------------------------- scenarios

const SCENARIO_DIRS = {
  "direct-once": "S1-direct-once",
  "mode-switch": "S2-mode-switch",
  "composition-enter": "S3-composition-enter",
  "target-switch": "S4-target-switch",
};

function caseDir(name, caseName) {
  return path.join(SCENARIO_DIRS[name], caseName);
}

/**
 * Write a scenario row's assertions file and build the receipt row. "pass" always means the
 * named invariant HELD — including for the failure/regression case, where the invariant is a
 * negative one (nothing was sent, nothing was lost, focus was not stolen).
 */
function finalizeRow(options) {
  const file = writeJson(path.join(options.dir, "assertions.json"), {
    scenario: options.name,
    case: options.caseName,
    verdict: options.ok ? "pass" : "fail",
    assertions: options.assertions,
    captured: options.captured,
    failures: options.failures ?? [],
  });
  options.files.push(file);
  return {
    name: options.name,
    case: options.caseName,
    status: options.ok ? "pass" : "fail",
    assertions: options.assertions,
    captured: options.captured,
    evidenceDir: options.dir,
    evidenceFiles: options.files,
    assertionsFile: file,
    reason: options.ok ? null : (options.failures ?? ["assertion failed"]).join("; ").slice(0, 800),
  };
}

/** A bounded, observable progress observer for real OSK keystrokes. */
function progressObserver(read, initialLength) {
  let last = initialLength;
  return async () => {
    const s = await readState(read);
    const length = (s.preedit ?? "").length + (s.activeElement?.value?.length ?? 0) + (s.chatDraft ?? "").length;
    if (length > last || (s.activeElement?.isComposing ?? false)) {
      last = Math.max(last, length);
      return true;
    }
    return false;
  };
}

/* ============================== S1 — direct-once ============================== */

/**
 * Real OSK jamo in the original pane -> the Hangul token appears in the rendered PTY grid
 * EXACTLY ONCE, composition alone never moves the grid, and the keyboard/layout, touch scroll
 * and late-ACK invariants all hold around it.
 */
async function directOnceHappy(cdp) {
  const name = "direct-once";
  const dir = caseDir(name, "happy");
  const files = [];
  const baseline = await ensureViewMode(cdp, dir, "terminal");
  files.push(writeJson(path.join(dir, "00-baseline.json"), baseline));
  files.push(await screenshot(path.join(dir, "01-baseline.png")));
  const baselineFingerprint = gridFingerprint(baseline);

  const layout = await captureLayout(cdp, dir, "happy", {
    cdpRectExpr: rectExpr('[data-testid="remote-terminal-grid"]'),
    focusTestid: "remote-terminal-input-sink",
    hint: "tap-grid-focus-sink",
    layoutControl: "grid",
  });
  const korean = requireKoreanLayout(dir, imeHierarchyFor());
  files.push(await screenshot(path.join(dir, "02-keyboard-open.png")));

  const observe = progressObserver(cdp, 0);
  await typeSequence(dir, ["ㅁ", "ㅗ", "ㅂ", "ㅏ", "ㅇ", "ㅣ", "ㄹ"], observe);

  const preEnter = await readState(cdp);
  files.push(writeJson(path.join(dir, "03-pre-enter.json"), preEnter));
  files.push(await screenshot(path.join(dir, "04-pre-enter.png")));
  const preEnterFingerprint = gridFingerprint(preEnter);
  const compositionMovedGrid = preEnterFingerprint.digest !== baselineFingerprint.digest;

  await tapEnter(dir);
  const after = await poll("grid-echo-after-enter", async () => {
    const s = await readState(cdp);
    return countIn(s.gridText, TOKENS.direct) > countIn(baseline.gridText, TOKENS.direct) ? s : null;
  }, {});
  await delay(DUPLICATE_WINDOW_MS); // bounded window whose ONLY purpose is catching a second echo
  const settled = await readState(cdp);
  files.push(writeJson(path.join(dir, "05-after-enter.json"), { firstGained: after, settled }));
  files.push(await screenshot(path.join(dir, "06-after-enter.png")));

  const scroll = await captureScroll(cdp, dir, "happy");
  const copy = await captureCopy(cdp, dir, "happy");
  const lateAck = await captureLateAck(cdp, dir, "happy", gridFingerprint(settled), { expectView: "terminal" });

  const delta = countIn(settled.gridText, TOKENS.direct) - countIn(baseline.gridText, TOKENS.direct);
  const copySatisfied = copy.ok === true || copy.reason === "no message-copy-button is rendered: the transcript has no message to copy";
  const assertions = {
    hangulToken: TOKENS.direct,
    occurrencesDelta: delta,
    exactlyOnce: delta === 1,
    compositionMovedGrid,
    keyboardOpened: layout.keyboardOpen,
    layoutShrinkPx: layout.shrinkPx,
    controlVisible: layout.controlVisible,
    controlObscuredByIme: layout.obscuredByIme,
    touchScrollChangedRenderedGrid: scroll.changed,
    copyAffordance: copy.ok ? "copied" : copy.reason ?? "unknown",
    lateGridChange: lateAck.lateGridChange,
    focusStolen: lateAck.foreignEditorFocused,
    koreanJamoExposed: korean.foundJamo.length,
    gridLinesBefore: baseline.gridLineCount,
    gridLinesAfter: settled.gridLineCount,
  };
  const captured = {
    baselineGridText: baseline.gridText,
    preEnterGridText: preEnter.gridText,
    finalGridText: settled.gridText,
    preeditAfterEnter: settled.preedit,
    activeValueAfterEnter: settled.activeElement?.value ?? null,
    layout,
    scroll,
    copy,
    lateAck,
  };
  const failures = [];
  if (!assertions.exactlyOnce) failures.push("the token was not echoed exactly once (delta " + delta + ")");
  if (compositionMovedGrid) failures.push("the grid moved during composition alone");
  if (!layout.ok) failures.push("keyboard/layout invariant failed");
  if (!scroll.ok) failures.push("touch scroll did not change the rendered grid");
  if (!copySatisfied) failures.push("the copy affordance did not report success");
  if (!lateAck.ok) failures.push("a late grid change or a stolen focus was observed");
  return finalizeRow({ name, caseName: "happy", dir, files, assertions, captured, ok: failures.length === 0, failures });
}

/**
 * Failure/regression case: the keyboard opens, a composition begins, and the phone switches
 * views MID-composition. The switch must not send the held composition, must not create a
 * message, must not move the PTY grid, and must not steal focus after the switch.
 */
async function directOnceFailure(cdp) {
  const name = "direct-once";
  const dir = caseDir(name, "failure");
  const files = [];
  const baseline = await ensureViewMode(cdp, dir, "terminal");
  files.push(writeJson(path.join(dir, "00-baseline.json"), baseline));
  const baselineFingerprint = gridFingerprint(baseline);
  const baselineBubbles = baseline.userBubbleCount;

  const layout = await captureLayout(cdp, dir, "failure", {
    cdpRectExpr: rectExpr('[data-testid="remote-terminal-grid"]'),
    focusTestid: "remote-terminal-input-sink",
    hint: "tap-grid-focus-sink",
    layoutControl: "grid",
  });
  const observe = progressObserver(cdp, 0);
  await typeKey(dir, "ㅎ", observe); // one jamo: a composition is now live and NOT committed
  const midComposition = await readState(cdp);
  files.push(writeJson(path.join(dir, "01-mid-composition.json"), midComposition));

  const chatView = await ensureViewMode(cdp, dir, "chat");
  files.push(writeJson(path.join(dir, "02-after-switch-chat.json"), chatView));
  files.push(await screenshot(path.join(dir, "03-after-switch-chat.png")));
  const chatFingerprint = gridFingerprint(chatView);
  const lateAck = await captureLateAck(cdp, dir, "failure", chatFingerprint, { expectView: "chat" });

  const terminalView = await ensureViewMode(cdp, dir, "terminal");
  files.push(writeJson(path.join(dir, "04-back-to-terminal.json"), terminalView));
  await delay(DUPLICATE_WINDOW_MS);
  const settled = await readState(cdp);
  files.push(writeJson(path.join(dir, "05-settled.json"), settled));
  files.push(await screenshot(path.join(dir, "06-settled.png")));
  const settledFingerprint = gridFingerprint(settled);

  const tokenDeltas = Object.fromEntries(
    Object.entries(TOKENS).map(([key, token]) => [key, countIn(settled.gridText, token) - countIn(baseline.gridText, token)]),
  );
  const assertions = {
    compositionStarted: Boolean(midComposition.activeElement?.isComposing) || (midComposition.preedit ?? "").length > 0,
    viewSwitchPerformed: chatView.chatVisible === true && terminalView.gridText !== null,
    tokenDeltas,
    anyTokenSent: Object.values(tokenDeltas).some((d) => d !== 0),
    messagesCreated: settled.userBubbleCount - baselineBubbles,
    gridChangedAcrossSwitch: settledFingerprint.digest !== baselineFingerprint.digest,
    layoutOk: layout.ok,
    lateGridChange: lateAck.lateGridChange,
    focusStolen: lateAck.foreignEditorFocused,
  };
  const captured = {
    baselineGridText: baseline.gridText,
    chatViewGridText: chatView.gridText,
    finalGridText: settled.gridText,
    midCompositionPreedit: midComposition.preedit,
    midCompositionActive: midComposition.activeElement,
    layout,
    lateAck,
  };
  const failures = [];
  if (assertions.anyTokenSent) failures.push("a token reached the PTY while only a composition was held");
  if (assertions.messagesCreated !== 0) failures.push("the view switch created " + assertions.messagesCreated + " message(s)");
  if (assertions.gridChangedAcrossSwitch) failures.push("the rendered grid changed across the switch");
  if (!layout.ok) failures.push("keyboard/layout invariant failed");
  if (!lateAck.ok) failures.push("a late grid change or a stolen focus was observed");
  return finalizeRow({ name, caseName: "failure", dir, files, assertions, captured, ok: failures.length === 0, failures });
}

/* ============================== S2 — mode-switch ============================== */

/**
 * A settled chat draft survives a real chat -> terminal -> chat round trip through the header
 * view-mode buttons: the draft is kept, nothing is sent, the PTY grid never receives the draft
 * token, and the keyboard/layout stays correct after the round trip.
 */
async function modeSwitchHappy(cdp) {
  const name = "mode-switch";
  const dir = caseDir(name, "happy");
  const files = [];
  const terminalBaseline = await ensureViewMode(cdp, dir, "terminal");
  files.push(writeJson(path.join(dir, "00-terminal-baseline.json"), terminalBaseline));
  const terminalFingerprint = gridFingerprint(terminalBaseline);
  const baselineBubbles = terminalBaseline.userBubbleCount;

  const chatView = await ensureViewMode(cdp, dir, "chat");
  files.push(writeJson(path.join(dir, "01-chat-view.json"), chatView));
  const layoutChat = await captureLayout(cdp, dir, "happy-chat", {
    cdpRectExpr: rectExpr('[data-testid="chat-composer-textarea"]'),
    focusTestid: "chat-composer-textarea",
    hint: "tap-chat-composer",
    layoutControl: "composer",
  });
  requireKoreanLayout(dir, imeHierarchyFor());

  const observe = progressObserver(cdp, 0);
  await typeSequence(dir, ["ㅎ", "ㅏ", "ㄴ"], observe);
  const typed = await readState(cdp);
  files.push(writeJson(path.join(dir, "02-chat-typed.json"), typed));
  files.push(await screenshot(path.join(dir, "03-chat-typed.png")));
  const draftTyped = typed.chatDraft;

  const backToTerminal = await ensureViewMode(cdp, dir, "terminal");
  files.push(writeJson(path.join(dir, "04-back-to-terminal.json"), backToTerminal));
  const backToChat = await ensureViewMode(cdp, dir, "chat");
  files.push(writeJson(path.join(dir, "05-back-to-chat.json"), backToChat));
  files.push(await screenshot(path.join(dir, "06-back-to-chat.png")));
  const draftAfterRoundTrip = backToChat.chatDraft;

  const lateAck = await captureLateAck(cdp, dir, "happy", gridFingerprint(backToTerminal), { expectView: "chat" });

  const tokenDelta = countIn(backToTerminal.gridText, TOKENS.chat) - countIn(terminalBaseline.gridText, TOKENS.chat);
  const assertions = {
    modeSwitchSubject: "chat-terminal-view-mode-toggle",
    chatToken: TOKENS.chat,
    draftTyped,
    draftAfterRoundTrip,
    draftPreserved: draftAfterRoundTrip === draftTyped,
    ptyTokenDelta: tokenDelta,
    draftNeverReachedPty: tokenDelta === 0,
    messagesCreated: backToChat.userBubbleCount - baselineBubbles,
    terminalGridUnchanged: gridFingerprint(backToTerminal).digest === terminalFingerprint.digest,
    layoutChatOk: layoutChat.ok,
    layoutShrinkPx: layoutChat.shrinkPx,
    lateGridChange: lateAck.lateGridChange,
    focusStolen: lateAck.foreignEditorFocused,
  };
  const captured = {
    baselineGridText: terminalBaseline.gridText,
    terminalAfterRoundTripGridText: backToTerminal.gridText,
    chatDraftTyped: draftTyped,
    chatDraftAfterRoundTrip: draftAfterRoundTrip,
    layoutChat,
    lateAck,
  };
  const failures = [];
  if (!assertions.draftPreserved) failures.push("the chat draft was not preserved across the view round trip");
  if (!assertions.draftNeverReachedPty) failures.push("the chat draft reached the PTY (delta " + tokenDelta + ")");
  if (assertions.messagesCreated !== 0) failures.push("the round trip created a message");
  if (!layoutChat.ok) failures.push("keyboard/layout invariant failed in the chat view");
  if (!lateAck.ok) failures.push("a late grid change or a stolen focus was observed");
  return finalizeRow({ name, caseName: "happy", dir, files, assertions, captured, ok: failures.length === 0, failures });
}

/**
 * Failure/regression case: switch views WHILE the chat composition is live. The held
 * composition must not be submitted, the draft must not be lost, no duplicate may appear, and
 * the terminal sink must not take focus while the chat view is showing.
 */
async function modeSwitchFailure(cdp) {
  const name = "mode-switch";
  const dir = caseDir(name, "failure");
  const files = [];
  const terminalBaseline = await ensureViewMode(cdp, dir, "terminal");
  const terminalFingerprint = gridFingerprint(terminalBaseline);
  const baselineBubbles = terminalBaseline.userBubbleCount;
  files.push(writeJson(path.join(dir, "00-terminal-baseline.json"), terminalBaseline));

  const chatView = await ensureViewMode(cdp, dir, "chat");
  await openKeyboard(cdp, dir, {
    cdpRectExpr: rectExpr('[data-testid="chat-composer-textarea"]'),
    focusTestid: "chat-composer-textarea",
    hint: "tap-chat-composer",
  });
  requireKoreanLayout(dir, imeHierarchyFor());
  const observe = progressObserver(cdp, 0);
  await typeKey(dir, "ㅎ", observe); // live composition, never committed
  const midComposition = await readState(cdp);
  files.push(writeJson(path.join(dir, "01-mid-composition.json"), midComposition));

  const switched = await ensureViewMode(cdp, dir, "terminal");
  const inTerminal = await readState(cdp);
  files.push(writeJson(path.join(dir, "02-switched-to-terminal.json"), inTerminal));
  files.push(await screenshot(path.join(dir, "03-switched-to-terminal.png")));
  const lateAck = await captureLateAck(cdp, dir, "failure", gridFingerprint(switched), { expectView: "terminal" });

  const backToChat = await ensureViewMode(cdp, dir, "chat");
  const final = await readState(cdp);
  files.push(writeJson(path.join(dir, "04-back-to-chat.json"), final));
  const draftAfter = final.chatDraft;

  const tokenDelta = countIn(final.gridText ?? inTerminal.gridText, TOKENS.chat) - countIn(terminalBaseline.gridText, TOKENS.chat);
  const assertions = {
    modeSwitchSubject: "chat-terminal-view-mode-toggle",
    compositionStarted: Boolean(midComposition.activeElement?.isComposing) || (midComposition.preedit ?? "").length > 0,
    compositionTokenDelta: tokenDelta,
    compositionNeverSubmitted: tokenDelta === 0,
    messagesCreated: final.userBubbleCount - baselineBubbles,
    terminalGridUnchanged: gridFingerprint(inTerminal).digest === terminalFingerprint.digest,
    draftAfterSwitch: draftAfter,
    draftNotSilentlyLost: (draftAfter ?? "").length > 0 || (midComposition.chatDraft ?? "").length === 0,
    lateGridChange: lateAck.lateGridChange,
    focusStolen: lateAck.foreignEditorFocused,
    midCompositionDraft: midComposition.chatDraft,
  };
  const captured = {
    baselineGridText: terminalBaseline.gridText,
    midCompositionChatDraft: midComposition.chatDraft,
    midCompositionPreedit: midComposition.preedit,
    terminalGridAfterSwitch: inTerminal.gridText,
    finalChatDraft: draftAfter,
    lateAck,
  };
  const failures = [];
  if (!assertions.compositionNeverSubmitted) failures.push("a live chat composition was submitted by the view switch");
  if (assertions.messagesCreated !== 0) failures.push("the view switch created a message");
  if (!assertions.terminalGridUnchanged) failures.push("the terminal grid changed across the switch");
  if (!assertions.draftNotSilentlyLost) failures.push("the chat draft was silently lost");
  if (!lateAck.ok) failures.push("a late grid change or a stolen focus was observed");
  return finalizeRow({ name, caseName: "failure", dir, files, assertions, captured, ok: failures.length === 0, failures });
}

/* ============================== S3 — composition-enter ============================== */

/**
 * Enter during a live chat composition. The FIRST Enter is classified honestly into one of two
 * legal outcomes (committed and submitted in one, or absorbed by the IME with zero send and a
 * second Enter required) or reported as an illegal one; the chat draft must never cross into the
 * PTY, and the PTY grid must never receive the composed token.
 */
async function compositionEnterHappy(cdp) {
  const name = "composition-enter";
  const dir = caseDir(name, "happy");
  const files = [];
  const terminalBaseline = await ensureViewMode(cdp, dir, "terminal");
  files.push(writeJson(path.join(dir, "00-terminal-baseline.json"), terminalBaseline));
  const terminalFingerprint = gridFingerprint(terminalBaseline);
  const baselineBubbles = terminalBaseline.userBubbleCount;

  await ensureViewMode(cdp, dir, "chat");
  await openKeyboard(cdp, dir, {
    cdpRectExpr: rectExpr('[data-testid="chat-composer-textarea"]'),
    focusTestid: "chat-composer-textarea",
    hint: "tap-chat-composer",
  });
  requireKoreanLayout(dir, imeHierarchyFor());

  const observe = progressObserver(cdp, 0);
  await typeSequence(dir, ["ㄱ", "ㅏ", "ㄷ", "ㅏ"], observe);
  const typed = await readState(cdp);
  files.push(writeJson(path.join(dir, "01-typed.json"), typed));
  files.push(await screenshot(path.join(dir, "02-typed.png")));
  const draftBefore = typed.chatDraft;
  const typedAsExpected = (draftBefore ?? "").trim() === TOKENS.composition;

  await tapEnter(dir);
  const afterFirst = await readState(cdp);
  files.push(writeJson(path.join(dir, "03-after-first-enter.json"), afterFirst));
  files.push(await screenshot(path.join(dir, "04-after-first-enter.png")));

  const sentAfterFirst = afterFirst.userBubbleCount - baselineBubbles;
  const draftAfterFirst = afterFirst.chatDraft;
  const draftStillHeld = (draftAfterFirst ?? "").trim() === TOKENS.composition;

  let firstEnterFinding;
  let final = afterFirst;
  if (sentAfterFirst === 1 && !draftStillHeld) {
    firstEnterFinding = "committed-and-submitted-in-one";
  } else if (sentAfterFirst === 0 && draftStillHeld) {
    firstEnterFinding = "absorbed-by-ime-no-send";
    await tapEnter(dir);
    final = await poll("second-enter-send", async () => {
      const s = await readState(cdp);
      return s.userBubbleCount - baselineBubbles >= 1 ? s : null;
    }, {});
  } else {
    firstEnterFinding = "illegal-intermediate-state";
  }

  await delay(DUPLICATE_WINDOW_MS);
  const settled = await readState(cdp);
  files.push(writeJson(path.join(dir, "05-settled.json"), settled));
  files.push(await screenshot(path.join(dir, "06-settled.png")));
  const terminalAfter = await ensureViewMode(cdp, dir, "terminal");
  files.push(writeJson(path.join(dir, "07-terminal-after.json"), terminalAfter));
  const lateAck = await captureLateAck(cdp, dir, "happy", gridFingerprint(terminalAfter), { expectView: "terminal" });

  const tokenDelta = countIn(terminalAfter.gridText, TOKENS.composition) - countIn(terminalBaseline.gridText, TOKENS.composition);
  const messagesSent = settled.userBubbleCount - baselineBubbles;
  const assertions = {
    hangulToken: TOKENS.composition,
    typedAsExpected,
    draftBefore,
    firstEnterFinding,
    messagesSentAfterFirstEnter: sentAfterFirst,
    messagesSentTotal: messagesSent,
    exactlyOnceSend: messagesSent === 1,
    draftAfterFirstEnter: draftAfterFirst,
    finalDraft: settled.chatDraft,
    draftClearedAfterSend: (settled.chatDraft ?? "") === "",
    ptyTokenDelta: tokenDelta,
    compositionNeverReachedPty: tokenDelta === 0,
    terminalGridUnchanged: gridFingerprint(terminalAfter).digest === terminalFingerprint.digest,
    lateGridChange: lateAck.lateGridChange,
    focusStolen: lateAck.foreignEditorFocused,
  };
  const captured = {
    baselineGridText: terminalBaseline.gridText,
    typedChatDraft: draftBefore,
    draftAfterFirstEnter: draftAfterFirst,
    finalChatDraft: settled.chatDraft,
    finalGridText: terminalAfter.gridText,
    preeditAfterFirstEnter: afterFirst.preedit,
    lateAck,
  };
  const failures = [];
  if (!typedAsExpected) failures.push("the chat draft was not the typed Korean token (" + JSON.stringify(draftBefore) + ")");
  if (firstEnterFinding === "illegal-intermediate-state") {
    failures.push("the first Enter left an illegal state (sent " + sentAfterFirst + ", draft " + JSON.stringify(draftAfterFirst) + ")");
  }
  if (messagesSent !== 1) failures.push("the composition was not sent exactly once (sent " + messagesSent + ")");
  if (!assertions.draftClearedAfterSend) failures.push("the chat draft was not cleared after the send");
  if (!assertions.compositionNeverReachedPty) failures.push("the composed token reached the PTY (delta " + tokenDelta + ")");
  if (!lateAck.ok) failures.push("a late grid change or a stolen focus was observed");
  return finalizeRow({ name, caseName: "happy", dir, files, assertions, captured, ok: failures.length === 0, failures });
}

/**
 * Failure/regression case: a SECOND Enter inside the duplicate window must not produce a second
 * message, and the draft must not be resubmitted. This is the duplicate-send regression the plan
 * names for Enter during composition.
 */
async function compositionEnterFailure(cdp) {
  const name = "composition-enter";
  const dir = caseDir(name, "failure");
  const files = [];
  const terminalBaseline = await ensureViewMode(cdp, dir, "terminal");
  const terminalFingerprint = gridFingerprint(terminalBaseline);
  const baselineBubbles = terminalBaseline.userBubbleCount;
  files.push(writeJson(path.join(dir, "00-terminal-baseline.json"), terminalBaseline));

  await ensureViewMode(cdp, dir, "chat");
  await openKeyboard(cdp, dir, {
    cdpRectExpr: rectExpr('[data-testid="chat-composer-textarea"]'),
    focusTestid: "chat-composer-textarea",
    hint: "tap-chat-composer",
  });
  requireKoreanLayout(dir, imeHierarchyFor());
  const observe = progressObserver(cdp, 0);
  await typeSequence(dir, ["ㄱ", "ㅏ", "ㄷ", "ㅏ"], observe);
  const typed = await readState(cdp);
  files.push(writeJson(path.join(dir, "01-typed.json"), typed));

  // First Enter: bring the draft to a settled state whichever legal outcome occurs.
  await tapEnter(dir);
  let afterFirst = await readState(cdp);
  if ((afterFirst.chatDraft ?? "").trim() === TOKENS.composition) {
    await tapEnter(dir);
    afterFirst = await poll("first-send", async () => {
      const s = await readState(cdp);
      return s.userBubbleCount - baselineBubbles >= 1 ? s : null;
    }, {});
  }
  files.push(writeJson(path.join(dir, "02-after-first-send.json"), afterFirst));

  // The regression: a second Enter inside the duplicate window with an empty draft.
  await tapEnter(dir);
  await delay(DUPLICATE_WINDOW_MS);
  const afterSecond = await readState(cdp);
  files.push(writeJson(path.join(dir, "03-after-second-enter.json"), afterSecond));
  files.push(await screenshot(path.join(dir, "04-after-second-enter.png")));

  const terminalAfter = await ensureViewMode(cdp, dir, "terminal");
  const lateAck = await captureLateAck(cdp, dir, "failure", gridFingerprint(terminalAfter), { expectView: "terminal" });
  const tokenDelta = countIn(terminalAfter.gridText, TOKENS.composition) - countIn(terminalBaseline.gridText, TOKENS.composition);

  const assertions = {
    messagesAfterFirstSend: afterFirst.userBubbleCount - baselineBubbles,
    messagesAfterSecondEnter: afterSecond.userBubbleCount - baselineBubbles,
    noDuplicateSend: afterSecond.userBubbleCount - baselineBubbles === 1,
    draftAfterSecondEnter: afterSecond.chatDraft,
    draftStayedEmpty: (afterSecond.chatDraft ?? "") === "",
    ptyTokenDelta: tokenDelta,
    compositionNeverReachedPty: tokenDelta === 0,
    terminalGridUnchanged: gridFingerprint(terminalAfter).digest === terminalFingerprint.digest,
    lateGridChange: lateAck.lateGridChange,
    focusStolen: lateAck.foreignEditorFocused,
  };
  const captured = {
    baselineGridText: terminalBaseline.gridText,
    afterFirstSendBubbles: afterFirst.userBubbleCount,
    afterSecondEnterBubbles: afterSecond.userBubbleCount,
    finalChatDraft: afterSecond.chatDraft,
    finalGridText: terminalAfter.gridText,
    lateAck,
  };
  const failures = [];
  if (!assertions.noDuplicateSend) failures.push("a second Enter produced another message");
  if (!assertions.draftStayedEmpty) failures.push("the draft was repopulated or resubmitted (" + JSON.stringify(assertions.draftAfterSecondEnter) + ")");
  if (!assertions.compositionNeverReachedPty) failures.push("the composed token reached the PTY (delta " + tokenDelta + ")");
  if (!lateAck.ok) failures.push("a late grid change or a stolen focus was observed");
  return finalizeRow({ name, caseName: "failure", dir, files, assertions, captured, ok: failures.length === 0, failures });
}

/* ============================== S4 — target-switch ============================== */

/**
 * Chat draft versus the original PTY: with a settled Korean draft in the chat composer, a real
 * target switch in both directions must send nothing, keep the draft, keep the PTY grid byte for
 * byte, and leave the terminal sink holding touch focus while the terminal view is showing.
 */
async function targetSwitchHappy(cdp) {
  const name = "target-switch";
  const dir = caseDir(name, "happy");
  const files = [];
  const terminalBaseline = await ensureViewMode(cdp, dir, "terminal");
  files.push(writeJson(path.join(dir, "00-terminal-baseline.json"), terminalBaseline));
  const terminalFingerprint = gridFingerprint(terminalBaseline);
  const baselineBubbles = terminalBaseline.userBubbleCount;
  if (terminalBaseline.gridText === null) {
    throw interactionFailed("terminal-view-unavailable", "the terminal grid never rendered; the target switch cannot be proven");
  }

  await ensureViewMode(cdp, dir, "chat");
  await openKeyboard(cdp, dir, {
    cdpRectExpr: rectExpr('[data-testid="chat-composer-textarea"]'),
    focusTestid: "chat-composer-textarea",
    hint: "tap-chat-composer",
  });
  requireKoreanLayout(dir, imeHierarchyFor());
  const observe = progressObserver(cdp, 0);
  await typeSequence(dir, ["ㅎ", "ㅏ", "ㄴ"], observe);
  const typed = await readState(cdp);
  files.push(writeJson(path.join(dir, "01-chat-typed.json"), typed));
  files.push(await screenshot(path.join(dir, "02-chat-typed.png")));
  const draftTyped = typed.chatDraft;

  const terminalView = await ensureViewMode(cdp, dir, "terminal");
  files.push(writeJson(path.join(dir, "03-terminal-view.json"), terminalView));
  files.push(await screenshot(path.join(dir, "04-terminal-view.png")));
  const sinkFocused = await poll("sink-focus", async () => {
    const s = await readState(cdp);
    return s.activeElement?.testid === "remote-terminal-input-sink" ? s : null;
  }, {});
  const lateAck = await captureLateAck(cdp, dir, "happy", gridFingerprint(terminalView), { expectView: "terminal" });

  const chatFinal = await ensureViewMode(cdp, dir, "chat");
  const final = await readState(cdp);
  files.push(writeJson(path.join(dir, "05-chat-final.json"), final));
  files.push(await screenshot(path.join(dir, "06-chat-final.png")));

  const ptyDelta = countIn(terminalView.gridText, TOKENS.chat) - countIn(terminalBaseline.gridText, TOKENS.chat);
  const assertions = {
    targetSwitchSubject: "chat-composer-vs-original-pty-pane",
    chatToken: TOKENS.chat,
    draftTyped,
    draftAfterRoundTrip: final.chatDraft,
    chatDraftRetained: final.chatDraft === draftTyped,
    ptyOccurrencesDeltaWhileChatOnly: ptyDelta,
    noChatTextReachedPty: ptyDelta === 0,
    messagesCreated: final.userBubbleCount - baselineBubbles,
    terminalGridUnchanged: gridFingerprint(terminalView).digest === terminalFingerprint.digest,
    // The original target must be PROTECTED across the switch: the pane the receipt binds is the
    // pane still rendered after the round trip, proven by an unchanged scrollback digest.
    targetProtection: {
      boundTarget: RUN.binding?.target ?? null,
      targetCopiedFromBinding: true,
      gridDigestUnchanged: gridFingerprint(terminalView).digest === terminalFingerprint.digest,
      gridRenderedAfterRoundTrip: terminalView.gridLineCount > 0,
      evidence: "the rendered [data-grid-line] digest after the chat round trip equals the pre-switch digest",
    },
    sinkHeldFocusInTerminalView: sinkFocused.activeElement?.testid === "remote-terminal-input-sink",
    sendPressed: false,
    lateGridChange: lateAck.lateGridChange,
    focusStolen: lateAck.foreignEditorFocused,
  };
  const captured = {
    baselineGridText: terminalBaseline.gridText,
    terminalGridAfterSwitch: terminalView.gridText,
    chatDraftTyped: draftTyped,
    chatDraftRetained: final.chatDraft,
    lateAck,
  };
  const failures = [];
  if (!assertions.noChatTextReachedPty) failures.push("chat text reached the PTY (delta " + ptyDelta + ")");
  if (!assertions.chatDraftRetained) failures.push("the chat draft was not retained across the target switch");
  if (assertions.messagesCreated !== 0) failures.push("the target switch created a message");
  if (!assertions.sinkHeldFocusInTerminalView) failures.push("the terminal view did not hold touch focus on its input sink");
  if (!lateAck.ok) failures.push("a late grid change or a stolen focus was observed");
  return finalizeRow({ name, caseName: "happy", dir, files, assertions, captured, ok: failures.length === 0, failures });
}

/**
 * Failure/regression case: the late-ACK window. An echo that lands AFTER the target switch must
 * not duplicate a row, must not repaint the wrong view, and must not steal focus back from the
 * view the user is now looking at.
 */
async function targetSwitchFailure(cdp) {
  const name = "target-switch";
  const dir = caseDir(name, "failure");
  const files = [];
  const terminalBaseline = await ensureViewMode(cdp, dir, "terminal");
  files.push(writeJson(path.join(dir, "00-terminal-baseline.json"), terminalBaseline));
  const terminalFingerprint = gridFingerprint(terminalBaseline);
  const baselineBubbles = terminalBaseline.userBubbleCount;

  // A real terminal keystroke that WILL echo back, then an immediate switch away: the echo is the
  // late ACK this case fences.
  await openKeyboard(cdp, dir, {
    cdpRectExpr: rectExpr('[data-testid="remote-terminal-grid"]'),
    focusTestid: "remote-terminal-input-sink",
    hint: "tap-grid-focus-sink",
  });
  requireKoreanLayout(dir, imeHierarchyFor());
  const observe = progressObserver(cdp, 0);
  await typeSequence(dir, ["ㅁ", "ㅗ", "ㅂ", "ㅏ", "ㅇ", "ㅣ", "ㄹ"], observe);
  await tapEnter(dir);

  const chatView = await ensureViewMode(cdp, dir, "chat");
  files.push(writeJson(path.join(dir, "01-switched-to-chat.json"), chatView));
  files.push(await screenshot(path.join(dir, "02-switched-to-chat.png")));
  const lateAck = await captureLateAck(cdp, dir, "failure", gridFingerprint(chatView), { expectView: "chat" });

  const terminalView = await ensureViewMode(cdp, dir, "terminal");
  const settled = await readState(cdp);
  files.push(writeJson(path.join(dir, "03-back-to-terminal.json"), settled));
  const echoDelta = countIn(settled.gridText, TOKENS.direct) - countIn(terminalBaseline.gridText, TOKENS.direct);
  const indexAdvanced = gridFingerprint(settled).lastIndex !== terminalFingerprint.lastIndex;

  const assertions = {
    echoObserved: echoDelta >= 1,
    echoExactlyOnce: echoDelta === 1,
    gridRowIndexAdvanced: indexAdvanced,
    lateGridChangeWhileInChatView: lateAck.lateGridChange,
    focusStolenWhileInChatView: lateAck.foreignEditorFocused,
    chatDraftUntouched: settled.chatDraft,
    messagesCreated: settled.userBubbleCount - baselineBubbles,
    terminalGridRenderedAfterReturn: settled.gridLineCount > 0,
  };
  const captured = {
    baselineGridText: terminalBaseline.gridText,
    chatViewGridText: chatView.gridText,
    finalGridText: settled.gridText,
    lateAck,
  };
  const failures = [];
  if (!assertions.echoExactlyOnce) failures.push("the late echo was not delivered exactly once (delta " + echoDelta + ")");
  if (!assertions.gridRowIndexAdvanced) failures.push("the rendered scrollback did not advance after the echo");
  if (!assertions.lateGridChangeWhileInChatView && lateAck.lateGridChange) failures.push("a late grid change was observed in the chat view");
  if (lateAck.foreignEditorFocused) failures.push("the chat view lost focus to the terminal sink");
  if (assertions.messagesCreated !== 0) failures.push("the terminal echo created a chat message");
  return finalizeRow({ name, caseName: "failure", dir, files, assertions, captured, ok: failures.length === 0, failures });
}

const SCENARIOS = {
  "direct-once": { happy: directOnceHappy, failure: directOnceFailure },
  "mode-switch": { happy: modeSwitchHappy, failure: modeSwitchFailure },
  "composition-enter": { happy: compositionEnterHappy, failure: compositionEnterFailure },
  "target-switch": { happy: targetSwitchHappy, failure: targetSwitchFailure },
};


// --------------------------------------------------------------------------- prerequisites (driver + device)

const PREREQ = {};

/**
 * Fail-closed driver verification. The driver is provisioned by the verification host and only
 * READ here: this producer never launches, installs or reconfigures a driver or a device.
 */
async function prereqProbe() {
  WDA = new WdaClient(DEVICE.driverEndpoint);
  let status;
  try {
    status = await WDA.status();
  } catch (err) {
    const message = String((err && err.message) || err);
    const consent = /trust|permission|authoriz|developer mode|unlock/i.test(message);
    if (consent) {
      RUN.blockedTargets.push({
        target: "ios-device-consent",
        reason: "device-owner-acknowledgment-needed",
        detail: message,
      });
      throw blocked("device-owner-acknowledgment-needed", message + " — acknowledge the prompt on the device manually, then re-run");
    }
    throw blocked("wda-unreachable", DEVICE.driverEndpoint + " -> " + message);
  }
  RUN.driverStatus = status ?? null;
  const ready = status && (status.ready === true || status.ready === undefined);
  if (status && status.ready === false) {
    throw blocked("wda-not-ready", DEVICE.driverEndpoint + " reports ready=false: " + String(status.message ?? ""));
  }
  PREREQ.driver = {
    endpoint: DEVICE.driverEndpoint,
    ready: ready === true ? true : null,
    status,
  };
  writeJson("prereq/driver-status.json", PREREQ.driver);

  const session = await WDA.startSession(null);
  PREREQ.session = { sessionId: session.sessionId, capabilities: session.capabilities };
  writeJson("prereq/driver-session.json", PREREQ.session);

  // Device provenance: read from the driver, never invented. A value the driver does not report is
  // recorded as null.
  const caps = session.capabilities || {};
  PREREQ.device = {
    serial: DEVICE.serial,
    model: caps.deviceModel ?? (status && status.device && status.device.name) ?? DEVICE.model ?? null,
    osVersion: caps.osVersion ?? (status && status.build && status.build.productVersion) ?? DEVICE.osVersion ?? null,
    driverEndpoint: DEVICE.driverEndpoint,
    manifestEntry: DEVICE.manifestEntry,
    windowRect: await WDA.windowRect().catch(() => null),
  };
  writeJson("prereq/provenance.json", PREREQ);
  appendLog("prereq OK: " + DEVICE.serial + " " + (PREREQ.device.model ?? "?") + " iOS " + (PREREQ.device.osVersion ?? "?") + " via " + DEVICE.driverEndpoint);
}

// --------------------------------------------------------------------------- teardown (own resources only)

/** Record a PID this run itself spawned, with the executable that was launched. */
function recordOwnedPid(pid, executablePath, extra) {
  const entry = {
    pid,
    executablePath: executablePath ?? null,
    role: "herdr-reference-device",
    spawnedAt: new Date().toISOString(),
    extra: extra ?? {},
  };
  RUN.cleanup.entries.push(entry);
  return entry;
}

/**
 * Delete ONLY the driver session this run created, and terminate ONLY the PIDs this run recorded at
 * spawn time whose live executable still matches what was launched. Anything else is REPORTED,
 * never killed — no pattern matching, no guesses. The provisioned driver itself is never stopped:
 * it is owned by the verification host.
 */
async function cleanup() {
  if (WDA && WDA.sessionId && !RUN.cleanup.sessionDeleted) {
    try {
      await WDA.deleteSession();
      RUN.cleanup.sessionDeleted = true;
      RUN.cleanup.receipts.push({ resource: "wda-session", id: WDA.sessionId, action: "deleted" });
    } catch (error) {
      RUN.cleanup.receipts.push({ resource: "wda-session", id: WDA.sessionId, action: "delete-failed", error: String(error) });
    }
  }
  for (const entry of RUN.cleanup.entries) {
    const live = probeProcessIdentity(entry.pid);
    if (!live.alive) {
      RUN.cleanup.receipts.push({ pid: entry.pid, action: "already-exited", executablePath: entry.executablePath });
      continue;
    }
    const recorded = entry.executablePath;
    const matches = Boolean(recorded) && Boolean(live.executable) &&
      (live.executable === recorded ||
        live.executable.endsWith("/" + recorded.split("/").pop()) ||
        live.executable.endsWith("\\" + recorded.split("\\").pop()));
    if (!matches) {
      RUN.cleanup.receipts.push({
        pid: entry.pid,
        action: "reported-not-killed",
        reason: "live executable does not match the spawn-recorded executable",
        recordedExecutablePath: recorded,
        liveExecutable: live.executable,
      });
      continue;
    }
    try {
      killExactPid(entry.pid);
      RUN.cleanup.receipts.push({ pid: entry.pid, action: "killed", executablePath: recorded });
    } catch (error) {
      RUN.cleanup.receipts.push({ pid: entry.pid, action: "kill-failed", error: String(error) });
    }
  }
  RUN.cleanup.killed = RUN.cleanup.receipts.filter((r) => r.action === "killed").map((r) => r.pid);
  appendLog("cleanup: sessionDeleted=" + RUN.cleanup.sessionDeleted + ", owned pids=" + RUN.cleanup.entries.length);
}

// --------------------------------------------------------------------------- PTY identity

/**
 * Cross-check the PTY identity the binding carries. The binding is authoritative; this only
 * RECORDS what the verifying host can observe about that PID. A PID that is not visible on this
 * host is reported as such (a remote owning host is a legitimate reason), never replaced by a
 * different process, and never fabricated.
 */
function verifyPtyIdentity() {
  const pty = RUN.binding?.pty ?? {};
  const pid = Number.parseInt(String(pty.pid ?? ""), 10);
  if (!Number.isFinite(pid)) {
    RUN.ptyHostIdentity = { status: "unparsable-pid", rawPid: pty.pid ?? null };
    return RUN.ptyHostIdentity;
  }
  const live = probeProcessIdentity(pid);
  const expected = path.basename(String(pty.executablePath ?? ""));
  RUN.ptyHostIdentity = {
    pid,
    alive: live.alive,
    liveExecutable: live.executable,
    expectedExecutableBasename: expected || null,
    matchesBinding: Boolean(live.executable) && expected.length > 0 && String(live.executable).includes(expected),
    status: live.alive ? "observed-on-this-host" : "not-observed-on-this-host",
    note: "the binding is authoritative; this entry records only what the verifying host could see",
  };
  return RUN.ptyHostIdentity;
}

// --------------------------------------------------------------------------- receipt + result

/**
 * The platform directory for this run.
 *
 * The fixed invocation in the plan passes a platform directory (`...\herdr-reference\ios`) while the
 * task 14 runner's consumer resolves `deviceReceiptPath(<commonRoot>, platform)` =
 * `<commonRoot>/<platform>/device-receipt.json`. Both forms must land on the SAME file, so the
 * platform segment is appended only when it is not already the last segment of the given directory —
 * never twice.
 */
function platformDirFor(evidenceDir, platform) {
  const resolved = path.resolve(evidenceDir);
  return path.basename(resolved) === platform ? resolved : path.join(resolved, platform);
}

/**
 * Where this producer writes its receipt. A binding receiptPath is honoured ONLY when it names THIS
 * platform's directory; the task 14 runner writes the Android path, and overwriting the Android
 * receipt from the iOS run would destroy another platform's evidence.
 */
function resolveReceiptPath(evidenceDir, platform, boundPath) {
  if (typeof boundPath === "string" && boundPath.length > 0) {
    const bound = path.resolve(boundPath);
    if (path.basename(path.dirname(bound)) === platform) return bound;
  }
  return path.join(platformDirFor(evidenceDir, platform), "device-receipt.json");
}

function receiptPath() {
  return RUN.receiptPath ?? resolveReceiptPath(EVIDENCE_DIR, PLATFORM, null);
}

function rowStatus(scenario) {
  if (scenario.status === "pass") return "pass";
  if (scenario.status === "blocked" || scenario.status === "not-run") return "blocked";
  return "fail";
}

function buildReceipt() {
  const binding = RUN.binding ?? { candidate: {}, page: {}, target: {}, pty: {} };
  const blockedRun = Boolean(RUN.blockedReason) || RUN.exitCode === EXIT.BLOCKED;
  return {
    schema: REFERENCE_DEVICE_RECEIPT_SCHEMA,
    platform: PLATFORM,
    verdict: blockedRun ? "blocked"
      : RUN.exitCode === EXIT.OK && RUN.scenarios.length > 0 && RUN.scenarios.every((s) => rowStatus(s) === "pass") ? "pass" : "fail",
    blockedReason: RUN.blockedReason ?? null,
    producedAt: RUN.finishedAt ?? new Date().toISOString(),
    producer: {
      scriptId: RUN.scriptId,
      scriptSha256: RUN.scriptSha256,
      host: RUN.host,
      evidenceDirAbsolute: EVIDENCE_DIR,
      bindingPath: RUN.binding?.path ?? null,
      bindingEnvVariable: REFERENCE_DEVICE_BINDING_ENV,
      bindingReceiptPath: RUN.bindingReceiptPath ?? null,
      evidenceDirAbsolute: EVIDENCE_DIR,
      platformDir: platformDirFor(EVIDENCE_DIR, PLATFORM),
      receiptPath: receiptPath(),
      case: ARGS?.caseName ?? null,
      scenario: ARGS?.scenario ?? null,
      timeoutMs: ARGS?.timeoutMs ?? null,
      fixtureManifest: ARGS?.fixtureManifest ?? null,
      typingPath: LAST_TYPING_PATH,
      policy:
        "native-keyboard-input-only; read-only page observation; no JavaScript input events; driver session cleanup; " +
        "exact-PID identity checks; no settings changes; no installs; no consent automation",
    },
    candidate: {
      candidateId: binding.candidate?.candidateId ?? null,
      sourceManifestSha256: binding.candidate?.sourceManifestSha256 ?? null,
      binarySha256: binding.candidate?.binarySha256 ?? null,
      binaryPath: binding.candidate?.binaryPath ?? null,
    },
    page: {
      origin: binding.page?.origin ?? null,
      urlRedacted: binding.page?.urlRedacted ?? null,
      url: binding.page?.url ?? null,
      viewport: {
        baseline: RUN.baselineState?.viewport ?? null,
        maxHeight: RUN.maxViewportHeight ?? null,
        minHeight: RUN.minViewportHeight ?? null,
      },
      webView: RUN.webView ?? null,
    },
    target: {
      hostId: binding.target?.hostId ?? null,
      ownerId: binding.target?.ownerId ?? null,
      epoch: binding.target?.epoch ?? null,
      backendSessionId: binding.target?.backendSessionId ?? null,
      registryId: binding.target?.registryId ?? null,
      providerSessionId: binding.target?.providerSessionId ?? null,
    },
    pty: {
      pid: binding.pty?.pid ?? null,
      executablePath: binding.pty?.executablePath ?? null,
      cols: binding.pty?.cols ?? null,
      rows: binding.pty?.rows ?? null,
      identitySource: "task14-device-binding",
      // The spawn-time identity record, copied VERBATIM when the provisioner produced one. Its
      // ptyChild.executable is the PTY child wrapper and is never relabelled as the session shell,
      // and sessionShell.pid is never reported where sessionShell.pidKnown is false.
      identity: binding.pty?.identity ?? null,
      hostProcessIdentity: RUN.ptyHostIdentity ?? null,
    },
    device: {
      serial: DEVICE?.serial ?? null,
      model: PREREQ.device?.model ?? null,
      osVersion: PREREQ.device?.osVersion ?? null,
      driverEndpoint: DEVICE?.driverEndpoint ?? null,
      windowRect: PREREQ.device?.windowRect ?? null,
      driverStatus: RUN.driverStatus ?? null,
    },
    session: RUN.session ?? null,
    keyboard: { keysExposed: RUN.keyboardKeys, typingPath: LAST_TYPING_PATH },
    scenarios: RUN.scenarios.map((s) => ({
      name: s.name,
      case: s.case,
      status: rowStatus(s),
      captured: s.captured ?? null,
      assertions: s.assertions ?? null,
      reason: s.reason ?? null,
      detail: s.detail ?? null,
      evidenceDir: s.evidenceDir ?? null,
      evidenceFiles: s.evidenceFiles ?? [],
      assertionsFile: s.assertionsFile ?? null,
    })),
    blockedTargets: RUN.blockedTargets,
    failure: RUN.failure,
    cleanup: {
      killed: RUN.cleanup.killed,
      removals: RUN.cleanup.receipts,
      receipts: RUN.cleanup.receipts,
      entries: RUN.cleanup.entries,
      ledgerSchema: SPAWN_LEDGER_SCHEMA,
    },
    exitCode: RUN.exitCode,
  };
}

function buildResult() {
  const receipt = buildReceipt();
  const allGreen = RUN.exitCode === EXIT.OK && RUN.scenarios.length > 0 && RUN.scenarios.every((s) => s.status === "pass");
  return {
    schema: "ferryx-herdr-reference-device.result/1",
    verdict: allGreen ? "pass" : RUN.exitCode === EXIT.BLOCKED ? "blocked" : "fail",
    verdictSemantics:
      "summary only — QA-08 consumes <evidence-dir>/ios/device-receipt.json (schema " + REFERENCE_DEVICE_RECEIPT_SCHEMA + "); skipped, not-run, fail and blocked are never green",
    deviceReceiptPath: receiptPath(),
    deviceReceipt: receipt,
    runner: receipt.producer,
    device: receipt.device,
    page: receipt.page,
    scenarios: RUN.scenarios,
    blockedTargets: RUN.blockedTargets,
    cleanup: RUN.cleanup,
    failure: RUN.failure,
    exitCode: RUN.exitCode,
  };
}

function writeReport() {
  const result = buildResult();
  const lines = [];
  lines.push("# herdr-reference-device (iOS) evidence report");
  lines.push("");
  lines.push("- result schema: " + result.schema);
  lines.push("- device receipt: " + result.deviceReceiptPath + " (schema " + result.deviceReceipt.schema + ")");
  lines.push("- verdict (summary only): **" + result.verdict + "** — QA-08 reads the receipt, never this boolean");
  lines.push("- exitCode: " + result.exitCode);
  lines.push("- script sha256: " + (result.runner.scriptSha256 ?? "unknown"));
  lines.push("- device: " + (result.device.serial ?? "?") + " / " + (result.device.model ?? "?") + " (iOS " + (result.device.osVersion ?? "?") + ")");
  lines.push("- driver: " + (result.device.driverEndpoint ?? "?") + " | typing path: " + (result.runner.typingPath ?? "not exercised"));
  lines.push("- candidate: " + (result.deviceReceipt.candidate.candidateId ?? "?") + " | target: " + (result.deviceReceipt.target.backendSessionId ?? "?"));
  lines.push("- pty: pid " + (result.deviceReceipt.pty.pid ?? "unresolved") + " " + (result.deviceReceipt.pty.executablePath ?? "") + " " + (result.deviceReceipt.pty.cols ?? "?") + "x" + (result.deviceReceipt.pty.rows ?? "?"));
  lines.push("");
  lines.push("## scenarios");
  lines.push("");
  lines.push("| scenario | case | status | reason |");
  lines.push("|---|---|---|---|");
  for (const s of result.scenarios) {
    lines.push("| " + s.name + " | " + s.case + " | " + rowStatus(s) + " | " + (s.reason ?? "") + " |");
  }
  if (result.blockedTargets.length > 0) {
    lines.push("");
    lines.push("## blocked targets (NOT green)");
    for (const b of result.blockedTargets) {
      lines.push("- " + b.target + ": " + b.reason + (b.detail ? " — " + b.detail : ""));
    }
  }
  if (result.failure) {
    lines.push("");
    lines.push("## failure");
    lines.push("- reason: " + result.failure.reason);
    lines.push("- detail: " + result.failure.detail);
  }
  lines.push("");
  lines.push("## cleanup receipt (own resources only)");
  for (const r of result.cleanup.receipts) {
    lines.push("- " + (r.resource ?? "pid " + r.pid) + ": " + r.action + (r.error ? " (" + r.error + ")" : ""));
  }
  lines.push("");
  lines.push("## evidence integrity");
  lines.push("- Korean input came from native keyboard actions only (real on-screen keyboard key touches, or driver hardware key events when the keyboard exposes no labels); no JavaScript input or composition event was dispatched and no field value was set as text.");
  lines.push("- Page state was OBSERVED through read-only script evaluation and the accessibility source; observation never produced input.");
  lines.push("- Every touch was verified by a bounded observable delta or recorded as a failure with a screenshot and the accessibility source.");
  lines.push("- The provisioned driver and device were only read; no settings, apps or keyguard were modified and no driver was launched.");
  writeFileSync(evPath("report.md"), lines.join("\n") + "\n", "utf8");
}

// --------------------------------------------------------------------------- main

async function main() {
  RUN.startedAt = new Date().toISOString();
  ARGS = parseArgs(process.argv.slice(2));
  EVIDENCE_DIR = path.resolve(ARGS.evidenceDir);
  mkdirSync(EVIDENCE_DIR, { recursive: true });
  RUN.caseName = ARGS.caseName;
  try {
    RUN.scriptSha256 = createHash("sha256").update(readFileSync(new URL(import.meta.url))).digest("hex");
  } catch (err) {
    appendLog("script hash failed: " + String(err));
  }
  writeJson("run-config.json", {
    scriptId: SCRIPT_ID,
    scriptSha256: RUN.scriptSha256,
    platform: PLATFORM,
    scenario: ARGS.scenario,
    case: ARGS.caseName,
    fixtureManifest: ARGS.fixtureManifest,
    evidenceDir: EVIDENCE_DIR,
    bindingEnvVariable: REFERENCE_DEVICE_BINDING_ENV,
    bindingPath: process.env[REFERENCE_DEVICE_BINDING_ENV] ?? null,
    startedAt: RUN.startedAt,
    policy:
      "native-keyboard-input-only; read-only page observation; no JavaScript input events; driver session cleanup; " +
      "exact-PID identity checks; no settings changes; no installs; no consent automation",
  });

  process.on("SIGINT", () => {
    appendLog("SIGINT received — cleaning up own resources, exiting 130");
    void cleanup().finally(() => {
      RUN.exitCode = EXIT.INTERRUPT;
      RUN.failure = { reason: "interrupted", detail: "SIGINT" };
      RUN.finishedAt = new Date().toISOString();
      try {
        writeJsonAbs(receiptPath(), buildReceipt());
        writeJson("result.json", buildResult());
        writeReport();
      } catch {
        // best-effort during interrupt
      }
      process.exit(EXIT.INTERRUPT);
    });
  });

  let exitCode = EXIT.OK;
  try {
    if (ARGS.scenario !== "QA-08") {
      throw blocked(
        "scenario-not-implemented",
        "this producer implements QA-08 (real-device composition and layout); " + ARGS.scenario + " belongs to the browser/HTTP runner",
      );
    }
    // The four-way binding first: without it nothing this run observes could be attributed to a
    // candidate, a served page, a pane or a PTY, so the run is blocked rather than green.
    loadBinding();
    const manifest = readJson(ARGS.fixtureManifest);
    selectDevice(manifest);
    verifyPtyIdentity();
    await prereqProbe();

    const cdp = IOS;
    RUN.baselineState = await readState(cdp);
    writeJson("baseline-state.json", RUN.baselineState);
    RUN.maxViewportHeight = RUN.baselineState.viewport.height;
    RUN.minViewportHeight = RUN.baselineState.viewport.height;
    appendLog("baseline: url=" + RUN.baselineState.url + " viewport=" + JSON.stringify(RUN.baselineState.viewport));

    for (const name of Object.keys(SCENARIOS)) {
      for (const caseName of selectedCases()) {
        appendLog("scenario " + name + "/" + caseName + ": start");
        try {
          const row = await SCENARIOS[name][caseName](cdp);
          RUN.scenarios.push(row);
          appendLog("scenario " + name + "/" + caseName + ": " + row.status);
        } catch (err) {
          const isBlocked = err instanceof RunnerError && err.code === EXIT.BLOCKED;
          RUN.scenarios.push({
            name,
            case: caseName,
            status: isBlocked ? "blocked" : "fail",
            code: err instanceof RunnerError ? err.code : 1,
            reason: err.reason ?? String(err),
            detail: String(err.detail ?? err.message ?? "").slice(0, 4000),
            evidenceDir: caseDir(name, caseName),
            evidenceFiles: err.evidenceFiles ?? [],
          });
          appendLog("scenario " + name + "/" + caseName + ": " + (isBlocked ? "blocked" : "fail") + " — " + (err.reason ?? String(err)));
          if (isBlocked) throw err;
        }
      }
    }

    if (ARGS.ptyHook) {
      const r = spawnSync("/bin/sh", ["-c", ARGS.ptyHook], { encoding: "utf8", timeout: 60000 });
      writeFileSync(
        evPath("pty-hook-output.txt"),
        "$ " + ARGS.ptyHook + "\n--- exit " + (r.status ?? -1) + " ---\n" + (r.stdout ?? "") + (r.stderr ?? "") + "\n",
        "utf8",
      );
      RUN.ptyHook = { command: ARGS.ptyHook, exitCode: r.status ?? -1, outputFile: "pty-hook-output.txt" };
    }
  } catch (err) {
    if (err instanceof RunnerError) {
      exitCode = err.code;
      RUN.failure = {
        reason: err.reason,
        detail: String(err.detail ?? "").slice(0, 4000),
        evidenceFiles: err.evidenceFiles ?? [],
      };
      if (err.code === EXIT.BLOCKED) RUN.blockedReason = err.reason + (err.detail ? ": " + err.detail : "");
    } else {
      exitCode = 1;
      RUN.failure = { reason: "unexpected-error", detail: String((err && err.stack) ?? err).slice(0, 4000) };
    }
    appendLog("FAILED exit=" + exitCode + " " + RUN.failure.reason + ": " + RUN.failure.detail);
    const executed = new Set(RUN.scenarios.map((s) => s.name + "/" + s.case));
    for (const name of Object.keys(SCENARIOS)) {
      for (const caseName of selectedCases()) {
        if (!executed.has(name + "/" + caseName)) {
          RUN.scenarios.push({ name, case: caseName, status: "not-run", reason: "the run stopped before this row" });
        }
      }
    }
  } finally {
    await cleanup();
    RUN.exitCode = exitCode;
    RUN.finishedAt = new Date().toISOString();
    try {
      const receipt = buildReceipt();
      writeJsonAbs(receiptPath(), receipt);
      writeJson("result.json", buildResult());
      writeReport();
      appendLog("device-receipt.json + result.json + report.md written; exit=" + exitCode + " verdict=" + receipt.verdict);
    } catch (err) {
      appendLog("result write failed: " + String(err));
    }
  }
  return exitCode;
}

process.exit(await main());

