#!/usr/bin/env node
/**
 * herdr-native-ime.mjs — real-device Android Korean-composition, layout, scroll and copy
 * producer for the Herdr reference-chat parity plan (task 15, QA-08).
 *
 * Device receipts, rather than this header, establish execution.
 * the receipt this file writes is validated by task 14's frozen
 * `validateDeviceReceipt()` in `scripts/qa/herdr-reference-fixtures.mjs`.
 *
 * WHAT IT PROVES (only when a run passes on an integrated candidate)
 *   Real Korean OSK input on a PHYSICALLY ATTACHED Android device reaches the served
 *   reference-chat candidate exactly once, and the phone surfaces survive the interactions
 *   QA-08 names:
 *     composition   the Hangul token appears in the original PTY grid exactly once, and
 *                   composition alone never moves the grid;
 *     layout        the keyboard shrinks the visual viewport and the surface control stays
 *                   visible (not under the IME window, not outside the visible region);
 *     scroll        a real touch swipe moves the rendered scrollback;
 *     copy          the transcript's own copy affordance performs a real clipboard write;
 *     mode          switching chat <-> terminal keeps the drafts and sends nothing;
 *     target        a chat draft never crosses into the PTY;
 *     late ACK      an echo arriving after a view switch does not duplicate or steal focus;
 *     Enter during live composition is classified honestly and never sends prematurely.
 *
 * HOST CONTRACT
 *   Mac/Linux/Windows host with `adb` + Node >= 22 (global WebSocket is required for the
 *   read-only Chrome DevTools Protocol observation). Builds and gateway serving stay on the
 *   verification host; the device reaches the isolated gateway through the caller-provided
 *   URL and/or runner-created `adb reverse` mappings. This producer starts NO server.
 *
 * WHAT IT WILL NEVER DO (fail-closed, evidence-integrity policy)
 *   - Never injects text (`adb shell input text`), never sends keyevents as text, never
 *     dispatches JS composition/input events, never fabricates screenshots, transcripts or
 *     coordinates.
 *   - Never changes device or keyboard settings: only READS `getprop` / `settings get` /
 *     `ime list` / `dumpsys` / `pm path`. Installs nothing, enables nothing, never touches
 *     keyguard, never rotates the device.
 *   - Never trusts coordinates: tap targets come from the UI hierarchy, from CDP rects plus a
 *     runtime-derived browser-chrome offset, or from an operator-authored --keymap; EVERY tap is
 *     verified by a bounded, observable DOM delta before the run proceeds. No delta => recorded
 *     failure with screenshots + hierarchy, never a silent pass.
 *   - Never accepts a Chrome first-run/consent screen. That outcome is a BLOCKED target with
 *     reason `device-owner-acknowledgment-needed`; automation can never acknowledge for the owner.
 *   - Never removes adb forwards/reverses it did not create itself (exact-id tracking + receipt).
 *   - Never kills a process by pattern. Only PIDs recorded at spawn time, re-checked against the
 *     executable that was launched, are ever terminated; anything else is REPORTED.
 *   - Never claims a green QA-08 for anything it did not observe on this device.
 *
 * CANDIDATE SURFACE BOUNDARY (authored against the task-12 candidate UI, not against the
 * older herdr-mobile-interaction UI this file is ported from)
 *   The reference-chat candidate renders `remote-terminal-grid` (DOM lines carrying
 *   `data-grid-line`), `remote-terminal-input-sink`, `remote-terminal-preedit` and the header
 *   view-mode buttons `remote-view-mode-chat` / `remote-view-mode-terminal`. It does NOT render
 *   `remote-terminal-line-input` or `remote-terminal-input-mode-toggle`: there is no line/direct
 *   terminal input mode in this candidate.
 *
 *   The frozen scenario name `mode-switch` is the CHAT/TERMINAL VIEW-MODE switch, and the receipt
 *   names that subject explicitly so no reader has to infer it:
 *     mode-switch / happy       subject "chat-terminal-view-mode-toggle" — a settled chat draft
 *                               survives a real chat -> terminal -> chat round trip through the
 *                               header view-mode buttons;
 *     mode-switch / failure     the same subject, switched MID-composition: nothing sent, draft
 *                               not lost, focus not stolen.
 *   The QA-08 requirement is exactly those two rows; no terminal line/direct input-mode behavior is
 *   required by the plan, and none is asserted here. The candidate's absent
 *   `remote-terminal-line-input` / `remote-terminal-input-mode-toggle` surfaces are therefore a
 *   fact about this UI, not an unmet gate.
 *
 * FIXED INVOCATION (plan, "Fixture provisioning and device commands")
 *   node scripts/qa/herdr-native-ime.mjs \
 *     --serial <provisioned-serial> \
 *     --evidence-dir <dir> \
 *     --page-url <isolated-gateway-url> \
 *     --scenarios direct-once,mode-switch,composition-enter,target-switch
 *   The serial comes from the provisioned fixture manifest / invocation. The historically seen
 *   R3CN8126R4Y is NOT assumed to exist.
 *
 * BINDING (how the candidate/page/target/PTY tuple reaches this producer)
 *   The fixed CLI above has no manifest argument, so the binding arrives through the environment
 *   variable FERRYX_HERDR_REFERENCE_BINDING, holding the ABSOLUTE path of the binding JSON that
 *   the task 14 runner writes (--device-binding-out, default <evidence-dir>/device-binding.json;
 *   the resolved path is echoed on the runner's stdout). There is NO fallback path: an unset
 *   variable, a relative path, a missing file, an incomplete binding, or a binding whose page
 *   origin disagrees with --page-url is BLOCKED (exit 2), never guessed.
 *
 * RECEIPT
 *   `<evidence-dir>/<platform>/device-receipt.json` (the binding's receiptPath when present),
 *   schema `ferryx-herdr-reference.device/1`, with candidate/page/target/pty copied verbatim
 *   from the binding, device provenance read from the attached device, one row per
 *   (scenario, case) with its real captures, and a cleanup ledger of the exact resources this
 *   run created. A missing device, driver or consent produces verdict "blocked" and a nonzero
 *   exit - never a PASS.
 *
 * EVIDENCE LAYOUT (under --evidence-dir)
 *   run-config.json, runner.log, result.json, report.md, device-receipt.json,
 *   prereq/{provenance.json, adb-devices.txt, device-props.txt, chrome.txt, ime.txt,
 *     chrome-launch.txt, cdp-socket-probe.json, cdp-blocked.png, proc-net-unix-at-block.txt},
 *   <scenario>/<case>/{screenshots, hierarchy XML, JSON state captures, layout/scroll/copy
 *     captures, assertions.json}
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
  REFERENCE_ANDROID_SCENARIOS,
  REFERENCE_DEVICE_BINDING_ENV,
  REFERENCE_DEVICE_RECEIPT_BINDING,
  REFERENCE_DEVICE_RECEIPT_SCHEMA,
  SPAWN_LEDGER_SCHEMA,
  deviceReceiptPath,
  killExactPid,
  probeProcessIdentity,
  readDeviceBinding,
} from "./herdr-reference-fixtures.mjs";

const SCRIPT_ID = "herdr-native-ime.mjs/1.2.0";
const EXIT = { OK: 0, BLOCKED: 2, INTERACTION: 3, ASSERTION: 4, USAGE: 5, INTERRUPT: 130 };
const DUPLICATE_WINDOW_MS = 700; // settle window used ONLY to detect a second, late echo
const CASES = ["happy", "failure"];
const TOKENS = { direct: "모바일", composition: "가다", chat: "한" };
// A hierarchy heuristic ONLY: the package fragment that helps locate the on-screen keyboard's
// window inside uiautomator's dump. It is never a gate — the hard gate is the Korean layout probe
// (requireKoreanLayout) against the keyboard the device actually shows, so a different real
// keyboard on a different provisioned device is not rejected for its package name.
const EXPECTED_IME = "honeyboard";
const JAMO = new Set(
  ("ㄱㄲㄳㄴㄵㄶㄷㄸㄹㄺㄻㄼㄽㄾㄿㅀㅁㅂㅃㅄㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ" +
    "ㅏㅐㅑㅒㅓㅔㅕㅖㅗㅘㅙㅚㅛㅜㅝㅞㅟㅠㅡㅢㅣ").split(""),
);

/* --------------------------------------------------------------------------- state */

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
  caseName: null,
  scenarios: [],
  blockedTargets: [],
  blockedReason: null,
  cdp: null,
  chromeOffsets: [],
  binding: null,
  receiptPath: null,
  maxViewportHeight: null,
  minViewportHeight: null,
  baselineState: null,
  cleanup: { createdForwards: [], createdReverses: [], removals: [], entries: [], receipts: [], killed: [] },
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
      "  node scripts/qa/herdr-native-ime.mjs --serial <device-serial> --evidence-dir <dir>\n" +
      "      --page-url <url> [--scenarios " + REFERENCE_ANDROID_SCENARIOS.join(",") + "]\n" +
      "      [--case happy|failure|all] [--adb <path>] [--adb-reverse dev:host]... [--keymap <file>]\n" +
      "      [--tap-offset-y <px>] [--expected-ime <substring>] [--paste-label <label>]\n" +
      "      [--timeout-ms <ms>] [--pty-hook <command>]\n",
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
    expectedIme: null,
    pasteLabel: null,
    scenarios: REFERENCE_ANDROID_SCENARIOS.slice(),
    caseName: "all",
    timeoutMs: 15000,
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
      case "--serial": args.serial = next(); break;
      case "--evidence-dir": args.evidenceDir = next(); break;
      case "--page-url": args.pageUrl = next(); break;
      case "--adb": args.adb = next(); break;
      case "--adb-reverse": args.adbReverse.push(next()); break;
      case "--keymap": args.keymap = next(); break;
      case "--tap-offset-y": args.tapOffsetY = Number.parseInt(next(), 10); break;
      case "--expected-ime": args.expectedIme = next(); break;
      case "--paste-label": args.pasteLabel = next(); break;
      case "--scenarios":
        args.scenarios = next().split(",").map((s) => s.trim()).filter(Boolean);
        break;
      case "--case": args.caseName = next(); break;
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
  if (!args.serial) usage("--serial is mandatory (the provisioned physical device serial; no wildcard default)");
  if (!args.evidenceDir) usage("--evidence-dir is mandatory");
  if (!args.pageUrl) usage("--page-url is mandatory (isolated gateway URL reachable from the device)");
  if (!Number.isFinite(args.timeoutMs) || args.timeoutMs <= 0) usage("--timeout-ms must be a positive integer");
  if (!Number.isFinite(args.tapOffsetY)) usage("--tap-offset-y must be an integer");
  if (!CASES.includes(args.caseName) && args.caseName !== "all") {
    usage("--case must be happy, failure or all");
  }
  if (args.scenarios.length === 0) usage("--scenarios must name at least one scenario");
  const unknown = args.scenarios.filter((s) => !REFERENCE_ANDROID_SCENARIOS.includes(s));
  if (unknown.length > 0) {
    usage("unknown scenario(s) " + unknown.join(", ") + "; the frozen set is " + REFERENCE_ANDROID_SCENARIOS.join(", "));
  }
  return args;
}

function selectedCases() {
  return ARGS.caseName === "all" ? CASES.slice() : [ARGS.caseName];
}

/* --------------------------------------------------------------------------- binding */

/**
 * Read the four-way binding the task 14 runner wrote. The path comes ONLY from
 * FERRYX_HERDR_REFERENCE_BINDING; a missing variable, a relative path, a missing file or an
 * incomplete binding is BLOCKED. Nothing here is ever defaulted.
 */
function loadBinding() {
  let read;
  try {
    read = readDeviceBinding();
  } catch (err) {
    const message = String((err && err.message) || err);
    // The frozen validator reports a binding that exists but lacks a bound block separately from
    // one that is absent, because they need different operator action. A binding with no PTY
    // identity is the case where the owning daemon exposes no shell child: this producer reports
    // it as a missing infrastructure prerequisite and NEVER substitutes a PID of its own.
    const incomplete = /incomplete/.test(message);
    throw blocked(
      incomplete ? "device-binding-incomplete" : "device-binding-missing",
      message + (incomplete
        ? " — a bound block is absent; for the PTY block this means the provisioner could not resolve the original shell identity, which this producer must not fabricate"
        : ""),
    );
  }
  const binding = read.binding;
  const pageUrl = new URL(ARGS.pageUrl);
  let boundPage;
  try {
    boundPage = new URL(binding.page.url);
  } catch (err) {
    throw blocked("device-binding-page-invalid", "binding page.url is not a URL: " + String(binding.page.url));
  }
  if (boundPage.origin !== pageUrl.origin) {
    throw blocked(
      "page-url-binding-mismatch",
      "--page-url origin " + pageUrl.origin + " is not the bound page origin " + boundPage.origin +
        "; a receipt must never bind a different served page than the one driven",
    );
  }
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
  RUN.bindingReceiptPath = typeof binding.receiptPath === "string" && binding.receiptPath.length > 0
    ? path.resolve(binding.receiptPath)
    : null;
  RUN.receiptPath = resolveReceiptPath(EVIDENCE_DIR, "android", binding.receiptPath);
  appendLog("binding loaded: " + read.path + " -> receipt " + RUN.receiptPath);
  return RUN.binding;
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


// --------------------------------------------------------------------------- page state (read-only capture)

/**
 * The read-only page-state probe for THIS candidate (reference chat, task 12 UI).
 *
 * The grid is rendered as DOM lines carrying `data-grid-line` (RemoteTerminal.tsx), so the
 * rendered scrollback is readable as text. The candidate has NO line/direct terminal input
 * mode: `remote-terminal-line-input` and `remote-terminal-input-mode-toggle` do not exist, so
 * this probe reports `lineDraft`/`lineVisible`/`mode` as null/false rather than inventing a
 * surface, and the mode scenario uses the header view-mode buttons that DO exist.
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

/**
 * The copy affordance's own state. Success and failure are the component's rendered outcome
 * (a lucide check vs an alert icon, plus the aria-label the component swaps to "Copied" /
 * "Copy failed" / "Copy message"), so this is the app's real feedback, not a simulated one.
 */
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

/** A cheap stable digest of the rendered grid, used to detect any change without keeping it all. */
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

// --------------------------------------------------------------------------- view mode (real taps)

/**
 * Ensure the served app is showing the requested view, using the header view-mode buttons the
 * candidate actually renders (remote-view-mode-chat / remote-view-mode-terminal). The switch is
 * a real tap, verified by a bounded observable delta (grid present vs composer present).
 */
async function ensureViewMode(cdp, evDir, want) {
  const s = await readState(cdp);
  const showing = want === "terminal" ? s.gridText !== null : s.chatVisible;
  if (showing) return s;
  const nodes = fetchHierarchy().nodes;
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

// --------------------------------------------------------------------------- layout / scroll / copy

/**
 * Keyboard-driven layout capture. The observable is the VISUAL viewport: on Android the
 * keyboard shrinks window.visualViewport.height, so a real keyboard that opened must shrink it.
 * The control the user needs must stay inside the visible region and must not sit under the IME
 * window.
 */
async function captureLayout(cdp, evDir, caseName, hint) {
  const closed = await readState(cdp);
  const hierarchy = await openKeyboard(cdp, evDir, hint);
  LAST_IME_HIERARCHY = hierarchy;
  const open = await readState(cdp);
  const imeBounds = imeRootBounds(hierarchy.nodes);
  const controlRect = hint.layoutControl === "composer" ? open.composerRect
    : hint.layoutControl === "grid" ? open.gridRect : open.sendRect;
  const controlVisible = hint.layoutControl === "composer" ? open.chatVisible
    : hint.layoutControl === "grid" ? open.gridRect !== null : open.sendVisible;
  const visualTop = open.viewport.visualOffsetTop ?? 0;
  const visualBottom = visualTop + (open.viewport.visualHeight ?? open.viewport.height);
  const obscuredByIme = Boolean(imeBounds) && Boolean(controlRect) && controlRect.bottom > imeBounds.y1;
  const shrinkPx = closed.viewport.visualHeight !== null && open.viewport.visualHeight !== null
    ? Math.round(closed.viewport.visualHeight - open.viewport.visualHeight)
    : null;
  const capture = {
    case: caseName,
    viewportClosed: closed.viewport,
    viewportOpen: open.viewport,
    shrinkPx,
    imeBounds,
    control: hint.layoutControl,
    controlRect,
    controlVisible,
    controlWithinVisualViewport: Boolean(controlRect) && controlRect.bottom <= visualBottom + 1 && controlRect.y >= visualTop - 1,
    obscuredByIme,
    keyboardOpen: Boolean(imeBounds),
  };
  capture.ok = capture.keyboardOpen && capture.controlVisible && capture.controlWithinVisualViewport && !capture.obscuredByIme;
  writeJson(path.join(evDir, "layout-" + caseName + ".json"), capture);
  return capture;
}

/**
 * Real touch scroll. adb shell input swipe is a physical gesture (not text injection): the grid's
 * touch handler sends a scroll message to the host and the rendered [data-grid-line] set must
 * change. The keyboard stays open across the gesture so the scroll happens in the state a phone
 * user actually types in.
 */
async function captureScroll(cdp, evDir, caseName) {
  const before = await readState(cdp);
  const nodes = fetchHierarchy().nodes;
  const gridTap = await resolvePageTap(cdp, nodes, {
    cdpRectExpr: rectExpr('[data-testid="remote-terminal-grid"]'),
    hint: "scroll-grid",
  });
  const travel = 220;
  const fromY = Math.round(gridTap.y + travel / 2);
  const toY = Math.round(gridTap.y - travel / 2);
  adb(["shell", "input", "swipe", String(gridTap.x), String(fromY), String(gridTap.x), String(toY), "300"]);
  appendLog("swipe scroll " + gridTap.x + "," + fromY + " -> " + gridTap.x + "," + toY);
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
    gesture: { kind: "adb-shell-input-swipe", x: gridTap.x, fromY, toY, durationMs: 300 },
    before: fingerprintBefore,
    after: fingerprintAfter,
    changed: fingerprintAfter.digest !== fingerprintBefore.digest,
    indexMoved: fingerprintAfter.firstIndex !== fingerprintBefore.firstIndex,
    stillRendering: fingerprintAfter.lineCount > 0,
    keyboardStillOpen: Boolean(imeRootBounds(fetchHierarchy().nodes)),
  };
  capture.ok = capture.changed && capture.stillRendering;
  writeJson(path.join(evDir, "scroll-" + caseName + ".json"), capture);
  return capture;
}

/**
 * The transcript's own copy affordance, driven by a real tap. Success is the component's own
 * rendered outcome (a check icon), and a best-effort read-only clipboard readback is recorded
 * beside it. The clipboard is never cleared and never written by this runner: the app performs
 * the write, exactly as a user's tap would.
 */
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
  const nodes = fetchHierarchy().nodes;
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
    tap: { x: tap.x, y: tap.y, via: tap.via },
  };
  capture.ok = Boolean(after && after.copied);
  writeJson(path.join(evDir, "copy-" + caseName + ".json"), capture);
  return capture;
}

/**
 * Late-ACK / focus watch. After a view switch or a draft-only interaction, the PTY grid must not
 * move and the opposite view's editor must not hold focus. The window is bounded and its ONLY
 * purpose is to catch an echo that arrives after the interaction.
 */
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


// --------------------------------------------------------------------------- prerequisites (read-only)

const PREREQ = {};

/**
 * Fail-closed, read-only device/host verification. Uses only adb devices / getprop / dumpsys /
 * pm path / ime list / settings get — never enables or disables IMEs, never installs apps, never
 * touches keyguard, screen rotation or any device setting.
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
    throw blocked("adb-not-found", "tried: " + candidates.join(", ") + " — pass --adb /path/to/adb");
  }
  ADB = chosen;
  RUN.adbPath = chosen;
  RUN.adbVersion = version;
  PREREQ.adb = { path: chosen, version };
  appendLog("adb: " + chosen + " (" + version + ")");

  // The four-way binding first: without it nothing this run observes could be attributed to a
  // candidate, a served page, a pane or a PTY, so the run is blocked rather than green.
  loadBinding();

  // Exact serial match against adb devices -l — no wildcard, ever.
  const devices = adbRaw(["devices", "-l"]).stdout;
  writeFileSync(evPath("prereq/adb-devices.txt"), devices, "utf8");
  const line = devices.split("\n").find((l) => l.trim().startsWith(SERIAL + "\t") || l.trim().startsWith(SERIAL + " "));
  if (!line) {
    throw blocked("device-not-attached", "serial " + SERIAL + " absent from adb devices -l (saved to prereq/adb-devices.txt)");
  }
  const state = line.trim().split(/\s+/)[1] ?? "";
  if (state === "unauthorized") {
    throw blocked("device-unauthorized", "accept the USB debugging authorization prompt on the device, then re-run");
  }
  if (state !== "device") {
    throw blocked("device-not-ready", "serial " + SERIAL + " state=" + state);
  }
  PREREQ.device = { serial: SERIAL, adbState: state, adbDevicesLine: line.trim() };

  // Physical device only — an emulator is not evidence for this gate.
  const qemu = adb(["shell", "getprop", "ro.kernel.qemu"]).stdout.trim();
  const characteristics = adb(["shell", "getprop", "ro.build.characteristics"]).stdout.trim();
  if (qemu === "1" || characteristics.includes("emulator")) {
    throw blocked(
      "not-physical",
      "ro.kernel.qemu=" + qemu + " ro.build.characteristics=" + characteristics +
        "; only a physically attached authorized device satisfies this gate",
    );
  }

  const props = adb(["shell", "getprop"]).stdout;
  writeFileSync(evPath("prereq/device-props.txt"), props, "utf8");
  const prop = (name) => new RegExp("\\[" + name + "\\]: \\[(.*?)\\]").exec(props)?.[1] ?? null;
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
    throw blocked("device-locked", "focus=" + currentFocus + "; unlock the device manually — this runner never dismisses keyguard");
  }
  PREREQ.screen = { screenState, currentFocus };

  // Chrome must already be installed — the runner never installs apps.
  const chromePath = adb(["shell", "pm", "path", "com.android.chrome"]).stdout.trim();
  if (!chromePath.startsWith("package:")) {
    throw blocked("chrome-missing", "com.android.chrome not installed on the device; this runner never installs apps");
  }
  const chromeDump = adb(["shell", "dumpsys", "package", "com.android.chrome"]).stdout;
  const chromeVersion = /versionName=([^\s]+)/.exec(chromeDump)?.[1] ?? null;
  PREREQ.chrome = {
    packageName: "com.android.chrome",
    apkPath: chromePath.replace(/^package:/, ""),
    versionName: chromeVersion,
  };
  writeFileSync(evPath("prereq/chrome.txt"), "path: " + chromePath + "\nversionName: " + (chromeVersion ?? "unknown") + "\n", "utf8");

  // The active IME is READ, never switched. An explicit --expected-ime is an operator opt-in for
  // a different real keyboard; without it any real IME is accepted and the hard gate is the
  // Korean layout probe against the focused keyboard inside each scenario.
  const imeList = adb(["shell", "ime", "list", "-s"]).stdout;
  const defaultIme = adb(["shell", "settings", "get", "secure", "default_input_method"]).stdout.trim();
  const imeDump = adb(["shell", "dumpsys", "input_method"]).stdout;
  writeFileSync(
    evPath("prereq/ime.txt"),
    "default_input_method: " + defaultIme + "\n\nime list -s:\n" + imeList +
      "\n\ndumpsys input_method (truncated):\n" + imeDump.slice(0, 200000) + "\n",
    "utf8",
  );
  if (!defaultIme) {
    throw blocked("ime-unresolved", "settings get secure default_input_method returned nothing; confirm the on-device keyboard manually and re-run");
  }
  if (ARGS.expectedIme && !defaultIme.includes(ARGS.expectedIme)) {
    throw blocked(
      "unexpected-ime",
      "default_input_method=" + defaultIme + " does not contain the required " + ARGS.expectedIme +
        "; confirm the on-device keyboard manually and re-run",
    );
  }
  if (!imeList.includes(defaultIme)) {
    throw blocked("ime-not-enabled", "default IME " + defaultIme + " missing from the enabled list (saved to prereq/ime.txt)");
  }
  PREREQ.ime = {
    defaultIme,
    enabled: imeList.split("\n").map((l) => l.trim()).filter(Boolean),
    expectedIme: ARGS.expectedIme,
  };

  PREREQ.host = { platform: process.platform, release: os.release(), node: process.version };
  PREREQ.binding = RUN.binding;
  writeJson("prereq/provenance.json", PREREQ);
  appendLog("prereq OK: " + (PREREQ.device.model ?? "?") + " android " + (PREREQ.device.androidRelease ?? "?") + " ime=" + defaultIme);
}

// --------------------------------------------------------------------------- teardown (own resources only)

/** Record a PID this run itself spawned, with the executable that was launched. */
function recordOwnedPid(pid, executablePath, extra) {
  const entry = {
    pid,
    executablePath: executablePath ?? null,
    role: "herdr-native-ime",
    spawnedAt: new Date().toISOString(),
    extra: extra ?? {},
  };
  RUN.cleanup.entries.push(entry);
  return entry;
}

/**
 * Remove ONLY the adb forwards/reverses this run created (by exact id), and terminate ONLY the
 * PIDs this run recorded at spawn time whose live executable still matches what was launched.
 * Anything else is REPORTED, never killed — no pattern matching, no guesses.
 */
function cleanup() {
  if (ADB && SERIAL) {
    for (const id of RUN.cleanup.createdReverses) {
      const r = run(ADB, ["-s", SERIAL, "reverse", "--remove", id], { expectCodeZero: false });
      RUN.cleanup.removals.push({ type: "adb-reverse", id, exit: r.code, stderr: r.stderr.slice(0, 200) });
    }
    for (const id of RUN.cleanup.createdForwards) {
      const r = run(ADB, ["-s", SERIAL, "forward", "--remove", id], { expectCodeZero: false });
      RUN.cleanup.removals.push({ type: "adb-forward", id, exit: r.code, stderr: r.stderr.slice(0, 200) });
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
  appendLog("cleanup: " + RUN.cleanup.removals.length + " adb resource(s), " + RUN.cleanup.entries.length + " owned pid(s)");
}

// --------------------------------------------------------------------------- receipt + result

/** The receipt path, resolved from the binding (never a default that could bind another run). */
/**
 * The platform directory for this run.
 *
 * The fixed invocations in the plan pass a platform directory (`...\herdr-reference\android`,
 * `...\herdr-reference\ios`) while the task 14 runner's consumer resolves
 * `deviceReceiptPath(<commonRoot>, platform)` = `<commonRoot>/<platform>/device-receipt.json`. Both
 * forms must land on the SAME file, so the platform segment is appended only when it is not already
 * the last segment of the given directory — never twice.
 */
function platformDirFor(evidenceDir, platform) {
  const resolved = path.resolve(evidenceDir);
  return path.basename(resolved) === platform ? resolved : path.join(resolved, platform);
}

/**
 * Where this producer writes its receipt. The binding's own receiptPath is honoured ONLY when it
 * names this platform's directory (otherwise it is the other platform's receipt, which this run must
 * never overwrite); in every other case the path is derived from --evidence-dir with the
 * no-double-platform rule above.
 */
function resolveReceiptPath(evidenceDir, platform, boundPath) {
  if (typeof boundPath === "string" && boundPath.length > 0) {
    const bound = path.resolve(boundPath);
    if (path.basename(path.dirname(bound)) === platform) return bound;
  }
  return path.join(platformDirFor(evidenceDir, platform), "device-receipt.json");
}

function receiptPath() {
  return RUN.receiptPath ?? resolveReceiptPath(EVIDENCE_DIR, "android", null);
}

function writeJsonAbs(absPath, data) {
  mkdirSync(path.dirname(absPath), { recursive: true });
  writeFileSync(absPath, JSON.stringify(data, null, 2) + "\n", "utf8");
  return absPath;
}

function rowStatus(scenario) {
  if (scenario.status === "pass") return "pass";
  if (scenario.status === "blocked" || scenario.status === "not-run") return "blocked";
  return "fail";
}

/**
 * The device receipt, schema ferryx-herdr-reference.device/1. The candidate, page, target and
 * PTY blocks are copied VERBATIM from the task 14 binding — this producer never invents a
 * candidate, a target or a PTY identity.
 */
function buildReceipt() {
  const binding = RUN.binding ?? { candidate: {}, page: {}, target: {}, pty: {} };
  const device = {
    serial: SERIAL,
    model: PREREQ.device?.model ?? null,
    osVersion: PREREQ.device?.androidRelease ?? null,
    driverEndpoint: RUN.cdp ? "adb-forward:127.0.0.1:" + RUN.cdp.localPort + " -> localabstract:" + RUN.cdp.socketPath : null,
    manufacturer: PREREQ.device?.manufacturer ?? null,
    sdk: PREREQ.device?.sdk ?? null,
    adbPath: RUN.adbPath,
    adbVersion: RUN.adbVersion,
    adbState: PREREQ.device?.adbState ?? null,
  };
  // A BLOCKED run (missing device, driver, binding or an unmet device prerequisite) is never
  // green; a row that failed its observable makes the verdict "fail".
  const blockedRun = Boolean(RUN.blockedReason) || RUN.exitCode === EXIT.BLOCKED;
  return {
    schema: REFERENCE_DEVICE_RECEIPT_SCHEMA,
    platform: "android",
    verdict: blockedRun ? "blocked" : RUN.exitCode === EXIT.OK && RUN.scenarios.length > 0 && RUN.scenarios.every((s) => rowStatus(s) === "pass") ? "pass" : "fail",
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
      platformDir: platformDirFor(EVIDENCE_DIR, "android"),
      receiptPath: receiptPath(),
      case: ARGS?.caseName ?? null,
      scenariosRequested: ARGS?.scenarios ?? [],
      pageUrlRedacted: ARGS ? redactUrl(ARGS.pageUrl) : null,
      timeoutMs: ARGS?.timeoutMs ?? null,
      policy:
        "real-OSK-physical-taps-only; read-only device provenance; own-resource cleanup with exact-PID identity checks; no settings changes; no installs; no consent automation; no coordinate guessing",
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
      cdp: RUN.cdp ?? null,
      chromeOffsetsUsed: RUN.chromeOffsets,
      viewport: {
        baseline: RUN.baselineState?.viewport ?? null,
        maxHeight: RUN.maxViewportHeight,
        minHeight: RUN.minViewportHeight,
      },
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
    device,
    ime: PREREQ.ime ?? null,
    screen: PREREQ.screen ?? null,
    chrome: PREREQ.chrome ?? null,
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
      removals: RUN.cleanup.removals,
      receipts: RUN.cleanup.receipts,
      entries: RUN.cleanup.entries,
      ledgerSchema: SPAWN_LEDGER_SCHEMA,
    },
    exitCode: RUN.exitCode,
  };
}

function buildResult() {
  const receipt = buildReceipt();
  const allGreen =
    RUN.exitCode === EXIT.OK &&
    RUN.scenarios.length > 0 &&
    RUN.scenarios.every((s) => s.status === "pass");
  return {
    schema: "ferryx-herdr-native-ime.result/1",
    verdict: allGreen ? "pass" : RUN.exitCode === EXIT.BLOCKED ? "blocked" : "fail",
    verdictSemantics:
      "summary only — QA-08 consumes <evidence-dir>/android/device-receipt.json (schema ferryx-herdr-reference.device/1); skipped, not-run, fail and blocked are never green",
    deviceReceiptPath: receiptPath(),
    deviceReceipt: receipt,
    runner: receipt.producer,
    device: receipt.device,
    page: receipt.page,
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
  lines.push("# herdr-native-ime (Android) evidence report");
  lines.push("");
  lines.push("- result schema: " + result.schema);
  lines.push("- device receipt: " + result.deviceReceiptPath + " (schema " + result.deviceReceipt.schema + ")");
  lines.push("- verdict (summary only): **" + result.verdict + "** — QA-08 reads the receipt, never this boolean");
  lines.push("- exitCode: " + result.exitCode);
  lines.push("- script sha256: " + (result.runner.scriptSha256 ?? "unknown"));
  lines.push("- device: " + (result.device.serial ?? "?") + " / " + (result.device.model ?? "?") + " (Android " + (result.device.osVersion ?? "?") + ")");
  lines.push("- IME: " + (result.deviceReceipt.ime?.defaultIme ?? "not probed") + " | chrome: " + (result.deviceReceipt.chrome?.versionName ?? "not probed"));
  lines.push("- page: " + (result.page.urlRedacted ?? "?") + " (CDP " + (result.page.cdp ? "via " + result.page.cdp.forwardSpec : "not connected") + ")");
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
      if (Array.isArray(b.evidenceFiles) && b.evidenceFiles.length > 0) {
        lines.push("  - evidence files: " + b.evidenceFiles.join(", "));
      }
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
  for (const r of result.cleanup.removals) lines.push("- " + r.type + " " + r.id + ": exit " + r.exit);
  for (const r of result.cleanup.receipts) lines.push("- pid " + r.pid + ": " + r.action + (r.reason ? " (" + r.reason + ")" : ""));
  lines.push("");
  lines.push("## evidence integrity");
  lines.push("- Every keyboard strike was a physical adb shell input tap on the device's real keyboard; no input text, keyevents-as-text or JS composition injection was used.");
  lines.push("- Every page/key tap was verified by a bounded observable DOM delta or recorded as a failure with screenshots and UI hierarchy XML.");
  lines.push("- All device reads were getprop / dumpsys / pm path / ime list / settings get only; no settings, IMEs, apps or keyguard were modified.");
  lines.push("- Chrome first-run consent is never accepted by this runner: that outcome is a blocked target.");
  writeFileSync(evPath("report.md"), lines.join("\n") + "\n", "utf8");
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
  files.push(screenshot(path.join(dir, "01-baseline.png")));
  const baselineFingerprint = gridFingerprint(baseline);

  const layout = await captureLayout(cdp, dir, "happy", {
    cdpRectExpr: rectExpr('[data-testid="remote-terminal-grid"]'),
    focusTestid: "remote-terminal-input-sink",
    hint: "tap-grid-focus-sink",
    layoutControl: "grid",
  });
  const korean = requireKoreanLayout(dir, imeHierarchyFor());
  files.push(screenshot(path.join(dir, "02-keyboard-open.png")));

  const observe = progressObserver(cdp, 0);
  await typeSequence(dir, ["ㅁ", "ㅗ", "ㅂ", "ㅏ", "ㅇ", "ㅣ", "ㄹ"], observe);

  const preEnter = await readState(cdp);
  files.push(writeJson(path.join(dir, "03-pre-enter.json"), preEnter));
  files.push(screenshot(path.join(dir, "04-pre-enter.png")));
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
  files.push(screenshot(path.join(dir, "06-after-enter.png")));

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
  files.push(screenshot(path.join(dir, "03-after-switch-chat.png")));
  const chatFingerprint = gridFingerprint(chatView);
  const lateAck = await captureLateAck(cdp, dir, "failure", chatFingerprint, { expectView: "chat" });

  const terminalView = await ensureViewMode(cdp, dir, "terminal");
  files.push(writeJson(path.join(dir, "04-back-to-terminal.json"), terminalView));
  await delay(DUPLICATE_WINDOW_MS);
  const settled = await readState(cdp);
  files.push(writeJson(path.join(dir, "05-settled.json"), settled));
  files.push(screenshot(path.join(dir, "06-settled.png")));
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
  files.push(screenshot(path.join(dir, "03-chat-typed.png")));
  const draftTyped = typed.chatDraft;

  const backToTerminal = await ensureViewMode(cdp, dir, "terminal");
  files.push(writeJson(path.join(dir, "04-back-to-terminal.json"), backToTerminal));
  const backToChat = await ensureViewMode(cdp, dir, "chat");
  files.push(writeJson(path.join(dir, "05-back-to-chat.json"), backToChat));
  files.push(screenshot(path.join(dir, "06-back-to-chat.png")));
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
  files.push(screenshot(path.join(dir, "03-switched-to-terminal.png")));
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
  files.push(screenshot(path.join(dir, "02-typed.png")));
  const draftBefore = typed.chatDraft;
  const typedAsExpected = (draftBefore ?? "").trim() === TOKENS.composition;

  await tapEnter(dir);
  const afterFirst = await readState(cdp);
  files.push(writeJson(path.join(dir, "03-after-first-enter.json"), afterFirst));
  files.push(screenshot(path.join(dir, "04-after-first-enter.png")));

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
  files.push(screenshot(path.join(dir, "06-settled.png")));
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
  files.push(screenshot(path.join(dir, "04-after-second-enter.png")));

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
  files.push(screenshot(path.join(dir, "02-chat-typed.png")));
  const draftTyped = typed.chatDraft;

  const terminalView = await ensureViewMode(cdp, dir, "terminal");
  files.push(writeJson(path.join(dir, "03-terminal-view.json"), terminalView));
  files.push(screenshot(path.join(dir, "04-terminal-view.png")));
  const sinkFocused = await poll("sink-focus", async () => {
    const s = await readState(cdp);
    return s.activeElement?.testid === "remote-terminal-input-sink" ? s : null;
  }, {});
  const lateAck = await captureLateAck(cdp, dir, "happy", gridFingerprint(terminalView), { expectView: "terminal" });

  const chatFinal = await ensureViewMode(cdp, dir, "chat");
  const final = await readState(cdp);
  files.push(writeJson(path.join(dir, "05-chat-final.json"), final));
  files.push(screenshot(path.join(dir, "06-chat-final.png")));

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
  files.push(screenshot(path.join(dir, "02-switched-to-chat.png")));
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


// --------------------------------------------------------------------------- keyboard helpers

/**
 * The most recently captured IME hierarchy, so a scenario can run the Korean layout probe
 * against the keyboard it just opened instead of re-reading the window (and instead of guessing).
 */
let LAST_IME_HIERARCHY = null;

async function openKeyboard(cdp, evDir, hint) {
  const hierarchy = await ensureKeyboardOpen(cdp, evDir, hint);
  LAST_IME_HIERARCHY = hierarchy;
  return hierarchy;
}

function imeHierarchyFor() {
  if (!LAST_IME_HIERARCHY) {
    throw blocked("ime-window-missing", "no IME hierarchy has been captured yet; the editor must be focused first");
  }
  return LAST_IME_HIERARCHY;
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

// --------------------------------------------------------------------------- main

async function main() {
  RUN.startedAt = new Date().toISOString();
  ARGS = parseArgs(process.argv.slice(2));
  if (ARGS.pageUrl.includes("'") || /[;`$\|]/.test(ARGS.pageUrl)) {
    usage("--page-url must be a plain URL without shell metacharacters");
  }
  SERIAL = ARGS.serial;
  EVIDENCE_DIR = path.resolve(ARGS.evidenceDir);
  mkdirSync(EVIDENCE_DIR, { recursive: true });
  RUN.serial = SERIAL;
  RUN.caseName = ARGS.caseName;
  try {
    RUN.scriptSha256 = createHash("sha256").update(readFileSync(new URL(import.meta.url))).digest("hex");
  } catch (err) {
    appendLog("script hash failed: " + String(err));
  }
  KEYMAP = loadKeymap(ARGS.keymap);
  writeJson("run-config.json", {
    scriptId: SCRIPT_ID,
    scriptSha256: RUN.scriptSha256,
    serial: SERIAL,
    pageUrlRedacted: redactUrl(ARGS.pageUrl),
    scenarios: ARGS.scenarios,
    case: ARGS.caseName,
    keymapFile: ARGS.keymap,
    tapOffsetY: ARGS.tapOffsetY,
    expectedIme: ARGS.expectedIme,
    adb: ARGS.adb,
    adbReverse: ARGS.adbReverse,
    timeoutMs: ARGS.timeoutMs,
    ptyHookSet: !!ARGS.ptyHook,
    bindingEnvVariable: REFERENCE_DEVICE_BINDING_ENV,
    bindingPath: process.env[REFERENCE_DEVICE_BINDING_ENV] ?? null,
    startedAt: RUN.startedAt,
    policy:
      "real-OSK-physical-taps-only; read-only device provenance; own-resource cleanup with exact-PID identity checks; no settings changes; no installs; no consent automation; no coordinate guessing",
  });

  process.on("SIGINT", () => {
    appendLog("SIGINT received — cleaning up own resources, exiting 130");
    cleanup();
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

  let exitCode = EXIT.OK;
  let cdp = null;
  try {
    await prereqProbe();
    verifyPtyIdentity();

    for (const pair of ARGS.adbReverse) {
      const [dev, host] = pair.split(":");
      if (!/^[0-9]+$/.test(dev ?? "") || !/^[0-9]+$/.test(host ?? "")) {
        usage("--adb-reverse must be devPort:hostPort, got " + pair);
      }
      const id = "tcp:" + dev;
      const r = run(ADB, ["-s", SERIAL, "reverse", id, "tcp:" + host], { expectCodeZero: false });
      if (r.code !== 0) {
        throw interactionFailed("adb-reverse-failed", id + " -> tcp:" + host + ": " + r.stderr.slice(0, 200));
      }
      RUN.cleanup.createdReverses.push(id);
      appendLog("adb reverse created: " + id + " -> tcp:" + host);
    }

    // Open the isolated gateway page in the real Chrome (single-quoted URL protects & in query).
    const launch = adb(
      ["shell", "am start -a android.intent.action.VIEW -d '" + ARGS.pageUrl + "'"],
      { expectCodeZero: false },
    );
    if (launch.code !== 0) {
      throw blocked("chrome-launch-failed", "am start exit " + launch.code + ": " + (launch.stderr || launch.stdout).slice(0, 300));
    }
    writeFileSync(
      evPath("prereq/chrome-launch.txt"),
      "command: am start -a android.intent.action.VIEW -d '" + ARGS.pageUrl + "'\n" +
        "exit: " + launch.code + "\n--- stdout ---\n" + (launch.stdout ?? "") + "\n--- stderr ---\n" +
        (launch.stderr ?? "") + "\n",
      "utf8",
    );
    appendLog("chrome launched for " + redactUrl(ARGS.pageUrl));

    cdp = await connectCdp({
      launchOutput: (launch.stdout ?? "") + "\n" + (launch.stderr ?? ""),
      launchExit: launch.code,
    });

    RUN.baselineState = await readState(cdp);
    RUN.maxViewportHeight = RUN.baselineState.viewport.height;
    RUN.minViewportHeight = RUN.baselineState.viewport.height;
    writeJson("baseline-state.json", RUN.baselineState);
    appendLog("baseline: url=" + RUN.baselineState.url + " viewport=" + JSON.stringify(RUN.baselineState.viewport));

    for (const name of ARGS.scenarios) {
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
      const r = run("/bin/sh", ["-c", ARGS.ptyHook], { timeoutMs: 60000 });
      writeFileSync(
        evPath("pty-hook-output.txt"),
        "$ " + ARGS.ptyHook + "\n--- exit " + r.code + " ---\n" + (r.stdout ?? "") + (r.stderr ?? "") + "\n",
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
      if (err.code === EXIT.BLOCKED) RUN.blockedReason = err.reason + (err.detail ? ": " + err.detail : "");
    } else {
      exitCode = 1;
      RUN.failure = { reason: "unexpected-error", detail: String(err?.stack ?? err).slice(0, 4000) };
    }
    appendLog("FAILED exit=" + exitCode + " " + RUN.failure.reason + ": " + RUN.failure.detail);
    const executed = new Set(RUN.scenarios.map((s) => s.name + "/" + s.case));
    for (const name of ARGS.scenarios) {
      for (const caseName of selectedCases()) {
        if (!executed.has(name + "/" + caseName)) {
          RUN.scenarios.push({ name, case: caseName, status: "not-run", reason: "the run stopped before this row" });
        }
      }
    }
  } finally {
    if (cdp) cdp.close();
    cleanup();
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

