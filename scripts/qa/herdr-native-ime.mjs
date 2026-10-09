#!/usr/bin/env node
/**
 * herdr-native-ime.mjs — real-device Android IME evidence runner (Ferryx Task 6 native gate).
 *
 * WHAT IT PROVES (when a run passes on an integrated candidate):
 *   Real Korean OSK input on a physical Android device flows into the Ferryx remote terminal:
 *   Hangul appears in the PTY echo EXACTLY ONCE, composition never submits prematurely, and a
 *   line/direct mode or chat/terminal target switch never transfers draft text into the PTY.
 *
 * HOST CONTRACT
 *   Mac-hosted ADB control (any host with `adb` + Node >= 22; global WebSocket is required for
 *   Chrome DevTools Protocol observation). Builds and gateway serving stay on maho-win; the device
 *   browser reaches the isolated gateway through the caller-provided URL (tunnel) and/or
 *   runner-created `adb reverse` mappings. This runner starts NO server.
 *
 * WHAT IT WILL NEVER DO (fail-closed, evidence-integrity policy)
 *   - Never injects text (`adb shell input text`), never sends keyevents as text, never dispatches
 *     JS composition/input events, never fabricates screenshots, transcripts, or coordinates.
 *   - Never changes device or keyboard settings: only READS `getprop` / `settings get` /
 *     `ime list` / `dumpsys` / `pm path`. Installs nothing, enables nothing, never touches
 *     keyguard, never rotates the device.
 *   - Never trusts coordinates: tap targets come from the UI hierarchy, from CDP rects plus a
 *     runtime-derived browser-chrome offset, or from an operator-authored --keymap; EVERY tap is
 *     verified by a bounded, observable DOM delta before the run proceeds. No delta ⇒ recorded
 *     failure with screenshots + hierarchy — never a silent pass.
 *   - Never removes adb forwards/reverses it did not create itself (exact-id tracking + receipt).
 *   - Never claims "native QA green" for anything it did not observe on this device.
 *   - Never accepts Chrome first-run/consent screens — fails closed as
 *     `device-owner-acknowledgment-needed` (automation can never acknowledge for the owner).
 *
 * REAL INPUT PATH
 *   Physical touch (`adb shell input tap`) strikes the REAL on-screen keyboard (Samsung HoneyBoard
 *   on the confirmed device R3CN8126R4Y / SM-N981N) whose real IME pipeline drives real
 *   composition events inside Chrome. Observation is read-only: CDP Runtime.evaluate reads DOM
 *   state, `uiautomator dump` reads the view hierarchy, `screencap` captures pixels.
 *
 * EVIDENCE LAYOUT (under --evidence-dir)
 *   run-config.json, runner.log, result.json, report.md,
 *   prereq/{provenance.json, device-props.txt, ime.txt, chrome.txt, korean-layout-probe.json,
 *     chrome-launch.txt, cdp-socket-probe.json, cdp-blocked.png, proc-net-unix-at-block.txt},
 *   <scenario>/{screenshots, hierarchy XML, JSON state captures, assertions.json}
 *
 * EXIT CODES
 *   0 all selected scenarios passed (a skipped scenario requires an honest recorded reason)
 *   2 prerequisite blocked   3 interaction failure   4 assertion failure
 *   5 usage error            130 interrupted (cleanup still runs)
 *
 * USAGE (canonical, Mac host)
 *   node scripts/qa/herdr-native-ime.mjs \
 *     --serial R3CN8126R4Y \
 *     --evidence-dir /Users/.../evidence/task-6-android \
 *     --page-url http://<isolated-gateway>/<remote-path> \
 *     [--adb /opt/homebrew/bin/adb] [--adb-reverse 8899:8899]... [--keymap <file.json>] \
 *     [--tap-offset-y <px>] [--device-set android] \
 *     [--scenarios direct-once,mode-switch,composition-enter,target-switch] \
 *     [--timeout-ms 15000] [--pty-hook "<command>"]
 *
 * KEYMAP JSON — only needed when the IME window does not expose key labels to uiautomator:
 *   { "imeFraction": { "ㅎ": [0.10, 0.12], "...": [0, 0], "enter": [0.95, 0.90] },
 *     "tapOffsetY": 250 }
 *   imeFraction entries are fractions of the DISCOVERED IME-window bounds (never absolute
 *   screen guesses); tapOffsetY is the browser-chrome y offset for page taps when hierarchy
 *   discovery is unavailable. Author it once from a real focused-keyboard capture at final QA.
 */

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";

const SCRIPT_ID = "herdr-native-ime.mjs/1.1.0";
const EXIT = { OK: 0, BLOCKED: 2, INTERACTION: 3, ASSERTION: 4, USAGE: 5, INTERRUPT: 130 };
const DUPLICATE_WINDOW_MS = 700; // settle window used ONLY to detect a second, late echo
const EXPECTED_IME = "honeyboard"; // confirmed device profile: Samsung HoneyBoard
const JAMO = new Set(
  ("ㄱㄲㄳㄴㄵㄶㄷㄸㄹㄺㄻㄼㄽㄾㄿㅀㅁㅂㅃㅄㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ" +
    "ㅏㅐㅑㅒㅓㅔㅕㅖㅗㅘㅙㅚㅛㅜㅝㅞㅟㅠㅡㅢㅣ").split(""),
);

// --------------------------------------------------------------------------- state

let ARGS = null;
let ADB = null;
let SERIAL = null;
let EVIDENCE_DIR = null;

const RUN = {
  startedAt: null,
  finishedAt: null,
  scriptId: SCRIPT_ID,
  scriptSha256: null,
  host: { platform: process.platform, release: os.release(), node: process.version },
  adbPath: null,
  adbVersion: null,
  serial: null,
  deviceSet: null,
  scenarios: [],
  blockedTargets: [],
  cdp: null,
  chromeOffsets: [],
  cleanup: { createdForwards: [], createdReverses: [], removals: [] },
  ptyHook: null,
  failure: null,
  exitCode: null,
};

class RunnerError extends Error {
  constructor(code, reason, detail) {
    super(`${reason}: ${detail ?? ""}`.trim());
    this.code = code;
    this.reason = reason;
    this.detail = detail ?? "";
  }
}
const blocked = (reason, detail) => new RunnerError(EXIT.BLOCKED, reason, detail);
const interactionFailed = (reason, detail) => new RunnerError(EXIT.INTERACTION, reason, detail);
const assertionFailed = (reason, detail) => new RunnerError(EXIT.ASSERTION, reason, detail);

// --------------------------------------------------------------------------- CLI

function usage(message) {
  process.stderr.write(
    `USAGE ERROR: ${message}\n\n` +
      `  node scripts/qa/herdr-native-ime.mjs --serial <device-serial> --evidence-dir <dir>\n` +
      `      --page-url <url> [--adb <path>] [--adb-reverse dev:host]... [--keymap <file>]\n` +
      `      [--tap-offset-y <px>] [--device-set android] [--scenarios a,b,c]\n` +
      `      [--timeout-ms <ms>] [--pty-hook <command>]\n`,
  );
  process.exit(EXIT.USAGE);
}

function parseArgs(argv) {
  const args = {
    serial: null,
    evidenceDir: null,
    pageUrl: null,
    adb: process.env.FERRYX_ADB || null,
    adbReverse: [],
    keymap: null,
    tapOffsetY: 0,
    scenarios: ["direct-once", "mode-switch", "composition-enter", "target-switch"],
    timeoutMs: 15000,
    ptyHook: null,
    deviceSet: "android",
  };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    const next = () => {
      i += 1;
      if (i >= argv.length) usage(`${flag} requires a value`);
      return argv[i];
    };
    switch (flag) {
      case "--serial": args.serial = next(); break;
      case "--evidence-dir": args.evidenceDir = next(); break;
      case "--page-url": args.pageUrl = next(); break;
      case "--adb": args.adb = next(); break;
      case "--adb-reverse": args.adbReverse.push(next()); break;
      case "--keymap": args.keymap = next(); break;
      case "--tap-offset-y": args.tapOffsetY = Number.parseInt(next(), 10); break;
      case "--device-set": args.deviceSet = next(); break;
      case "--scenarios":
        args.scenarios = next().split(",").map((s) => s.trim()).filter(Boolean);
        break;
      case "--timeout-ms": args.timeoutMs = Number.parseInt(next(), 10); break;
      case "--pty-hook": args.ptyHook = next(); break;
      case "-h": case "--help":
        process.stdout.write(readFileSync(new URL(import.meta.url), "utf8").split("*/")[0] + "*/\n");
        process.exit(EXIT.OK);
        break;
      default:
        usage(`unknown flag ${flag}`);
    }
  }
  if (!args.serial) usage("--serial is mandatory (exact physical device serial; no wildcard default)");
  if (!args.evidenceDir) usage("--evidence-dir is mandatory");
  if (!args.pageUrl) usage("--page-url is mandatory (isolated gateway URL reachable from the device)");
  if (!Number.isFinite(args.timeoutMs) || args.timeoutMs <= 0) usage("--timeout-ms must be a positive integer");
  if (!Number.isFinite(args.tapOffsetY)) usage("--tap-offset-y must be an integer");
  return args;
}

// --------------------------------------------------------------------------- exec / adb

function run(cmd, args, opts = {}) {
  const res = spawnSync(cmd, args, {
    encoding: opts.binary ? undefined : "utf8",
    maxBuffer: 64 * 1024 * 1024,
    timeout: opts.timeoutMs ?? 30000,
    killSignal: "SIGKILL",
  });
  if (res.error) throw blocked("exec-failed", `${cmd} ${args.join(" ")} -> ${res.error.message}`);
  return {
    code: res.status ?? -1,
    stdout: res.stdout ?? "",
    stderr: (res.stderr ?? "").toString(),
    buffer: res.stdout,
  };
}

function adb(args, opts = {}) {
  const prefixed = ["-s", SERIAL, ...args];
  const res = run(ADB, prefixed, opts);
  if (opts.expectCodeZero !== false && res.code !== 0) {
    throw interactionFailed(
      "adb-command-failed",
      `${prefixed.join(" ")} -> exit ${res.code}: ${res.stderr.slice(0, 400)}`,
    );
  }
  return res;
}

function adbRaw(args, opts = {}) {
  const res = run(ADB, args, opts);
  if (res.code !== 0) {
    throw blocked("adb-command-failed", `${args.join(" ")} -> exit ${res.code}: ${res.stderr.slice(0, 400)}`);
  }
  return res;
}

// --------------------------------------------------------------------------- evidence

function evPath(...parts) {
  const p = path.join(EVIDENCE_DIR, ...parts);
  mkdirSync(path.dirname(p), { recursive: true });
  return p;
}

function writeJson(relPath, data) {
  const p = evPath(relPath);
  writeFileSync(p, JSON.stringify(data, null, 2) + "\n", "utf8");
  return p;
}

function screenshot(name) {
  const p = evPath(name);
  const res = adb(["exec-out", "screencap", "-p"], { binary: true });
  if (res.code !== 0 || !res.buffer || res.buffer.length < 100) {
    throw interactionFailed("screencap-failed", `${name}: exit ${res.code}, ${res.buffer?.length ?? 0} bytes`);
  }
  writeFileSync(p, res.buffer);
  return p;
}

function appendLog(line) {
  try {
    appendFileSync(evPath("runner.log"), `[${new Date().toISOString()}] ${line}\n`, "utf8");
  } catch {
    // best-effort logging; never masks the real failure
  }
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function poll(label, fn, { timeoutMs, intervalMs = 400 } = {}) {
  const budget = timeoutMs ?? ARGS.timeoutMs;
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
      throw interactionFailed("bounded-poll-timeout", `${label} (budget ${budget}ms; last: ${lastErr?.message})`);
    }
    await delay(intervalMs);
  }
}

// --------------------------------------------------------------------------- UI hierarchy

function fetchHierarchy() {
  // Contract: `adb exec-out uiautomator dump /dev/tty` prints XML prefixed by the upstream
  // "UI hierchary dumped to: /dev/tty" line; strip everything before the first "<".
  // Fallback: dump to /sdcard then cat it back; the temp file is our own and is removed.
  let out = adb(["exec-out", "uiautomator", "dump", "/dev/tty"], { expectCodeZero: false });
  let xml = (out.stdout ?? "").toString();
  let start = xml.indexOf("<");
  if (out.code !== 0 || start < 0) {
    const remote = `/sdcard/ferryx-qa-hierarchy-${Date.now()}.xml`;
    const dump = adb(["shell", "uiautomator", "dump", remote], { expectCodeZero: false });
    if (dump.code !== 0) {
      throw interactionFailed(
        "uiautomator-dump-failed",
        `exec-out exit ${out.code} ${out.stderr?.slice(0, 200) ?? ""} | shell ${dump.stderr?.slice(0, 200) ?? ""}`,
      );
    }
    const cat = adb(["exec-out", "cat", remote]);
    adb(["shell", "rm", "-f", remote]);
    xml = cat.stdout.toString();
    start = xml.indexOf("<");
    if (start < 0) throw interactionFailed("uiautomator-dump-empty", "no XML in fallback dump");
  }
  return parseHierarchyXml(xml.slice(start));
}

function parseHierarchyXml(xml) {
  const nodes = [];
  const nodeRe = /<node\b([^>]*?)(?:\/>|>)/g;
  let m;
  while ((m = nodeRe.exec(xml)) !== null) {
    const attrs = {};
    const attrRe = /([\w:-]+)="([^"]*)"/g;
    let a;
    while ((a = attrRe.exec(m[1])) !== null) attrs[a[1]] = decodeXml(a[2]);
    const b = /\[(\d+),(\d+)\]\[(\d+),(\d+)\]/.exec(attrs.bounds ?? "");
    if (!b) continue;
    const node = {
      text: attrs.text ?? "",
      desc: attrs["content-desc"] ?? "",
      cls: attrs["class"] ?? "",
      pkg: attrs.package ?? "",
      res: attrs["resource-id"] ?? "",
      clickable: attrs.clickable === "true",
      bounds: {
        x1: Number.parseInt(b[1], 10),
        y1: Number.parseInt(b[2], 10),
        x2: Number.parseInt(b[3], 10),
        y2: Number.parseInt(b[4], 10),
      },
    };
    node.cx = Math.round((node.bounds.x1 + node.bounds.x2) / 2);
    node.cy = Math.round((node.bounds.y1 + node.bounds.y2) / 2);
    node.w = node.bounds.x2 - node.bounds.x1;
    node.h = node.bounds.y2 - node.bounds.y1;
    nodes.push(node);
  }
  if (nodes.length === 0) throw interactionFailed("hierarchy-parse-empty", "no <node> entries with bounds");
  return { xml, nodes };
}

function decodeXml(s) {
  return s
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&amp;/g, "&");
}

function saveHierarchy(relDir, name, hierarchy) {
  const p = evPath(relDir, `${name}.xml`);
  writeFileSync(p, hierarchy.xml, "utf8");
  return p;
}

function imeRootBounds(nodes) {
  const ime = nodes.filter(
    (n) =>
      n.pkg.includes(EXPECTED_IME) ||
      n.pkg.includes("inputmethod") ||
      /InputMethod/i.test(n.cls),
  );
  if (ime.length === 0) return null;
  return ime.reduce((a, b) => (b.w * b.h > a.w * a.h ? b : a)).bounds;
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
  for (const label of ["enter", "return", "go", "send", "action", "줄바꿈", "다음"]) {
    const hit = pool.find(
      (n) => n.desc.toLowerCase() === label || n.text.toLowerCase() === label,
    );
    if (hit) return hit;
  }
  return pool.find((n) => /enter|action|done|go/i.test(n.res) && n.clickable) ?? null;
}

// --------------------------------------------------------------------------- taps (real touch + verification)

async function tapPoint(label, x, y, verify, { screenshotOnFail = null } = {}) {
  adb(["shell", "input", "tap", String(Math.round(x)), String(Math.round(y))]);
  appendLog(`tap ${label} @ (${Math.round(x)},${Math.round(y)})`);
  try {
    await poll(`tap-verify:${label}`, verify, {});
  } catch (err) {
    if (screenshotOnFail) screenshot(screenshotOnFail);
    throw err;
  }
}

function rectExpr(selector) {
  return (
    `(() => { const el = document.querySelector('${selector}');` +
    ` if (!el) return null; const r = el.getBoundingClientRect();` +
    ` return { x: r.x, y: r.y, w: r.width, h: r.height }; })()`
  );
}

/**
 * Page element -> real tap point.
 *  1) hierarchy text/content-desc match (Chrome exposes button/input text),
 *  2) else CDP rect * devicePixelRatio + a browser-chrome y offset DERIVED at runtime by
 *     comparing a hierarchy-exposed element against its CDP rect, or supplied explicitly
 *     via --tap-offset-y / keymap.tapOffsetY.
 * Candidates are never trusted: tapPoint() always verifies a bounded DOM delta.
 */
async function resolvePageTap(cdp, nodes, hint) {
  const hier = nodes.find(
    (n) => (hint.text && n.text === hint.text) || (hint.desc && n.desc === hint.desc),
  );
  if (hier) return { x: hier.cx, y: hier.cy, via: "hierarchy" };

  const rect = await cdp.evaluate(hint.cdpRectExpr);
  if (!rect || !Number.isFinite(rect.x)) {
    throw interactionFailed("tap-target-undiscoverable", `${hint.hint}: no hierarchy node, empty CDP rect`);
  }
  const scale = (await cdp.evaluate("window.devicePixelRatio || 1")) || 1;
  const offset = await deriveChromeOffset(cdp, nodes, scale);
  const x = Math.round((rect.x + rect.w / 2) * scale);
  const y = Math.round((rect.y + rect.h / 2) * scale + offset);
  if (x <= 0 || y <= 0) {
    throw interactionFailed("tap-point-invalid", `${hint.hint}: computed (${x},${y})`);
  }
  return { x, y, via: `cdp-rect+chrome-offset(${offset}px)` };
}

/**
 * Physical-pixel y offset that maps a CDP viewport rect onto this device's screen coordinates
 * (browser chrome + viewport origin shift). Derived LIVE from a hierarchy-visible Chrome
 * element matched by its text in-page — never a hardcoded constant. If no probe can be matched,
 * an explicit --tap-offset-y / keymap.tapOffsetY is required; otherwise this fails closed with a
 * precise remedy instead of guessing coordinates.
 */
async function deriveChromeOffset(cdp, nodes, scale) {
  const probeExpr = (text) =>
    `(() => {\n` +
    `    const t = ${JSON.stringify(text)};\n` +
    `    const els = [...document.querySelectorAll('button,a,[role="button"],[role="switch"],label,textarea,input,span,div')];\n` +
    `    let best = null, bestArea = Infinity;\n` +
    `    for (const el of els) {\n` +
    `      if ((el.textContent || '').trim() !== t) continue;\n` +
    `      const r = el.getBoundingClientRect();\n` +
    `      const area = r.width * r.height;\n` +
    `      if (r.width > 0 && r.height > 0 && area < bestArea) { bestArea = area; best = r; }\n` +
    `    }\n` +
    `    if (!best) return null;\n` +
    `    return { x: best.x, y: best.y, w: best.width, h: best.height };\n` +
    `  })()`;
  const probes = nodes
    .filter((n) => n.pkg.includes("chrome") && n.text && n.text.length <= 40 && n.w > 0 && n.h > 0)
    .slice(0, 12);
  for (const n of probes) {
    const rect = await cdp.evaluate(probeExpr(n.text));
    if (rect && Number.isFinite(rect.y) && rect.h > 0) {
      const offset = Math.round(n.cy - (rect.y + rect.h / 2) * scale);
      RUN.chromeOffsets.push({ probeText: n.text, hierarchyCy: n.cy, cdpRect: rect, scale, offsetPx: offset });
      appendLog(`chrome offset derived from probe "${n.text}": ${offset}px`);
      return offset;
    }
  }
  const override = ARGS.tapOffsetY || KEYMAP?.tapOffsetY || 0;
  if (override > 0) {
    RUN.chromeOffsets.push({ probeText: null, offsetPx: override, source: "explicit" });
    return override;
  }
  throw interactionFailed(
    "browser-chrome-offset-undiscoverable",
    "no hierarchy-visible Chrome element matched in-page; supply --tap-offset-y <physical px> (or keymap.tapOffsetY) captured from a real focused page — never guess",
  );
}

// --------------------------------------------------------------------------- keymap

let KEYMAP = null;

function loadKeymap(file) {
  if (!file) return null;
  let raw;
  try {
    raw = JSON.parse(readFileSync(file, "utf8"));
  } catch (err) {
    throw blocked("keymap-unreadable", `${file}: ${err.message}`);
  }
  if (!raw || typeof raw.imeFraction !== "object" || raw.imeFraction === null) {
    throw blocked("keymap-invalid", `${file}: missing imeFraction object {label: [xFraction, yFraction]}`);
  }
  for (const [k, v] of Object.entries(raw.imeFraction)) {
    if (!Array.isArray(v) || v.length !== 2 || v.some((n) => typeof n !== "number" || n < 0 || n > 1)) {
      throw blocked("keymap-invalid", `${file}: imeFraction["${k}"] must be [0..1, 0..1] fractions of the discovered IME bounds`);
    }
  }
  if (raw.tapOffsetY !== undefined && !Number.isFinite(raw.tapOffsetY)) {
    throw blocked("keymap-invalid", `${file}: tapOffsetY must be a number (physical px)`);
  }
  appendLog(`keymap loaded: ${file} (${Object.keys(raw.imeFraction).length} entries)`);
  return raw;
}

// --------------------------------------------------------------------------- IME keys (real touch)

async function resolveImeKey(evDir, label) {
  const hierarchy = fetchHierarchy();
  const node = findImeKey(hierarchy.nodes, label);
  if (node) return { x: node.cx, y: node.cy, via: "hierarchy", hierarchy };
  const bounds = imeRootBounds(hierarchy.nodes);
  const frac = KEYMAP?.imeFraction?.[label];
  if (bounds && Array.isArray(frac)) {
    const x = bounds.x1 + (bounds.x2 - bounds.x1) * frac[0];
    const y = bounds.y1 + (bounds.y2 - bounds.y1) * frac[1];
    return { x, y, via: `keymap-fraction[${frac}]`, hierarchy };
  }
  saveHierarchy(evDir, "ime-key-not-discoverable", hierarchy);
  screenshot(path.join(evDir, "ime-key-not-discoverable.png"));
  throw blocked(
    "ime-key-not-discoverable",
    `label "${label}" not exposed by the IME hierarchy and no keymap entry; author the keymap from a real focused-keyboard capture (--keymap) — never guess coordinates`,
  );
}

async function resolveImeEnter(evDir) {
  const hierarchy = fetchHierarchy();
  const node = findImeEnter(hierarchy.nodes);
  if (node) return { x: node.cx, y: node.cy, via: "hierarchy", hierarchy };
  const bounds = imeRootBounds(hierarchy.nodes);
  const frac = KEYMAP?.imeFraction?.enter;
  if (bounds && Array.isArray(frac)) {
    const x = bounds.x1 + (bounds.x2 - bounds.x1) * frac[0];
    const y = bounds.y1 + (bounds.y2 - bounds.y1) * frac[1];
    return { x, y, via: `keymap-fraction[${frac}]`, hierarchy };
  }
  saveHierarchy(evDir, "ime-enter-not-discoverable", hierarchy);
  screenshot(path.join(evDir, "ime-enter-not-discoverable.png"));
  throw blocked(
    "ime-enter-not-discoverable",
    "IME enter/return key not discoverable; supply keymap.imeFraction.enter from a real capture — never guess",
  );
}

// --------------------------------------------------------------------------- CDP client (read-only)

/**
 * Minimal Chrome DevTools Protocol client. This runner only ever issues READ-ONLY
 * Runtime.evaluate calls (state capture, rects); it never injects input, never mutates
 * page state, and ignores all CDP events.
 */
class CdpClient {
  constructor(ws) {
    this.ws = ws;
    this.seq = 0;
    this.pending = new Map();
    ws.addEventListener("message", (event) => {
      let msg;
      try {
        msg = JSON.parse(typeof event.data === "string" ? event.data : event.data.toString());
      } catch {
        return;
      }
      if (!msg.id) return; // events are ignored by design
      const waiter = this.pending.get(msg.id);
      if (!waiter) return;
      this.pending.delete(msg.id);
      waiter.clear();
      if (msg.error) waiter.reject(new Error(`CDP error: ${msg.error.message}`));
      else waiter.resolve(msg.result);
    });
    ws.addEventListener("close", () => {
      for (const waiter of this.pending.values()) {
        waiter.clear();
        waiter.reject(new Error("CDP socket closed"));
      }
      this.pending.clear();
    });
  }

  send(method, params = {}) {
    const id = ++this.seq;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(interactionFailed("cdp-timeout", method));
      }, ARGS.timeoutMs);
      this.pending.set(id, {
        clear: () => clearTimeout(timer),
        resolve,
        reject,
      });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }

  /** READ-ONLY evaluation contract: the expression must not mutate page state. */
  async evaluate(expression) {
    const res = await this.send("Runtime.evaluate", {
      expression,
      returnByValue: true,
      awaitPromise: true,
      userGesture: false,
    });
    if (res.exceptionDetails) {
      const detail = res.exceptionDetails;
      throw interactionFailed("cdp-evaluate-exception", detail.exception?.description ?? detail.text ?? "unknown");
    }
    return res.result?.value;
  }

  close() {
    try {
      this.ws.close();
    } catch {
      // closing a dead socket is fine
    }
  }
}

/**
 * Bounded post-launch probe for Chrome's `chrome_devtools_remote` abstract socket. Chrome's
 * process start races the `am start` intent, and a FirstRunActivity consent screen prevents the
 * browser from ever coming up — first-run fails fast (never clicked/accepted by automation).
 * Pure data collection: the caller writes evidence files for whichever outcome occurred.
 */
async function waitForDevtoolsSocket({ timeoutMs, launchOutput }) {
  const startedAt = Date.now();
  const attempts = [];
  const seen = new Set();
  const launchFirstRun = /firstrun/i.test(launchOutput ?? "");
  let focus = null;
  let firstRunDetected = launchFirstRun;
  for (;;) {
    let unix = "";
    try {
      unix = adb(["exec-out", "cat", "/proc/net/unix"]).stdout ?? "";
    } catch (err) {
      attempts.push({ atMs: Date.now() - startedAt, error: String(err).slice(0, 200) });
    }
    for (const line of unix.split("\n")) {
      const idx = line.lastIndexOf(" ");
      if (idx < 0) continue;
      const socketPath = line.slice(idx + 1).trim();
      if (socketPath.includes("_devtools_remote")) seen.add(socketPath.replace(/^@/, ""));
    }
    focus = readWindowFocus();
    if (isFirstRunFocus(focus)) firstRunDetected = true;
    attempts.push({ atMs: Date.now() - startedAt, socketsSeen: seen.size, focus, firstRunDetected });
    if (seen.size > 0 && !firstRunDetected) {
      return {
        socketPaths: [...seen].sort(),
        attempts,
        elapsedMs: Date.now() - startedAt,
        focus,
        firstRunDetected: false,
        firstRunFromLaunch: launchFirstRun,
      };
    }
    if (firstRunDetected) break;
    if (Date.now() - startedAt >= timeoutMs) break;
    await delay(500);
  }
  return {
    socketPaths: [...seen].sort(),
    attempts,
    elapsedMs: Date.now() - startedAt,
    focus,
    firstRunDetected,
    firstRunFromLaunch: launchFirstRun,
  };
}

function readWindowFocus() {
  try {
    const out = adb(["shell", "dumpsys", "window", "displays"]).stdout ?? "";
    return /mCurrentFocus=([^\n]+)/.exec(out)?.[1]?.trim() ?? null;
  } catch (err) {
    return `focus-read-failed: ${String(err).slice(0, 200)}`;
  }
}

function isFirstRunFocus(focus) {
  if (!focus) return false;
  return /firstrun|first-run|first run|chromewelcome/i.test(focus);
}

function tryScreenshotEvidence(name) {
  try {
    return { path: screenshot(name) };
  } catch (err) {
    return { error: String(err).slice(0, 300) };
  }
}

/**
 * Discover the real Chrome DevTools socket on the device (bounded wait first — see
 * waitForDevtoolsSocket — then /proc/net/unix, abstract namespace, name ends in
 * _devtools_remote, the Stetho-documented approach), create ONE adb forward on a randomly
 * chosen free host port (tracked for exact removal in cleanup), find (or open) a page target
 * on the isolated gateway origin, and connect a read-only CDP client. First-run consent and
 * missing-socket outcomes both write exact evidence files and surface them on the error.
 */
async function connectCdp({ launchOutput = "", launchExit = null } = {}) {
  if (typeof WebSocket === "undefined") {
    throw blocked("websocket-unsupported", "global WebSocket missing — Node >= 22 required on this host");
  }
  const probe = await waitForDevtoolsSocket({ timeoutMs: ARGS.timeoutMs, launchOutput });

  if (probe.firstRunDetected) {
    // Fail-closed blockedTarget: automation never accepts a consent screen for the owner.
    const shot = tryScreenshotEvidence("prereq/cdp-blocked.png");
    writeJson("prereq/cdp-socket-probe.json", {
      blocked: "chrome-first-run",
      reasonCode: "device-owner-acknowledgment-needed",
      launchExit,
      launchOutput: String(launchOutput).slice(0, 4000),
      firstRunFromLaunch: probe.firstRunFromLaunch,
      attempts: probe.attempts,
      elapsedMs: probe.elapsedMs,
      timeoutMs: ARGS.timeoutMs,
      focus: probe.focus,
      socketsSeen: probe.socketPaths,
      screenshot: shot,
    });
    const evidenceFiles = [
      "prereq/chrome-launch.txt",
      "prereq/cdp-socket-probe.json",
      ...(shot.path ? ["prereq/cdp-blocked.png"] : []),
    ];
    const detail =
      `Chrome FirstRunActivity/consent screen is showing (focus=${probe.focus}); ` +
      "the runner never accepts consent on the owner's behalf — acknowledge Chrome's first-run " +
      "on the device manually, then re-run";
    RUN.blockedTargets.push({
      target: "chrome-first-run-consent",
      reason: "device-owner-acknowledgment-needed",
      detail,
      evidenceFiles,
    });
    const err = blocked("device-owner-acknowledgment-needed", detail);
    err.evidenceFiles = evidenceFiles;
    throw err;
  }

  if (probe.socketPaths.length === 0) {
    const shot = tryScreenshotEvidence("prereq/cdp-blocked.png");
    let unixNow = "";
    try {
      unixNow = adb(["exec-out", "cat", "/proc/net/unix"]).stdout ?? "";
    } catch (err) {
      unixNow = `read-failed: ${String(err).slice(0, 300)}`;
    }
    writeFileSync(evPath("prereq/proc-net-unix-at-block.txt"), unixNow, "utf8");
    writeJson("prereq/cdp-socket-probe.json", {
      blocked: "chrome-devtools-socket",
      reasonCode: "cdp-socket-not-found",
      launchExit,
      launchOutput: String(launchOutput).slice(0, 4000),
      attempts: probe.attempts,
      elapsedMs: probe.elapsedMs,
      timeoutMs: ARGS.timeoutMs,
      focus: probe.focus,
      socketsSeen: [],
      screenshot: shot,
    });
    const evidenceFiles = [
      "prereq/chrome-launch.txt",
      "prereq/cdp-socket-probe.json",
      "prereq/proc-net-unix-at-block.txt",
      ...(shot.path ? ["prereq/cdp-blocked.png"] : []),
    ];
    const detail =
      `no *_devtools_remote socket after bounded wait ${probe.elapsedMs}ms ` +
      `(${probe.attempts.length} attempts, last focus=${probe.focus})`;
    RUN.blockedTargets.push({
      target: "chrome-devtools-socket",
      reason: "cdp-socket-not-found",
      detail,
      evidenceFiles,
    });
    const err = blocked("cdp-socket-not-found", detail);
    err.evidenceFiles = evidenceFiles;
    throw err;
  }

  const sorted = probe.socketPaths;
  const socketPath =
    sorted.find((p) => /(^|\/)chrome_devtools_remote$/.test(p)) ??
    sorted.find((p) => !/webview/i.test(p)) ??
    sorted[0];

  let localPort = null;
  let lastErr = "";
  for (let attempt = 0; attempt < 10 && localPort === null; attempt += 1) {
    const port = 18000 + Math.floor(Math.random() * 4000);
    const spec = `tcp:${port}`;
    const res = adb(["forward", spec, `localabstract:${socketPath}`], { expectCodeZero: false });
    if (res.code === 0) {
      localPort = port;
      RUN.cleanup.createdForwards.push(spec);
      appendLog(`adb forward created: ${spec} -> localabstract:${socketPath}`);
    } else {
      lastErr = res.stderr.slice(0, 200);
    }
  }
  if (localPort === null) {
    throw blocked("cdp-forward-failed", `could not create any adb forward (10 attempts): ${lastErr}`);
  }

  const origin = new URL(ARGS.pageUrl).origin;
  const listTargets = async () => {
    const res = await fetch(`http://127.0.0.1:${localPort}/json/list`);
    const list = await res.json();
    return Array.isArray(list) ? list : [];
  };

  let targets = [];
  try {
    targets = await listTargets();
  } catch (err) {
    throw interactionFailed("cdp-list-failed", String(err));
  }
  let target = targets.find((t) => t.type === "page" && t.url && t.url.startsWith(origin));
  if (!target) {
    // The requested page is not open yet: open it (operational necessity, recorded as evidence).
    try {
      const created = await fetch(
        `http://127.0.0.1:${localPort}/json/new?${encodeURIComponent(ARGS.pageUrl)}`,
        { method: "PUT" },
      );
      if (created.ok) await created.json();
      appendLog("cdp: opened requested page via /json/new");
    } catch (err) {
      appendLog(`cdp: /json/new failed: ${String(err)}`);
    }
    targets = await listTargets().catch(() => []);
    target = targets.find((t) => t.type === "page" && t.url && t.url.startsWith(origin));
  }
  if (!target || !target.webSocketDebuggerUrl) {
    throw blocked(
      "cdp-target-not-found",
      `no page target on origin ${origin}; targets seen: ${targets.map((t) => t.url).join(" | ").slice(0, 400)}`,
    );
  }

  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(interactionFailed("cdp-connect-timeout", target.webSocketDebuggerUrl)), ARGS.timeoutMs);
    ws.addEventListener("open", () => {
      clearTimeout(timer);
      resolve();
    });
    ws.addEventListener("error", () => {
      clearTimeout(timer);
      reject(interactionFailed("cdp-connect-failed", target.webSocketDebuggerUrl));
    });
  });
  const client = new CdpClient(ws);
  RUN.cdp = {
    socketPath,
    localPort,
    forwardSpec: `tcp:${localPort}`,
    targetId: target.id ?? null,
    targetUrl: target.url ?? null,
  };
  appendLog(`CDP connected: ${socketPath} -> 127.0.0.1:${localPort} target=${target.url}`);
  return client;
}

// --------------------------------------------------------------------------- page state (read-only capture)

const READ_STATE_EXPRESSION = `(() => {
  const q = (s) => document.querySelector(s);
  const grid = q('[data-testid="remote-terminal-grid"]');
  const lineEl = q('[data-testid="remote-terminal-line-input"]');
  const toggle = q('[data-testid="remote-terminal-input-mode-toggle"]');
  const preeditEl = q('[data-testid="remote-terminal-preedit"]');
  const chatEl = q('[data-testid="chat-composer-textarea"]') || q('[data-testid="chat-composer"]');
  const termBtn = q('[data-testid="remote-view-mode-terminal"]');
  const chatBtn = q('[data-testid="remote-view-mode-chat"]');
  const ae = document.activeElement;
  const visible = (el) => !!el && !!(el.offsetWidth || el.offsetHeight || el.getClientRects().length);
  const gridText = grid ? grid.textContent : null;
  return {
    gridText: gridText,
    gridLines: gridText ? gridText.split('\\n').filter((l) => l.length > 0) : [],
    preedit: preeditEl ? preeditEl.textContent : null,
    mode: toggle ? (toggle.getAttribute('data-mode') || (lineEl && visible(lineEl) ? 'line' : 'direct'))
                 : (lineEl && visible(lineEl) ? 'line' : null),
    toggleText: toggle ? (toggle.textContent || '').trim() : null,
    toggleAriaChecked: toggle ? toggle.getAttribute('aria-checked') : null,
    lineDraft: lineEl ? lineEl.value : null,
    lineVisible: visible(lineEl),
    chatDraft: chatEl ? chatEl.value : null,
    chatVisible: visible(chatEl),
    viewModeButtons: { terminalVisible: visible(termBtn), chatVisible: visible(chatBtn) },
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

async function readState(cdp) {
  const state = await cdp.evaluate(READ_STATE_EXPRESSION);
  if (!state || typeof state !== "object") {
    throw interactionFailed("read-state-failed", JSON.stringify(state)?.slice(0, 200) ?? String(state));
  }
  return state;
}

function countIn(haystack, needle) {
  if (!haystack || !needle) return 0;
  return haystack.split(needle).length - 1;
}

// --------------------------------------------------------------------------- interaction helpers

/** Ensure the REAL system keyboard (HoneyBoard) is on screen; tap-to-focus is verified by a
 *  bounded focus delta, the keyboard itself is verified by the UI hierarchy. Returns the
 *  hierarchy captured while the keyboard is visible. */
async function ensureKeyboardOpen(cdp, evDir, hint) {
  try {
    const h = fetchHierarchy();
    if (imeRootBounds(h.nodes)) return h;
  } catch {
    // hierarchy unavailable — proceed to the tap flow
  }
  let nodes = [];
  try {
    nodes = fetchHierarchy().nodes;
  } catch {
    // empty probe set; resolvePageTap will fall back to CDP rect + chrome offset
  }
  const tap = await resolvePageTap(cdp, nodes, hint);
  await tapPoint(`focus:${hint.hint}`, tap.x, tap.y, async () => {
    const s = await readState(cdp);
    if (hint.focusTestid) return s.activeElement?.testid === hint.focusTestid;
    return !!s.activeElement && s.activeElement.tag !== "BODY";
  }, { screenshotOnFail: path.join(evDir, `focus-${hint.hint}-failed.png`) });
  return poll(`keyboard-visible:${hint.hint}`, () => {
    try {
      const h = fetchHierarchy();
      return imeRootBounds(h.nodes) ? h : null;
    } catch {
      return null;
    }
  }, {});
}

/** Fail-closed Korean layout verification against the REAL focused keyboard:
 *  the probe jamo must be exposed by the IME hierarchy. Never switches layouts. */
function requireKoreanLayout(evDir, hierarchy) {
  const bounds = imeRootBounds(hierarchy.nodes);
  if (!bounds) throw blocked("ime-window-missing", "no IME window in the captured hierarchy");
  const labels = new Set();
  for (const n of hierarchy.nodes) {
    if (!within(bounds, n)) continue;
    for (const ch of `${n.text}${n.desc}`) {
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
  if (missing.length > 0) {
    screenshot(path.join(evDir, "korean-layout-not-active.png"));
    saveHierarchy(evDir, "korean-layout-not-active", hierarchy);
    throw blocked(
      "korean-layout-inactive",
      `probe jamo not exposed by the real keyboard: ${missing.join(", ")}; enable the Korean layout manually on the device — this runner never changes keyboard settings`,
    );
  }
  return { foundJamo: [...labels].sort() };
}

/** Real OSK keystroke: physical tap on the resolved key point; success is a bounded,
 *  observable DOM delta (draft growth / preedit / active value growth). No delta = failure
 *  with screenshot + hierarchy, never a silent pass. */
async function typeKey(evDir, label, observe) {
  const key = await resolveImeKey(evDir, label);
  await tapPoint(`ime-key:${label}`, key.x, key.y, observe, {
    screenshotOnFail: path.join(evDir, `key-${label}-no-delta.png`),
  });
}

async function typeSequence(evDir, labels, observe) {
  for (const label of labels) {
    await typeKey(evDir, label, observe);
    await delay(120); // natural inter-key gap for the real IME pipeline (not a wait-for-condition)
  }
}

/** Real OSK Enter tap. No tap-level delta is required HERE: an Enter absorbed by the IME during
 *  composition legitimately produces no DOM change — outcomes are classified at scenario level
 *  (four-way, evidence-backed) and never assumed. */
async function tapEnter(evDir) {
  const key = await resolveImeEnter(evDir);
  adb(["shell", "input", "tap", String(Math.round(key.x)), String(Math.round(key.y))]);
  appendLog(`tap ime-enter @ (${Math.round(key.x)}, ${Math.round(key.y)}) via ${key.via}`);
  await delay(150);
}

/** Ensure the RemoteTerminal input mode via the real toggle; verified by mode-state delta. */
async function ensureMode(evDir, cdp, want) {
  let s = await readState(cdp);
  if (s.mode === want) return s;
  const nodes = fetchHierarchy().nodes;
  const tap = await resolvePageTap(cdp, nodes, {
    text: s.mode === "direct" ? "Direct" : "Line",
    desc: s.mode === "direct" ? "Switch to line input mode" : "Switch to direct input mode",
    cdpRectExpr: rectExpr('[data-testid="remote-terminal-input-mode-toggle"]'),
    hint: "input-mode-toggle",
  });
  await tapPoint(`mode-switch->${want}`, tap.x, tap.y, async () => {
    const n = await readState(cdp);
    return n.mode === want;
  }, { screenshotOnFail: path.join(evDir, `mode-switch-to-${want}-failed.png`) });
  return readState(cdp);
}

// --------------------------------------------------------------------------- scenarios

/**
 * S1 — direct mode: real OSK jamo -> composition on the sink -> Enter -> the Hangul token
 * must appear in the PTY grid EXACTLY ONCE (poll for first occurrence, then one bounded
 * duplicate-detection settle window). Composition alone must never move the grid.
 */
async function scenarioDirectOnce(cdp) {
  const dir = "S1-direct-once";
  const files = [];
  const baseline = await ensureMode(dir, cdp, "direct");
  files.push(writeJson(path.join(dir, "00-baseline.json"), baseline));
  files.push(screenshot(path.join(dir, "01-baseline.png")));
  const ime = await ensureKeyboardOpen(cdp, dir, {
    cdpRectExpr: rectExpr('[data-testid="remote-terminal-grid"]'),
    focusTestid: "remote-terminal-input-sink",
    hint: "tap-grid-focus-sink",
  });
  const korean = requireKoreanLayout(dir, ime);
  files.push(screenshot(path.join(dir, "02-keyboard-open.png")));

  let lastLen = baseline.activeElement?.value?.length ?? 0;
  const basePreeditLen = (baseline.preedit ?? "").length;
  const observe = async () => {
    const s = await readState(cdp);
    const len = s.activeElement?.value?.length ?? 0;
    const preeditLen = (s.preedit ?? "").length;
    if (len > lastLen || preeditLen > basePreeditLen || (s.activeElement?.isComposing ?? false)) {
      lastLen = Math.max(lastLen, len);
      return true;
    }
    return false;
  };
  await typeSequence(dir, ["ㅁ", "ㅗ", "ㅂ", "ㅏ", "ㅇ", "ㅣ", "ㄹ"], observe);

  const preEnter = await readState(cdp);
  files.push(writeJson(path.join(dir, "03-pre-enter.json"), preEnter));
  files.push(screenshot(path.join(dir, "04-pre-enter.png")));
  const premature = preEnter.gridText !== baseline.gridText || preEnter.gridLines.length !== baseline.gridLines.length;
  if (premature) {
    throw assertionFailed(
      "premature-enter",
      `grid moved during composition alone: lines ${baseline.gridLines.length} -> ${preEnter.gridLines.length}`,
    );
  }

  await tapEnter(dir);
  const after = await poll("grid-echo-after-enter", async () => {
    const s = await readState(cdp);
    return countIn(s.gridText, "모바일") > countIn(baseline.gridText, "모바일") ? s : null;
  }, {});
  await delay(DUPLICATE_WINDOW_MS); // bounded window whose ONLY purpose is catching a second echo
  const settled = await readState(cdp);
  files.push(writeJson(path.join(dir, "04-after-enter.json"), { firstGained: after, settled }));
  files.push(screenshot(path.join(dir, "05-after-enter.png")));
  const delta = countIn(settled.gridText, "모바일") - countIn(baseline.gridText, "모바일");
  const assertions = {
    hangulToken: "모바일",
    occurrencesDelta: delta,
    exactlyOnce: delta === 1,
    gridLines: { before: baseline.gridLines.length, after: settled.gridLines.length },
    inputModeAfter: settled.mode,
    koreanJamoExposed: korean.foundJamo.length,
  };
  const captured = {
    baselineGridText: baseline.gridText,
    preEnterGridText: preEnter.gridText,
    finalGridText: settled.gridText,
    preeditAfterEnter: settled.preedit,
    lineDraftAfterEnter: settled.lineDraft,
    activeValueAfterEnter: settled.activeElement?.value ?? null,
  };
  files.push(writeJson(path.join(dir, "assertions.json"), {
    verdict: assertions.exactlyOnce ? "pass" : "fail",
    assertions,
    captured,
  }));
  if (!assertions.exactlyOnce) {
    throw assertionFailed("hangul-not-exactly-once", `모바일 occurrences delta=${delta} in grid text`);
  }
  return { name: "direct-once", status: "pass", assertions, captured, evidenceDir: dir, evidenceFiles: files };
}

/**
 * S2 — line<->direct mode switch: type 한글 into the line draft, switch away and back with a
 * real toggle tap (blur-in-flight): the grid must never grow during switches (no premature
 * send), the draft must survive the round trip, and Enter must deliver exactly one echo.
 */
async function scenarioModeSwitch(cdp) {
  const dir = "S2-mode-switch";
  const files = [];
  const baseline = await ensureMode(dir, cdp, "direct");
  files.push(writeJson(path.join(dir, "00-baseline.json"), baseline));
  files.push(screenshot(path.join(dir, "01-baseline.png")));

  const lineState = await ensureMode(dir, cdp, "line");
  const ime = await ensureKeyboardOpen(cdp, dir, {
    cdpRectExpr: rectExpr('[data-testid="remote-terminal-line-input"]'),
    focusTestid: "remote-terminal-line-input",
    hint: "tap-line-input",
  });
  requireKoreanLayout(dir, ime);
  files.push(screenshot(path.join(dir, "02-keyboard-line-mode.png")));

  let lastLen = lineState.lineDraft?.length ?? 0;
  const observe = async () => {
    const s = await readState(cdp);
    const len = s.lineDraft?.length ?? 0;
    if (len > lastLen || (s.activeElement?.isComposing ?? false)) {
      lastLen = Math.max(lastLen, len);
      return true;
    }
    return false;
  };
  await typeSequence(dir, ["ㅎ", "ㅏ", "ㄴ", "ㄱ", "ㅡ", "ㄹ"], observe);

  const typed = await readState(cdp);
  files.push(writeJson(path.join(dir, "03-typed.json"), typed));
  files.push(screenshot(path.join(dir, "04-typed.png")));
  const gridStableBeforeSwitch = typed.gridText === baseline.gridText;
  const draftAtSwitch = typed.lineDraft;
  if (!gridStableBeforeSwitch) {
    throw assertionFailed("premature-send-before-switch", "grid changed while only composing in the line draft");
  }
  if ((draftAtSwitch ?? "").trim() !== "한글") {
    throw assertionFailed("draft-not-as-typed", `line draft=${JSON.stringify(draftAtSwitch)} expected "한글"`);
  }

  const directState = await ensureMode(dir, cdp, "direct");
  files.push(writeJson(path.join(dir, "04-after-switch-direct.json"), directState));
  files.push(screenshot(path.join(dir, "05-switched-direct.png")));
  if (directState.gridText !== baseline.gridText) {
    throw assertionFailed("switch-sent-to-pty", "grid changed during direct->line->direct switching without Enter");
  }

  const backToLine = await ensureMode(dir, cdp, "line");
  files.push(writeJson(path.join(dir, "05-back-to-line.json"), backToLine));
  files.push(screenshot(path.join(dir, "06-back-to-line.png")));
  const draftRestored = backToLine.lineDraft;
  const roundTripClean = backToLine.gridText === baseline.gridText;
  if (!roundTripClean) {
    throw assertionFailed("switch-sent-to-pty", "grid changed across the line->direct->line round trip");
  }
  if (draftRestored !== draftAtSwitch) {
    throw assertionFailed(
      "draft-not-preserved-across-switch",
      `draft ${JSON.stringify(draftAtSwitch)} -> ${JSON.stringify(draftRestored)} across mode round trip`,
    );
  }

  await ensureKeyboardOpen(cdp, dir, {
    cdpRectExpr: rectExpr('[data-testid="remote-terminal-line-input"]'),
    focusTestid: "remote-terminal-line-input",
    hint: "refocus-line-input",
  });
  await tapEnter(dir);
  await poll("grid-echo-after-submit", async () => {
    const s = await readState(cdp);
    return countIn(s.gridText, "한글") > countIn(baseline.gridText, "한글");
  }, {});
  await delay(DUPLICATE_WINDOW_MS);
  const settled = await readState(cdp);
  files.push(writeJson(path.join(dir, "06-after-submit.json"), settled));
  files.push(screenshot(path.join(dir, "07-after-submit.png")));
  const delta = countIn(settled.gridText, "한글") - countIn(baseline.gridText, "한글");
  const assertions = {
    hangulToken: "한글",
    draftBeforeSwitch: draftAtSwitch,
    draftAfterRoundTrip: draftRestored,
    draftPreserved: draftRestored === draftAtSwitch,
    gridUnchangedThroughSwitches: roundTripClean,
    occurrencesDelta: delta,
    exactlyOnce: delta === 1,
    draftClearedAfterSubmit: (settled.lineDraft ?? "") === "",
  };
  const captured = {
    baselineGridText: baseline.gridText,
    typedGridText: typed.gridText,
    directSwitchGridText: directState.gridText,
    backToLineGridText: backToLine.gridText,
    finalGridText: settled.gridText,
    draftAtSwitch,
    draftRestored,
    finalDraft: settled.lineDraft,
    preeditAfterSubmit: settled.preedit,
  };
  files.push(writeJson(path.join(dir, "assertions.json"), {
    verdict: assertions.draftPreserved && assertions.gridUnchangedThroughSwitches && assertions.exactlyOnce ? "pass" : "fail",
    assertions,
    captured,
  }));
  if (!assertions.exactlyOnce) {
    throw assertionFailed("hangul-not-exactly-once-after-switch", `한글 occurrences delta=${delta}`);
  }
  return { name: "mode-switch", status: "pass", assertions, captured, evidenceDir: dir, evidenceFiles: files };
}

/**
 * S3 — Enter during live composition (line mode): classify the FIRST Enter honestly into one
 * of four evidence-backed outcomes: A committed+submitted in one, B absorbed by the IME with
 * zero send (then a second Enter submits), C premature send while draft held (FAILURE),
 * D draft lost with no PTY evidence (FAILURE). Either legal path must end exactly-once.
 */
async function scenarioCompositionEnter(cdp) {
  const dir = "S3-composition-enter";
  const files = [];
  const baseline = await ensureMode(dir, cdp, "line");
  files.push(writeJson(path.join(dir, "00-baseline.json"), baseline));
  files.push(screenshot(path.join(dir, "01-baseline.png")));
  const ime = await ensureKeyboardOpen(cdp, dir, {
    cdpRectExpr: rectExpr('[data-testid="remote-terminal-line-input"]'),
    focusTestid: "remote-terminal-line-input",
    hint: "tap-line-input",
  });
  requireKoreanLayout(dir, ime);

  let lastLen = baseline.lineDraft?.length ?? 0;
  const observe = async () => {
    const s = await readState(cdp);
    const len = s.lineDraft?.length ?? 0;
    if (len > lastLen || (s.activeElement?.isComposing ?? false)) {
      lastLen = Math.max(lastLen, len);
      return true;
    }
    return false;
  };
  await typeSequence(dir, ["ㄱ", "ㅏ", "ㄷ", "ㅏ"], observe);

  const typed = await readState(cdp);
  files.push(writeJson(path.join(dir, "03-typed.json"), typed));
  files.push(screenshot(path.join(dir, "04-typed.png")));
  const draftBefore = typed.lineDraft;
  if ((draftBefore ?? "").trim() !== "가다") {
    throw assertionFailed("draft-not-as-typed", `line draft=${JSON.stringify(draftBefore)} expected "가다"`);
  }
  if (typed.gridText !== baseline.gridText) {
    throw assertionFailed("premature-send", "grid changed before the first Enter — composition alone was sent");
  }

  await tapEnter(dir);
  let gained = null;
  try {
    gained = await poll("first-enter-echo", async () => {
      const s = await readState(cdp);
      return countIn(s.gridText, "가다") > countIn(baseline.gridText, "가다") ? s : null;
    }, {});
  } catch {
    // no echo within budget — classified below from a fresh read (B vs D)
  }

  let firstEnterFinding;
  let afterFirst;
  if (gained) {
    files.push(writeJson(path.join(dir, "04-after-first-enter.json"), gained));
    const draftKept = (gained.lineDraft ?? "").trim() === "가다";
    if (draftKept) {
      throw assertionFailed(
        "premature-send-during-composition",
        `grid gained 가다 while the draft is still held: ${JSON.stringify(gained.lineDraft)}`,
      );
    }
    firstEnterFinding = "committed-and-submitted-in-one";
    afterFirst = gained;
  } else {
    const s = await readState(cdp);
    files.push(writeJson(path.join(dir, "04-after-first-enter.json"), s));
    files.push(screenshot(path.join(dir, "05-after-first-enter.png")));
    if ((s.lineDraft ?? "").trim() === "가다") {
      firstEnterFinding = "absorbed-by-ime-no-send";
      await tapEnter(dir);
      afterFirst = await poll("second-enter-echo", async () => {
        const n = await readState(cdp);
        return countIn(n.gridText, "가다") > countIn(baseline.gridText, "가다") ? n : null;
      }, {});
    } else {
      throw assertionFailed(
        "draft-lost-without-pty-evidence",
        `draft vanished (${JSON.stringify(s.lineDraft)}) and the grid never gained 가다 within budget`,
      );
    }
  }

  await delay(DUPLICATE_WINDOW_MS);
  const final = await readState(cdp);
  files.push(writeJson(path.join(dir, "05-final.json"), final));
  files.push(screenshot(path.join(dir, "06-final.png")));
  const delta = countIn(final.gridText, "가다") - countIn(baseline.gridText, "가다");
  const assertions = {
    hangulToken: "가다",
    firstEnterFinding,
    occurrencesDelta: delta,
    exactlyOnce: delta === 1,
    draftAfterFinal: final.lineDraft,
    draftClearedAfterSubmit: (final.lineDraft ?? "") === "",
  };
  const captured = {
    baselineGridText: baseline.gridText,
    typedGridText: typed.gridText,
    afterFirstEnterGridText: afterFirst.gridText,
    finalGridText: final.gridText,
    draftBefore,
    draftAfterFirstEnter: afterFirst.lineDraft,
    finalDraft: final.lineDraft,
    preeditFinal: final.preedit,
  };
  files.push(writeJson(path.join(dir, "assertions.json"), {
    verdict: assertions.exactlyOnce ? "pass" : "fail",
    assertions,
    captured,
  }));
  if (!assertions.exactlyOnce) {
    throw assertionFailed("not-exactly-once", `가다 occurrences delta=${delta} after classified first Enter`);
  }
  return { name: "composition-enter", status: "pass", assertions, captured, evidenceDir: dir, evidenceFiles: files };
}

/**
 * S4 — chat/terminal target switch: type Korean into the CHAT composer with the real OSK and
 * never press send; switch views both ways; the terminal grid must gain ZERO occurrences of
 * the typed token (chat draft never crosses into the PTY) and the chat draft must be retained.
 * Skips with an honest recorded reason when RemoteApp hides the view toggle at this viewport.
 */
async function scenarioTargetSwitch(cdp) {
  const dir = "S4-target-switch";
  const files = [];
  let s = await readState(cdp);
  files.push(writeJson(path.join(dir, "00-initial.json"), s));
  files.push(screenshot(path.join(dir, "01-initial.png")));
  if (!s.viewModeButtons.terminalVisible || !s.viewModeButtons.chatVisible) {
    files.push(writeJson(path.join(dir, "skipped.json"), {
      reason: "view-mode-controls-not-present-at-viewport",
      viewModeButtons: s.viewModeButtons,
      note: "RemoteApp hides the Chat/Terminal toggle at this viewport; rotation or device settings changes are forbidden by this runner",
    }));
    return {
      name: "target-switch",
      status: "skipped",
      reason: "view-mode-controls-not-present-at-viewport",
      assertions: null,
      captured: { viewModeButtons: s.viewModeButtons, url: s.url },
      evidenceDir: dir,
      evidenceFiles: files,
    };
  }

  if (s.gridText === null) {
    const tap = await resolvePageTap(cdp, fetchHierarchy().nodes, {
      cdpRectExpr: rectExpr('[data-testid="remote-view-mode-terminal"]'),
      hint: "view-mode-terminal",
    });
    await tapPoint("show-terminal-view", tap.x, tap.y, async () => (await readState(cdp)).gridText !== null, {
      screenshotOnFail: path.join(dir, "show-terminal-view-failed.png"),
    });
    s = await readState(cdp);
  }
  const baseline = s;
  files.push(writeJson(path.join(dir, "02-terminal-baseline.json"), baseline));
  files.push(screenshot(path.join(dir, "03-terminal-baseline.png")));

  // Switch to chat view (real tap, verified by the composer appearing).
  const chatTap = await resolvePageTap(cdp, fetchHierarchy().nodes, {
    cdpRectExpr: rectExpr('[data-testid="remote-view-mode-chat"]'),
    hint: "view-mode-chat",
  });
  await tapPoint("show-chat-view", chatTap.x, chatTap.y, async () => {
    const n = await readState(cdp);
    return n.chatVisible === true || n.chatDraft !== null;
  }, { screenshotOnFail: path.join(dir, "show-chat-view-failed.png") });
  const chatView = await readState(cdp);
  files.push(writeJson(path.join(dir, "03b-chat-view.json"), chatView));
  files.push(screenshot(path.join(dir, "04-chat-view.png")));

  // Focus the real chat composer and type 한 through the OSK (draft only — send is never pressed).
  const ime = await ensureKeyboardOpen(cdp, dir, {
    cdpRectExpr: rectExpr('[data-testid="chat-composer-textarea"]'),
    focusTestid: "chat-composer-textarea",
    hint: "tap-chat-composer",
  });
  requireKoreanLayout(dir, ime);
  let lastLen = chatView.chatDraft?.length ?? 0;
  const observe = async () => {
    const n = await readState(cdp);
    const len = n.chatDraft?.length ?? 0;
    if (len > lastLen || (n.activeElement?.isComposing ?? false)) {
      lastLen = Math.max(lastLen, len);
      return true;
    }
    return false;
  };
  await typeSequence(dir, ["ㅎ", "ㅏ", "ㄴ"], observe);
  const typed = await readState(cdp);
  files.push(writeJson(path.join(dir, "05-chat-typed.json"), typed));
  files.push(screenshot(path.join(dir, "06-chat-typed.png")));
  const draftTyped = typed.chatDraft;
  if ((draftTyped ?? "").trim() !== "한") {
    throw assertionFailed("chat-draft-not-as-typed", `chat draft=${JSON.stringify(draftTyped)} expected "한"`);
  }

  // Back to terminal view; the grid must gain ZERO occurrences of the chat token.
  const termTap = await resolvePageTap(cdp, fetchHierarchy().nodes, {
    cdpRectExpr: rectExpr('[data-testid="remote-view-mode-terminal"]'),
    hint: "view-mode-terminal",
  });
  await tapPoint("back-to-terminal-view", termTap.x, termTap.y, async () => (await readState(cdp)).gridText !== null, {
    screenshotOnFail: path.join(dir, "back-to-terminal-failed.png"),
  });
  await delay(DUPLICATE_WINDOW_MS);
  const final = await readState(cdp);
  files.push(writeJson(path.join(dir, "06-terminal-final.json"), final));
  files.push(screenshot(path.join(dir, "07-terminal-final.png")));
  const ptyDelta = countIn(final.gridText, "한") - countIn(baseline.gridText, "한");

  // Chat draft must survive the round trip (re-enter chat view, read it back).
  const chatTap2 = await resolvePageTap(cdp, fetchHierarchy().nodes, {
    cdpRectExpr: rectExpr('[data-testid="remote-view-mode-chat"]'),
    hint: "view-mode-chat",
  });
  await tapPoint("reenter-chat-view", chatTap2.x, chatTap2.y, async () => {
    const n = await readState(cdp);
    return n.chatVisible === true || n.chatDraft !== null;
  }, { screenshotOnFail: path.join(dir, "reenter-chat-failed.png") });
  const chatFinal = await readState(cdp);
  files.push(writeJson(path.join(dir, "07-chat-final.json"), chatFinal));
  files.push(screenshot(path.join(dir, "08-chat-final.png")));
  const draftRetained = (chatFinal.chatDraft ?? "").trim() === draftTyped.trim();

  const assertions = {
    chatToken: "한",
    draftTyped,
    draftAfterRoundTrip: chatFinal.chatDraft,
    chatDraftRetained: draftRetained,
    ptyOccurrencesDeltaWhileChatOnly: ptyDelta,
    noChatTextReachedPty: ptyDelta === 0,
    sendPressed: false, // this runner never presses chat send — draft left in place, recorded
  };
  const captured = {
    baselineGridText: baseline.gridText,
    finalGridText: final.gridText,
    chatDraftTyped: draftTyped,
    chatDraftRetained: chatFinal.chatDraft,
    chatViewButtons: s.viewModeButtons,
  };
  files.push(writeJson(path.join(dir, "assertions.json"), {
    verdict: assertions.noChatTextReachedPty && assertions.chatDraftRetained ? "pass" : "fail",
    assertions,
    captured,
  }));
  if (!assertions.noChatTextReachedPty) {
    throw assertionFailed("chat-text-reached-pty", `"한" occurrences in grid grew by ${ptyDelta} while only typing into the chat composer`);
  }
  if (!assertions.chatDraftRetained) {
    throw assertionFailed("chat-draft-not-retained", `chat draft ${JSON.stringify(draftTyped)} -> ${JSON.stringify(chatFinal.chatDraft)} across view switches`);
  }
  return { name: "target-switch", status: "pass", assertions, captured, evidenceDir: dir, evidenceFiles: files };
}

// --------------------------------------------------------------------------- prerequisites (read-only)

const PREREQ = {};

/**
 * Fail-closed, read-only device/host verification. Uses only `adb devices`, `getprop`,
 * `dumpsys`, `pm path`, `ime list` and `settings get` — never enables/disables IMEs, never
 * installs apps, never touches keyguard, screen rotation, or any device setting.
 */
async function prereqProbe() {
  const candidates = [ARGS.adb, process.env.FERRYX_ADB, "/opt/homebrew/bin/adb", "/usr/bin/adb", "/opt/local/bin/adb", "adb"].filter(Boolean);
  let chosen = null;
  let version = null;
  for (const candidate of candidates) {
    const probe = spawnSync(candidate, ["version"], { encoding: "utf8" });
    if (!probe.error && probe.status === 0) {
      chosen = candidate;
      version = (probe.stdout ?? "").split("\n")[0].trim();
      break;
    }
  }
  if (!chosen) {
    throw blocked("adb-not-found", `tried: ${candidates.join(", ")} — pass --adb /path/to/adb`);
  }
  ADB = chosen;
  RUN.adbPath = chosen;
  RUN.adbVersion = version;
  PREREQ.adb = { path: chosen, version };
  appendLog(`adb: ${chosen} (${version})`);

  // Exact serial match against `adb devices -l` — no wildcard, ever.
  const devices = adbRaw(["devices", "-l"]).stdout;
  writeFileSync(evPath("prereq/adb-devices.txt"), devices, "utf8");
  const line = devices.split("\n").find((l) => l.trim().startsWith(`${SERIAL}\t`) || l.trim().startsWith(`${SERIAL} `));
  if (!line) {
    throw blocked("device-not-attached", `serial ${SERIAL} absent from adb devices -l (saved to prereq/adb-devices.txt)`);
  }
  const state = line.trim().split(/\s+/)[1] ?? "";
  if (state === "unauthorized") {
    throw blocked("device-unauthorized", "accept the USB debugging authorization prompt on the device, then re-run");
  }
  if (state !== "device") {
    throw blocked("device-not-ready", `serial ${SERIAL} state=${state}`);
  }
  PREREQ.device = { serial: SERIAL, adbState: state, adbDevicesLine: line.trim() };

  // Physical device only — emulators are not evidence for this gate.
  const qemu = adb(["shell", "getprop", "ro.kernel.qemu"]).stdout.trim();
  const characteristics = adb(["shell", "getprop", "ro.build.characteristics"]).stdout.trim();
  if (qemu === "1" || characteristics.includes("emulator")) {
    throw blocked(
      "not-physical",
      `ro.kernel.qemu=${qemu} ro.build.characteristics=${characteristics}; only a physically attached authorized device satisfies this gate`,
    );
  }

  const props = adb(["shell", "getprop"]).stdout;
  writeFileSync(evPath("prereq/device-props.txt"), props, "utf8");
  const prop = (name) => new RegExp(`\\[${name}\\]: \\[(.*?)\\]`).exec(props)?.[1] ?? null;
  PREREQ.device = {
    ...PREREQ.device,
    manufacturer: prop("ro.product.manufacturer"),
    model: prop("ro.product.model"),
    device: prop("ro.product.device"),
    androidRelease: prop("ro.build.version.release"),
    sdk: prop("ro.build.version.sdk"),
    buildId: prop("ro.build.id"),
    abis: prop("ro.product.cpu.abilist"),
    hardware: prop("ro.hardware"),
  };

  // Screen must be on and unlocked — read-only checks; the runner never wakes or unlocks.
  const display = adb(["shell", "dumpsys", "display"]).stdout;
  const screenState = /mScreenState=(ON|OFF)/.exec(display)?.[1] ?? null;
  if (screenState === "OFF") {
    throw blocked("screen-off", "wake the device manually — this runner never presses hardware keys");
  }
  const windows = adb(["shell", "dumpsys", "window", "displays"]).stdout;
  const currentFocus = /mCurrentFocus=([^\n]+)/.exec(windows)?.[1]?.trim() ?? null;
  if (currentFocus && /keyguard/i.test(currentFocus)) {
    throw blocked("device-locked", `focus=${currentFocus}; unlock the device manually — this runner never dismisses keyguard`);
  }
  PREREQ.screen = { screenState, currentFocus };

  // Chrome must already be installed — the runner never installs apps.
  const chromePath = adb(["shell", "pm", "path", "com.android.chrome"]).stdout.trim();
  if (!chromePath.startsWith("package:")) {
    throw blocked("chrome-missing", "com.android.chrome not installed on the device; runner never installs apps");
  }
  const chromeDump = adb(["shell", "dumpsys", "package", "com.android.chrome"]).stdout;
  const chromeVersion = /versionName=([^\s]+)/.exec(chromeDump)?.[1] ?? null;
  PREREQ.chrome = {
    packageName: "com.android.chrome",
    apkPath: chromePath.replace(/^package:/, ""),
    versionName: chromeVersion,
  };
  writeFileSync(evPath("prereq/chrome.txt"), `path: ${chromePath}\nversionName: ${chromeVersion ?? "unknown"}\n`, "utf8");

  // Active IME must be the confirmed HoneyBoard profile (read-only; no IME switching ever).
  const imeList = adb(["shell", "ime", "list", "-s"]).stdout;
  const defaultIme = adb(["shell", "settings", "get", "secure", "default_input_method"]).stdout.trim();
  const imeDump = adb(["shell", "dumpsys", "input_method"]).stdout;
  writeFileSync(
    evPath("prereq/ime.txt"),
    `default_input_method: ${defaultIme}\n\nime list -s:\n${imeList}\n\ndumpsys input_method (truncated):\n${imeDump.slice(0, 200000)}\n`,
    "utf8",
  );
  if (!defaultIme.includes(EXPECTED_IME)) {
    throw blocked(
      "unexpected-ime",
      `default_input_method=${defaultIme}; this runner is authored for the confirmed ${EXPECTED_IME} profile — confirm the on-device keyboard manually and re-run`,
    );
  }
  if (!imeList.includes(defaultIme)) {
    throw blocked("ime-not-enabled", `default IME ${defaultIme} missing from the enabled list (saved to prereq/ime.txt)`);
  }
  PREREQ.ime = {
    defaultIme,
    enabled: imeList.split("\n").map((l) => l.trim()).filter(Boolean),
  };

  // Honesty for mixed device sets: iOS stays BLOCKED on this host, never green.
  if ((ARGS.deviceSet ?? "").includes("ios")) {
    RUN.blockedTargets.push({
      target: "ios",
      reason: "no iOS-capable host assigned for this run (no macOS+Xcode device driver; Windows cannot drive iOS) — iOS native IME evidence remains BLOCKED and must not be reported as green",
    });
  }
  PREREQ.host = { platform: process.platform, release: os.release(), node: process.version };
  writeJson("prereq/provenance.json", PREREQ);
  appendLog(`prereq OK: ${PREREQ.device.model ?? "?"} android ${PREREQ.device.androidRelease ?? "?"} ime=${defaultIme}`);
}

// --------------------------------------------------------------------------- cleanup (own resources only)

/** Remove ONLY the forwards/reverses this run created, by exact id, and record a receipt. */
function cleanup() {
  if (!ADB || !SERIAL) return;
  for (const id of RUN.cleanup.createdReverses) {
    const r = run(ADB, ["-s", SERIAL, "reverse", "--remove", id], { expectCodeZero: false });
    RUN.cleanup.removals.push({ type: "adb-reverse", id, exit: r.code, stderr: r.stderr.slice(0, 200) });
  }
  for (const id of RUN.cleanup.createdForwards) {
    const r = run(ADB, ["-s", SERIAL, "forward", "--remove", id], { expectCodeZero: false });
    RUN.cleanup.removals.push({ type: "adb-forward", id, exit: r.code, stderr: r.stderr.slice(0, 200) });
  }
  appendLog(`cleanup: ${RUN.cleanup.removals.length} own resource(s) processed`);
}

// --------------------------------------------------------------------------- result schema + report

const SCENARIO_DIRS = {
  "direct-once": "S1-direct-once",
  "mode-switch": "S2-mode-switch",
  "composition-enter": "S3-composition-enter",
  "target-switch": "S4-target-switch",
};

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

/**
 * result.json — schema `ferryx-herdr-native-ime.result/1` (filename: <evidence-dir>/result.json).
 * Acceptance consumers MUST read scenarios[].captured (actual before/after grid text, occurrence
 * deltas, drafts, preedit) plus device/chrome/ime/page provenance and the cleanup receipt.
 * The top-level `verdict` is a convenience summary ONLY and is never sufficient on its own:
 * `verdict:"pass"` requires exitCode 0 with EVERY requested scenario at status `pass`;
 * `skipped`, `not-run`, `fail` and `error` are never green.
 */
function buildResult() {
  const allGreen =
    RUN.exitCode === EXIT.OK &&
    RUN.scenarios.length > 0 &&
    RUN.scenarios.every((s) => s.status === "pass");
  return {
    schema: "ferryx-herdr-native-ime.result/1",
    verdict: allGreen ? "pass" : "fail",
    verdictSemantics:
      "summary only — consume scenarios[].captured and page/device/ime provenance; skipped and not-run are not green",
    runner: {
      scriptId: RUN.scriptId,
      scriptSha256: RUN.scriptSha256,
      startedAt: RUN.startedAt,
      finishedAt: RUN.finishedAt,
      host: RUN.host,
      adbPath: RUN.adbPath,
      adbVersion: RUN.adbVersion,
      evidenceDirAbsolute: EVIDENCE_DIR,
    },
    device: PREREQ.device ?? null,
    screen: PREREQ.screen ?? null,
    chrome: PREREQ.chrome ?? null,
    ime: PREREQ.ime ?? null,
    page: {
      urlRedacted: ARGS ? redactUrl(ARGS.pageUrl) : null,
      cdp: RUN.cdp,
      chromeOffsetsUsed: RUN.chromeOffsets,
    },
    scenariosRequested: ARGS?.scenarios ?? [],
    scenarios: RUN.scenarios,
    blockedTargets: RUN.blockedTargets,
    ptyHook: RUN.ptyHook,
    cleanup: RUN.cleanup,
    failure: RUN.failure,
    exitCode: RUN.exitCode,
  };
}

function writeReport() {
  const result = buildResult();
  const lines = [];
  lines.push("# herdr-native-ime evidence report");
  lines.push("");
  lines.push(`- schema: \`${result.schema}\``);
  lines.push(`- verdict (summary only): **${result.verdict}** — consumers must read scenario captures, never this boolean`);
  lines.push(`- exitCode: ${result.exitCode}`);
  lines.push(`- script sha256: \`${result.runner.scriptSha256 ?? "unknown"}\``);
  lines.push(`- device: ${result.device ? `${result.device.manufacturer ?? "?"} ${result.device.model ?? "?"} (serial ${result.device.serial}, Android ${result.device.androidRelease ?? "?"}, sdk ${result.device.sdk ?? "?"})` : "not probed"}`);
  lines.push(`- chrome: ${result.chrome?.versionName ?? "not probed"} | IME: ${result.ime?.defaultIme ?? "not probed"}`);
  lines.push(`- page: ${result.page.urlRedacted ?? "?"} (CDP ${result.page.cdp ? `via ${result.page.cdp.forwardSpec}` : "not connected"})`);
  lines.push("");
  lines.push("## scenarios");
  lines.push("");
  lines.push("| scenario | status | reason | key captures |");
  lines.push("|---|---|---|---|");
  for (const s of result.scenarios) {
    const key = s.captured
      ? `occDelta=${s.assertions?.occurrencesDelta ?? s.assertions?.ptyOccurrencesDeltaWhileChatOnly ?? "-"} grid ${JSON.stringify(s.captured.baselineGridText ?? "").slice(0, 40)} -> ${JSON.stringify(s.captured.finalGridText ?? "").slice(0, 40)}`
      : "-";
    lines.push(`| ${s.name} | ${s.status} | ${s.reason ?? ""} | ${key} |`);
  }
  if (result.blockedTargets.length > 0) {
    lines.push("");
    lines.push("## blocked targets (NOT green)");
    for (const b of result.blockedTargets) {
      lines.push(`- ${b.target}: ${b.reason}${b.detail ? ` — ${b.detail}` : ""}`);
      if (Array.isArray(b.evidenceFiles) && b.evidenceFiles.length > 0) {
        lines.push(`  - evidence files: ${b.evidenceFiles.join(", ")}`);
      }
    }
  }
  if (result.failure) {
    lines.push("");
    lines.push("## failure");
    lines.push(`- reason: ${result.failure.reason}`);
    lines.push(`- detail: ${result.failure.detail}`);
    if (Array.isArray(result.failure.evidenceFiles) && result.failure.evidenceFiles.length > 0) {
      lines.push(`- evidence files: ${result.failure.evidenceFiles.join(", ")}`);
    }
  }
  lines.push("");
  lines.push("## cleanup receipt (own resources only)");
  for (const r of result.cleanup.removals) lines.push(`- ${r.type} ${r.id}: exit ${r.exit}`);
  if (result.ptyHook) {
    lines.push("");
    lines.push(`## pty hook: \`${result.ptyHook.command}\` exit=${result.ptyHook.exitCode} (${result.ptyHook.outputFile})`);
  }
  lines.push("");
  lines.push("## evidence integrity");
  lines.push("- Every keyboard strike was a physical `adb shell input tap` on the real Samsung HoneyBoard OSK; no input text, keyevents-as-text, or JS composition injection was used.");
  lines.push("- Every page/key tap was verified by a bounded observable DOM delta or recorded as a failure with screenshots + UI hierarchy XML.");
  lines.push("- All device reads were getprop/dumpsys/pm/ime-list/settings-get only; no settings, IMEs, apps, or keyguard were modified.");
  writeFileSync(evPath("report.md"), lines.join("\n") + "\n", "utf8");
}

// --------------------------------------------------------------------------- main

const SCENARIOS = {
  "direct-once": scenarioDirectOnce,
  "mode-switch": scenarioModeSwitch,
  "composition-enter": scenarioCompositionEnter,
  "target-switch": scenarioTargetSwitch,
};

async function main() {
  RUN.startedAt = new Date().toISOString();
  ARGS = parseArgs(process.argv.slice(2));
  if (ARGS.pageUrl.includes("'") || /[;`$\\|]/.test(ARGS.pageUrl)) {
    usage("--page-url must be a plain URL without shell metacharacters");
  }
  SERIAL = ARGS.serial;
  EVIDENCE_DIR = path.resolve(ARGS.evidenceDir);
  mkdirSync(EVIDENCE_DIR, { recursive: true });
  RUN.serial = SERIAL;
  RUN.deviceSet = ARGS.deviceSet;
  try {
    RUN.scriptSha256 = createHash("sha256").update(readFileSync(new URL(import.meta.url))).digest("hex");
  } catch (err) {
    appendLog(`script hash failed: ${String(err)}`);
  }
  KEYMAP = loadKeymap(ARGS.keymap);
  writeJson("run-config.json", {
    scriptId: SCRIPT_ID,
    scriptSha256: RUN.scriptSha256,
    serial: SERIAL,
    pageUrlRedacted: redactUrl(ARGS.pageUrl),
    deviceSet: ARGS.deviceSet,
    scenarios: ARGS.scenarios,
    keymapFile: ARGS.keymap,
    tapOffsetY: ARGS.tapOffsetY,
    adb: ARGS.adb,
    adbReverse: ARGS.adbReverse,
    timeoutMs: ARGS.timeoutMs,
    ptyHookSet: !!ARGS.ptyHook,
    startedAt: RUN.startedAt,
    policy: "real-OSK-taps-only; read-only provenance; own-resource cleanup; no settings changes; no installs; no coordinate guessing",
  });

  process.on("SIGINT", () => {
    appendLog("SIGINT received — cleaning up own resources, exiting 130");
    cleanup();
    RUN.exitCode = EXIT.INTERRUPT;
    RUN.failure = { reason: "interrupted", detail: "SIGINT" };
    RUN.finishedAt = new Date().toISOString();
    try {
      writeJson("result.json", buildResult());
      writeReport();
    } catch {
      // best-effort during interrupt
    }
    process.exit(EXIT.INTERRUPT);
  });

  let exitCode = EXIT.OK;
  let cdp = null;
  try {
    await prereqProbe();

    for (const pair of ARGS.adbReverse) {
      const [dev, host] = pair.split(":");
      if (!/^[0-9]+$/.test(dev ?? "") || !/^[0-9]+$/.test(host ?? "")) {
        usage(`--adb-reverse must be devPort:hostPort, got ${pair}`);
      }
      const id = `tcp:${dev}`;
      const r = run(ADB, ["-s", SERIAL, "reverse", id, `tcp:${host}`], { expectCodeZero: false });
      if (r.code !== 0) {
        throw interactionFailed("adb-reverse-failed", `${id} -> tcp:${host}: ${r.stderr.slice(0, 200)}`);
      }
      RUN.cleanup.createdReverses.push(id);
      appendLog(`adb reverse created: ${id} -> tcp:${host}`);
    }

    // Open the isolated gateway page in the real Chrome (single-quoted URL protects & in query).
    const launch = adb(
      ["shell", `am start -a android.intent.action.VIEW -d '${ARGS.pageUrl}'`],
      { expectCodeZero: false },
    );
    if (launch.code !== 0) {
      throw blocked("chrome-launch-failed", `am start exit ${launch.code}: ${(launch.stderr || launch.stdout).slice(0, 300)}`);
    }
    writeFileSync(
      evPath("prereq/chrome-launch.txt"),
      `command: am start -a android.intent.action.VIEW -d '${ARGS.pageUrl}'\n` +
        `exit: ${launch.code}\n--- stdout ---\n${launch.stdout ?? ""}\n--- stderr ---\n${launch.stderr ?? ""}\n`,
      "utf8",
    );
    appendLog(`chrome launched for ${redactUrl(ARGS.pageUrl)}`);

    cdp = await connectCdp({
      launchOutput: `${launch.stdout ?? ""}\n${launch.stderr ?? ""}`,
      launchExit: launch.code,
    });

    for (const name of ARGS.scenarios) {
      const impl = SCENARIOS[name];
      if (!impl) throw blocked("unknown-scenario", `${name}; known: ${Object.keys(SCENARIOS).join(", ")}`);
      appendLog(`scenario ${name}: start`);
      try {
        const res = await impl(cdp);
        RUN.scenarios.push(res);
        appendLog(`scenario ${name}: ${res.status}`);
      } catch (err) {
        RUN.scenarios.push({
          name,
          status: err instanceof RunnerError ? "fail" : "error",
          code: err instanceof RunnerError ? err.code : 1,
          reason: err.reason ?? String(err),
          detail: String(err.detail ?? err.message ?? "").slice(0, 4000),
          evidenceDir: SCENARIO_DIRS[name] ?? null,
          evidenceFiles: err.evidenceFiles ?? [],
        });
        throw err;
      }
    }

    if (ARGS.ptyHook) {
      const r = run("/bin/sh", ["-c", ARGS.ptyHook], { timeoutMs: 60000 });
      writeFileSync(
        evPath("pty-hook-output.txt"),
        `$ ${ARGS.ptyHook}\n--- exit ${r.code} ---\n${r.stdout ?? ""}\n${r.stderr ?? ""}\n`,
        "utf8",
      );
      RUN.ptyHook = {
        command: ARGS.ptyHook,
        exitCode: r.code,
        outputFile: "pty-hook-output.txt",
        stdoutBytes: Buffer.byteLength(r.stdout ?? ""),
        stderrBytes: Buffer.byteLength(r.stderr ?? ""),
      };
    }
  } catch (err) {
    if (err instanceof RunnerError) {
      exitCode = err.code;
      RUN.failure = {
        reason: err.reason,
        detail: String(err.detail ?? "").slice(0, 4000),
        evidenceFiles: err.evidenceFiles ?? [],
      };
    } else {
      exitCode = 1;
      RUN.failure = { reason: "unexpected-error", detail: String(err?.stack ?? err).slice(0, 4000) };
    }
    appendLog(`FAILED exit=${exitCode} ${RUN.failure.reason}: ${RUN.failure.detail}`);
    const executed = new Set(RUN.scenarios.map((s) => s.name));
    for (const name of ARGS.scenarios) {
      if (!executed.has(name)) {
        RUN.scenarios.push({ name, status: "not-run", reason: "run stopped after failure" });
      }
    }
  } finally {
    if (cdp) cdp.close();
    cleanup();
    RUN.exitCode = exitCode;
    RUN.finishedAt = new Date().toISOString();
    try {
      writeJson("result.json", buildResult());
      writeReport();
      appendLog(`result.json + report.md written; exit=${exitCode}`);
    } catch (err) {
      appendLog(`result write failed: ${String(err)}`);
    }
  }
  return exitCode;
}

process.exit(await main());
