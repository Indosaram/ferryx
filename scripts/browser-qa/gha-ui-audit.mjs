#!/usr/bin/env bun
// Real-OS-input UI audit for the Ferryx built-in browser, run on GitHub-hosted runners.
//
// Launches the debug executable with isolated FERRYX_* directories, drives it only through the
// host's own input injection (Linux xdotool, macOS osascript/cliclick, Windows SendInput), finds
// on-screen labels with tesseract OCR so clicks land on real controls, and captures a full-screen
// screenshot after every step. Each step's outcome and its visible-text check go to summary.json,
// and the script exits 0 once it has started. The job then fails on any false entry of a `checks`
// object outside `steps` unless the object's `informational` list names it
// (scripts/browser-qa/check-summary.mjs).
//
// Chromium engine checks (every OS): set FERRYX_UI_AUDIT_ENGINE_ZIP to the engine archive the
// app installs on this platform (engine-linux-<arch>.zip, engine-macos-aarch64.zip with
// ferryx_engine.app at its root, engine-windows-x86_64.zip with ferryx_engine.exe at its root).
// The script serves it with a manifest signed by a throwaway ed25519 key that only debug builds
// accept (FERRYX_BROWSER_ENGINE_PUBLIC_KEY), installs it from Settings > Browser, opens a
// Chromium tab, types into a fixture page and kills the engine. The engine's window is a child
// X window on Linux, a separate app's window on macOS and a child HWND owned by another process
// on Windows; keyboard focus and the engine processes are read per platform.
//
// Extension scenario (needs the engine and network): installs uBlock Origin Lite from the Chrome
// Web Store in an engine tab (the app has no other install path: Settings > Browser > Extensions
// only links to the store), removes it with the trash button of its row in Settings > Browser >
// Extensions, restarts the app and checks it stays removed. Results go to summary.extension.
//
// Agent automation: `ferryx browser snapshot` and act commands (fill, click with an
// --expect-text postcondition, a click with a ref from before a reload) run against a local
// agent fixture page in the system webview tab on every OS and, when the engine is installed,
// in the Chromium engine tab; outcomes go to summary.agent.<engine>.
//
// Usage: bun scripts/browser-qa/gha-ui-audit.mjs --binary <path> --out <dir>
//        [--scenario full|drag-mask]
//
// --scenario drag-mask (needs FERRYX_UI_AUDIT_ENGINE_DIR, an unpacked engine dir, passed to the
// app as FERRYX_CEF_ENGINE_DIR so new browser tabs open on Chromium): opens a browser tab on the
// fixture page, drags that tab in the tab bar with real OS mouse input and checks from the
// screenshot pixels that the engine's native window is hidden mid-drag and shown again after
// the drop. Results go to summary.drag_mask; none of the full flow runs.
import { spawn, spawnSync } from "node:child_process";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { existsSync, mkdirSync, openSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { inflateSync } from "node:zlib";

const argv = process.argv.slice(2);
const option = (name) => {
  const index = argv.indexOf(`--${name}`);
  return index >= 0 ? argv[index + 1] : undefined;
};
const platform = process.platform; // linux | darwin | win32
const binary = resolve(option("binary") ?? "");
const out = resolve(option("out") ?? "ui-audit");
const scenario = option("scenario") ?? "full";
if (!["full", "drag-mask"].includes(scenario)) throw new Error(`unknown --scenario ${scenario} (full | drag-mask)`);
const shots = join(out, "screenshots");
const ocrDir = join(out, "ocr");
mkdirSync(shots, { recursive: true });
mkdirSync(ocrDir, { recursive: true });

// One isolation dir per scenario: two invocations in one job must not share FERRYX_* dirs.
const isolation = join(process.env.RUNNER_TEMP ?? out, scenario === "full" ? "ferryx-ui-audit-isolation" : `ferryx-ui-audit-isolation-${scenario}`);
const dirs = Object.fromEntries(["data", "runtime", "session", "tmp", "home"].map((name) => [name, join(isolation, name)]));
for (const dir of Object.values(dirs)) mkdirSync(dir, { recursive: true });

// The Settings install flow is part of the full scenario only; drag-mask uses a local engine dir.
const engineZip = scenario === "full" && process.env.FERRYX_UI_AUDIT_ENGINE_ZIP && existsSync(process.env.FERRYX_UI_AUDIT_ENGINE_ZIP)
  ? resolve(process.env.FERRYX_UI_AUDIT_ENGINE_ZIP)
  : null;
// The app's engine platform key and executable (platform_key in engine_install.rs, ENGINE_EXE in
// cef_engine/{linux,macos,windows}.rs).
const enginePlatformKey = platform === "darwin"
  ? "macos-aarch64"
  : platform === "win32"
    ? "windows-x86_64"
    : `linux-${process.arch === "arm64" ? "aarch64" : "x86_64"}`;
const ENGINE_EXE = { linux: "ferryx_engine", darwin: "ferryx_engine.app/Contents/MacOS/ferryx_engine", win32: "ferryx_engine.exe" }[platform];
const dragEngineDir = scenario === "drag-mask" && process.env.FERRYX_UI_AUDIT_ENGINE_DIR ? resolve(process.env.FERRYX_UI_AUDIT_ENGINE_DIR) : null;
if (scenario === "drag-mask" && (!dragEngineDir || !existsSync(dragEngineDir))) {
  throw new Error("--scenario drag-mask needs FERRYX_UI_AUDIT_ENGINE_DIR naming an unpacked engine directory");
}

const summary = {
  platform,
  scenario,
  binary,
  commit: process.env.GITHUB_SHA ?? null,
  started_at: new Date().toISOString(),
  isolation: dirs,
  notes: [
    "New browser tab: registered shortcut tab.newBrowser (Mod+Shift+B); the command palette lists only worktrees, tabs and 'Show changes', so it has no browser-tab command.",
    "Downloads shelf: no shortcut or palette command; it renders only while a download exists, so the step navigates to a local attachment URL.",
    "Settings > Browser is hidden until General > Advanced features is on (fresh-install default is off), so the run enables it first.",
    "Controls are located from their own on-screen text: the address bar by its URL/about:blank text, Settings nav items in the column of 'General', the Advanced features switch from the row's pixels. Settings groups are reached with real mouse-wheel input: scroll until the exact heading (at the content column edge, nothing after it on the line) is read, turning around at the pane end, bounded by a step count.",
    "Page loads use a local fixture server (example.html, download.html), so results do not depend on the network.",
    "ui_check uses tesseract OCR on the step screenshot: pass = expected text visible, fail = not visible, unavailable = OCR missing.",
    "Agent automation runs with Settings > Browser > Agent access 'require pairing' off (seeded agent-authority.json), so the CLI uses the transport token only.",
  ],
  engine: engineZip
    ? {
        zip: engineZip,
        platform_key: enginePlatformKey,
        // Every engine check starts false, so a run that stops early reports what it never reached.
        checks: Object.fromEntries(["installed_from_settings", "engine_process_started", "fixture_loaded", "typing_reaches_page", "agent_automation", "engine_kill_closes_tabs"].map((name) => [name, false])),
      }
    : { skipped: scenario === "full" ? "FERRYX_UI_AUDIT_ENGINE_ZIP not set or missing" : `not part of the ${scenario} scenario` },
  drag_mask: dragEngineDir ? { engine_dir: dragEngineDir } : undefined,
  agent: {},
  steps: [],
};
const writeSummary = () => writeFileSync(join(out, "summary.json"), JSON.stringify(summary, null, 2));
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", timeout: options.timeout ?? 30000, maxBuffer: 64 * 1024 * 1024, ...options });
  return {
    ok: result.status === 0 && !result.error,
    status: result.status,
    stdout: (result.stdout ?? "").trim(),
    stderr: ((result.stderr ?? "").trim() || String(result.error ?? "")).slice(0, 2000),
  };
}

// ---------------------------------------------------------------- local fixture server
const FIXTURE = `<!doctype html><html><head><title>fixture-ready</title><style>
html,body{margin:0;height:100%;background:#00a86b}
textarea{position:fixed;inset:40px;width:calc(100% - 80px);height:calc(100% - 80px);font:28px sans-serif;background:#ffd400;border:0}
</style></head><body><textarea id=t autofocus placeholder="Ferryx CEF fixture: type here"></textarea>
<script>t.addEventListener('input',()=>{document.title='typed:'+t.value})</script></body></html>`;
const served = new Map([
  ["/fixture.html", { body: FIXTURE, type: "text/html" }],
  ["/agent.html", {
    body: `<!doctype html><html><head><title>agent-fixture</title><style>body{font:28px sans-serif;margin:60px;background:#fff}
input,button{font:28px sans-serif;margin:12px 0;display:block}</style></head><body><h1>Agent fixture</h1>
<input id=e aria-label=Email placeholder=Email><button id=b type=button>Save</button><p id=s>idle</p>
<script>b.addEventListener('click',()=>{s.textContent='Saved '+e.value;document.title='saved:'+e.value})</script></body></html>`,
    type: "text/html",
  }],
  ["/example.html", {
    body: `<!doctype html><html><head><title>Example Domain</title><style>body{background:#eee;font:20px sans-serif;margin:0}
div{width:600px;margin:15vh auto;padding:2em;background:#fdfdff;border-radius:.5em}h1{font-size:40px}</style></head>
<body><div><h1>Example Domain</h1><p>This domain is for use in illustrative examples in documents.</p>
<p>Example text repeated for find: Example, example, EXAMPLE.</p><p><a href="/download.html">More information...</a></p></div></body></html>`,
    type: "text/html",
  }],
  ["/download.html", {
    body: `<!doctype html><title>download-fixture</title><style>html,body{margin:0;height:100%;background:#fff}
a{display:flex;align-items:center;justify-content:center;position:fixed;inset:0;font:bold 40px sans-serif;color:#000;text-decoration:none}</style>
<a id=d href="/download/ferryx-qa.txt" download="ferryx-qa.txt">FERRYXDOWNLOAD</a>`,
    type: "text/html",
  }],
  ["/download/ferryx-qa.txt", { body: "ferryx ui audit download\n", type: "text/plain", headers: { "Content-Disposition": 'attachment; filename="ferryx-qa.txt"' } }],
  ["/media.html", {
    body: `<!doctype html><title>media-fixture</title><style>html,body{margin:0;height:100%;background:#fff}
button{position:fixed;inset:0;width:100%;height:100%;font:bold 40px sans-serif;background:#fff;color:#000;border:0}</style>
<button id=m type=button>MEDIAREQUEST</button>
<script>m.addEventListener('click',()=>{document.title='media:asking';navigator.mediaDevices.getUserMedia({video:true,audio:true})
.then((stream)=>{stream.getTracks().forEach((track)=>track.stop());document.title='media:granted'})
.catch((error)=>{document.title='media:'+error.name})})</script>`,
    type: "text/html",
  }],
]);
let engineKeyPublic = null;
if (engineZip) {
  const zipBytes = readFileSync(engineZip);
  const versionMatch = /ENGINE_VERSION: &str = "([^"]+)"/.exec(
    existsSync("src-tauri/src/browser/engine_install.rs") ? readFileSync("src-tauri/src/browser/engine_install.rs", "utf8") : "",
  );
  summary.engine.version = versionMatch?.[1] ?? null;
  served.set(`/${basename(engineZip)}`, { body: zipBytes, type: "application/zip" });
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  engineKeyPublic = publicKey.export({ format: "der", type: "spki" }).subarray(-32).toString("base64");
  summary.engine.signing_key = "throwaway ed25519 generated for this run (debug-build override)";
  served.set("/__engine-key", { privateKey, zipBytes });
}
const server = Bun.serve({
  port: 0,
  hostname: "127.0.0.1",
  fetch(request) {
    const entry = served.get(new URL(request.url).pathname);
    if (!entry || entry.privateKey) return new Response("not found", { status: 404 });
    return new Response(entry.body, { headers: { "Content-Type": entry.type, ...(entry.headers ?? {}) } });
  },
});
const fixtureBase = `http://127.0.0.1:${server.port}`;
if (engineZip) {
  const { privateKey, zipBytes } = served.get("/__engine-key");
  const manifest = Buffer.from(JSON.stringify({
    version: summary.engine.version,
    platforms: {
      [enginePlatformKey]: {
        url: `${fixtureBase}/${basename(engineZip)}`,
        sha256: createHash("sha256").update(zipBytes).digest("hex"),
        size: zipBytes.length,
      },
    },
  }));
  served.set("/manifest.json", { body: manifest, type: "application/json" });
  served.set("/manifest.json.sig", { body: `${sign(null, manifest, privateKey).toString("base64")}\n`, type: "text/plain" });
  served.delete("/__engine-key");
}

// ---------------------------------------------------------------- platform: Linux (X11)
const linux = {
  windowId: null,
  findWindow(pid) {
    for (const query of [["search", "--onlyvisible", "--pid", String(pid)], ["search", "--onlyvisible", "--name", "Ferryx"]]) {
      const ids = run("xdotool", query).stdout.split(/\s+/).filter(Boolean);
      if (ids.length) return ids[ids.length - 1];
    }
    return null;
  },
  focus() {
    return this.windowId ? run("xdotool", ["windowactivate", "--sync", this.windowId]) : { ok: false, stderr: "no window" };
  },
  keyName(key) {
    return { Tab: "Tab", Enter: "Return", Escape: "Escape", Space: "space", ",": "comma", "=": "equal", "[": "bracketleft" }[key] ?? key;
  },
  key({ key, mods = [] }) {
    return run("xdotool", ["key", "--clearmodifiers", [...mods.map((mod) => (mod === "mod" ? "ctrl" : mod)), this.keyName(key)].join("+")]);
  },
  type(text) {
    return run("xdotool", ["type", "--delay", "40", text]);
  },
  click(x, y) {
    return run("xdotool", ["mousemove", "--sync", String(x), String(y), "click", "1"]);
  },
  // Left button press, move with the button held, release: a real pointer drag for dnd-kit.
  mouseDown(x, y) {
    return run("xdotool", ["mousemove", "--sync", String(x), String(y), "mousedown", "1"]);
  },
  mouseMove(x, y) {
    return run("xdotool", ["mousemove", "--sync", String(x), String(y)]);
  },
  mouseUp(x, y) {
    return run("xdotool", ["mousemove", "--sync", String(x), String(y), "mouseup", "1"]);
  },
  resize(width, height) {
    return run("xdotool", ["windowsize", "--sync", this.windowId, String(width), String(height)]);
  },
  /** Window box on the root screen (client area plus a 30px margin for the frame), or null. */
  geometry() {
    if (!this.windowId) return null;
    const out = run("xdotool", ["getwindowgeometry", "--shell", this.windowId]);
    const value = (name) => Number(new RegExp(`^${name}=(-?\\d+)`, "m").exec(out.stdout)?.[1]);
    const [x, y, width, height] = ["X", "Y", "WIDTH", "HEIGHT"].map(value);
    if (!out.ok || ![x, y, width, height].every(Number.isFinite) || width < 200 || height < 200) return null;
    const left = Math.max(0, x - 30), top = Math.max(0, y - 30);
    return { x: left, y: top, width: width + (x - left) + 30, height: height + (y - top) + 30 };
  },
  wheel(x, y, notches) {
    return run("xdotool", ["mousemove", "--sync", String(x), String(y), "click", "--repeat", String(Math.abs(notches)), "--delay", "60", notches > 0 ? "5" : "4"]);
  },
  screenshot(path) {
    const result = run("import", ["-window", "root", path]);
    return result.ok ? result : run("sh", ["-c", `xwd -root -silent | convert xwd:- png:'${path}'`]);
  },
  scale: () => 1,
};

// ---------------------------------------------------------------- platform: macOS
const macKeyCodes = { Tab: 48, Enter: 36, Escape: 53, Space: 49 };
const mac = {
  pid: null,
  pixelScale: null,
  osa(script) {
    return run("osascript", ["-e", script]);
  },
  findWindow(pid) {
    const result = this.osa(`tell application "System Events" to count windows of (first process whose unix id is ${pid})`);
    return result.ok && Number(result.stdout) > 0 ? String(pid) : null;
  },
  focus() {
    // `set frontmost` on an app that is already frontmost still brings all of its windows to the
    // front, which buries the engine's pane windows (a separate app ordered above Ferryx) without
    // a focus change that would raise them again. Only activate when Ferryx is not frontmost.
    const front = this.osa('tell application "System Events" to get unix id of first process whose frontmost is true');
    if (front.ok && Number(front.stdout) === this.pid) return { ok: true, already_frontmost: true };
    return this.osa(`tell application "System Events" to set frontmost of (first process whose unix id is ${this.pid}) to true`);
  },
  key({ key, mods = [] }) {
    const using = mods.map((mod) => ({ mod: "command down", shift: "shift down", alt: "option down", ctrl: "control down" })[mod]);
    const suffix = using.length ? ` using {${using.join(", ")}}` : "";
    const action = macKeyCodes[key] !== undefined ? `key code ${macKeyCodes[key]}${suffix}` : `keystroke "${key}"${suffix}`;
    const result = this.osa(`tell application "System Events" to ${action}`);
    if (result.ok) return result;
    const cliMods = mods.map((mod) => ({ mod: "cmd", shift: "shift", alt: "alt", ctrl: "ctrl" })[mod]);
    const named = { Tab: "tab", Enter: "return", Escape: "esc", Space: "space" }[key];
    const fallback = run("cliclick", [...(cliMods.length ? [`kd:${cliMods.join(",")}`] : []), named ? `kp:${named}` : `t:${key}`, ...(cliMods.length ? [`ku:${cliMods.join(",")}`] : [])]);
    return { ...fallback, stderr: `osascript: ${result.stderr}; cliclick: ${fallback.stderr}` };
  },
  type(text) {
    const result = this.osa(`tell application "System Events" to keystroke "${text}"`);
    return result.ok ? result : run("cliclick", [`t:${text}`]);
  },
  click(x, y) {
    const scale = this.scale();
    return run("cliclick", [`c:${Math.round(x / scale)},${Math.round(y / scale)}`]);
  },
  // cliclick dd/dm/du: button down, drag-move with the button held, button up (points, like click).
  cliMouse(command, x, y) {
    const scale = this.scale();
    return run("cliclick", [`${command}:${Math.round(x / scale)},${Math.round(y / scale)}`]);
  },
  mouseDown(x, y) {
    return this.cliMouse("dd", x, y);
  },
  mouseMove(x, y) {
    return this.cliMouse("dm", x, y);
  },
  mouseUp(x, y) {
    return this.cliMouse("du", x, y);
  },
  resize(width, height) {
    return this.osa(`tell application "System Events" to tell (first process whose unix id is ${this.pid}) to set size of window 1 to {${width}, ${height}}`);
  },
  screenshot(path) {
    return run("screencapture", ["-x", "-t", "png", path]);
  },
  wheelBinary: null,
  wheel(x, y, notches) {
    if (!this.wheelBinary) {
      const source = join(out, "mac-wheel.swift");
      const binaryPath = join(out, "mac-wheel");
      writeFileSync(source, [
        "import CoreGraphics",
        "import Foundation",
        "let a = CommandLine.arguments",
        "let point = CGPoint(x: Double(a[1])!, y: Double(a[2])!)",
        "CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: point, mouseButton: .left)?.post(tap: .cghidEventTap)",
        "usleep(100000)",
        "let lines = Int32(a[3])!",
        "for _ in 0..<abs(lines) {",
        "  CGEvent(scrollWheelEvent2Source: nil, units: .line, wheelCount: 1, wheel1: lines > 0 ? -3 : 3, wheel2: 0, wheel3: 0)?.post(tap: .cghidEventTap)",
        "  usleep(60000)",
        "}",
      ].join("\n"));
      const built = run("swiftc", ["-O", source, "-o", binaryPath], { timeout: 180000 });
      this.wheelBinary = built.ok ? binaryPath : "";
      if (!built.ok) summary.notes.push(`mac wheel helper failed to build: ${built.stderr.slice(0, 300)}`);
    }
    if (!this.wheelBinary) return { ok: false, stderr: "no wheel helper" };
    const scale = this.scale();
    return run(this.wheelBinary, [String(Math.round(x / scale)), String(Math.round(y / scale)), String(notches)]);
  },
  scale() {
    if (this.pixelScale) return this.pixelScale;
    const bounds = this.osa('tell application "Finder" to get bounds of window of desktop').stdout.split(/,\s*/).map(Number);
    const probe = join(ocrDir, "scale-probe.png");
    this.screenshot(probe);
    const width = existsSync(probe) ? readFileSync(probe).readUInt32BE(16) : 0;
    this.pixelScale = bounds.length === 4 && bounds[2] > 0 && width > 0 ? width / bounds[2] : 1;
    return this.pixelScale;
  },
};

// ---------------------------------------------------------------- platform: Windows
const winHelper = join(out, "win-input.ps1");
const winVk = { Tab: 0x09, Enter: 0x0d, Escape: 0x1b, Space: 0x20, ",": 0xbc, "=": 0xbb, "[": 0xdb, mod: 0x11, ctrl: 0x11, shift: 0x10, alt: 0x12 };
const win = {
  pid: null,
  hwnd: null,
  ps(action, extra = []) {
    return run("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", winHelper, "-Action", action, ...extra], { timeout: 60000 });
  },
  findWindow(pid) {
    const result = this.ps("find", ["-ProcessId", String(pid)]);
    const hwnd = result.stdout.split(/\s+/).pop();
    return result.ok && hwnd && hwnd !== "0" ? hwnd : null;
  },
  focus() {
    return this.ps("focus", ["-Hwnd", this.hwnd]);
  },
  key({ key, mods = [] }) {
    const codes = [...mods.map((mod) => winVk[mod]), winVk[key] ?? key.toUpperCase().charCodeAt(0)];
    return this.ps("keys", ["-Hwnd", this.hwnd, "-Codes", codes.join(",")]);
  },
  type(text) {
    return this.ps("type", ["-Hwnd", this.hwnd, "-Text", text]);
  },
  click(x, y) {
    return this.ps("click", ["-Hwnd", this.hwnd, "-Width", String(x), "-Height", String(y)]);
  },
  // Each call is a separate helper process, so moves are slower than on the other hosts; the
  // button state lives in the OS, so it stays held between calls.
  mouseDown(x, y) {
    return this.ps("down", ["-Width", String(x), "-Height", String(y)]);
  },
  mouseMove(x, y) {
    return this.ps("move", ["-Width", String(x), "-Height", String(y)]);
  },
  mouseUp(x, y) {
    return this.ps("up", ["-Width", String(x), "-Height", String(y)]);
  },
  resize(width, height) {
    return this.ps("resize", ["-Hwnd", this.hwnd, "-Width", String(width), "-Height", String(height)]);
  },
  closeDialogs() {
    const result = this.ps("closedialogs", ["-ProcessId", String(this.pid)]);
    return { ...result, closed: Number(result.stdout.split(/\s+/).pop()) || 0 };
  },
  wheel(x, y, notches) {
    return this.ps("wheel", ["-Hwnd", this.hwnd, "-Width", String(x), "-Height", String(y), "-Codes", String(notches)]);
  },
  screenshot(path) {
    return this.ps("shot", ["-Path", path]);
  },
  scale: () => 1,
};
if (platform === "win32") {
  writeFileSync(
    winHelper,
    String.raw`param([string]$Action, [int]$ProcessId = 0, [string]$Hwnd = "0", [string]$Codes = "", [string]$Text = "", [int]$Width = 0, [int]$Height = 0, [string]$Path = "")
$ErrorActionPreference = "Stop"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class FxInput {
  [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Explicit)] public struct INPUTUNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public INPUTUNION u; }
  [DllImport("user32.dll", SetLastError = true)] public static extern uint SendInput(uint n, INPUT[] inputs, int size);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, System.Text.StringBuilder name, int max);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct GUITHREADINFO { public int cbSize; public uint flags; public IntPtr hwndActive; public IntPtr hwndFocus; public IntPtr hwndCapture; public IntPtr hwndMenuOwner; public IntPtr hwndMoveSize; public IntPtr hwndCaret; public RECT rcCaret; }
  [DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint thread, ref GUITHREADINFO info);
  /** "<focus hwnd> <its process id>" of the foreground thread's keyboard focus (a child HWND of
   * another process, such as the engine's pane window, included). */
  public static string FocusOwner() {
    uint ignored;
    uint thread = GetWindowThreadProcessId(GetForegroundWindow(), out ignored);
    var info = new GUITHREADINFO(); info.cbSize = Marshal.SizeOf(typeof(GUITHREADINFO));
    if (!GetGUIThreadInfo(thread, ref info) || info.hwndFocus == IntPtr.Zero) return "0 0";
    uint pid; GetWindowThreadProcessId(info.hwndFocus, out pid);
    return ((long)info.hwndFocus).ToString() + " " + pid.ToString();
  }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  public static IntPtr FindTopWindow(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { found = h; return false; } return true; }, IntPtr.Zero);
    return found;
  }
  /** Posts WM_CLOSE to visible standard dialogs (#32770, e.g. a file picker) of a process. */
  public static int CloseDialogs(uint pid) {
    int closed = 0;
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      var name = new System.Text.StringBuilder(64); GetClassName(h, name, 64);
      if (p == pid && IsWindowVisible(h) && name.ToString() == "#32770") { PostMessage(h, 0x0010, IntPtr.Zero, IntPtr.Zero); closed++; }
      return true;
    }, IntPtr.Zero);
    return closed;
  }
  static INPUT Key(ushort vk, ushort scan, uint flags) { var i = new INPUT(); i.type = 1; i.u.ki.wVk = vk; i.u.ki.wScan = scan; i.u.ki.dwFlags = flags; return i; }
  static INPUT Mouse(uint flags) { var i = new INPUT(); i.type = 0; i.u.mi.dwFlags = flags; return i; }
  public static uint Chord(ushort[] vks) {
    var list = new System.Collections.Generic.List<INPUT>();
    foreach (var vk in vks) list.Add(Key(vk, 0, 0));
    for (int k = vks.Length - 1; k >= 0; k--) list.Add(Key(vks[k], 0, 2));
    return SendInput((uint)list.Count, list.ToArray(), Marshal.SizeOf(typeof(INPUT)));
  }
  public static uint Type(string text) {
    var list = new System.Collections.Generic.List<INPUT>();
    foreach (var c in text) { list.Add(Key(0, c, 4)); list.Add(Key(0, c, 4 | 2)); }
    return SendInput((uint)list.Count, list.ToArray(), Marshal.SizeOf(typeof(INPUT)));
  }
  public static uint Wheel(int x, int y, int notches) {
    SetCursorPos(x, y);
    var list = new System.Collections.Generic.List<INPUT>();
    for (int k = 0; k < Math.Abs(notches); k++) { var i = Mouse(0x0800); i.u.mi.mouseData = (uint)(notches > 0 ? -120 : 120); list.Add(i); }
    return SendInput((uint)list.Count, list.ToArray(), Marshal.SizeOf(typeof(INPUT)));
  }
  public static uint Click(int x, int y) {
    SetCursorPos(x, y);
    return SendInput(2, new INPUT[] { Mouse(0x0002), Mouse(0x0004) }, Marshal.SizeOf(typeof(INPUT)));
  }
  /** Cursor to (x, y), then one mouse event: 0x0002 left down, 0x0001 move, 0x0004 left up. */
  public static uint Button(int x, int y, uint flags) {
    SetCursorPos(x, y);
    return SendInput(1, new INPUT[] { Mouse(flags) }, Marshal.SizeOf(typeof(INPUT)));
  }
  public static bool Focus(IntPtr h) {
    // Already foreground: touch nothing. An Alt tap here opens the window menu or moves
    // keyboard focus off the page, which swallowed typed text in the first smoke run.
    if (GetForegroundWindow() == h) return true;
    ShowWindow(h, 9);
    // Tap Alt so the foreground lock lets this process move focus, then attach input queues.
    Chord(new ushort[] { 0x12 });
    uint fg = GetWindowThreadProcessId(GetForegroundWindow(), IntPtr.Zero);
    uint me = GetCurrentThreadId();
    AttachThreadInput(me, fg, true);
    BringWindowToTop(h);
    bool ok = SetForegroundWindow(h);
    AttachThreadInput(me, fg, false);
    return ok && GetForegroundWindow() == h;
  }
}
"@
[void][FxInput]::SetProcessDPIAware()
$handle = [IntPtr]::new([long]$Hwnd)
switch ($Action) {
  "find" { $h = [FxInput]::FindTopWindow([uint32]$ProcessId); if ($h -eq [IntPtr]::Zero) { foreach ($p in Get-Process -Name ferryx -ErrorAction SilentlyContinue) { if ($p.MainWindowHandle -ne 0) { $h = $p.MainWindowHandle } } }; Write-Output ([long]$h) }
  "focus" { if (-not [FxInput]::Focus($handle)) { Write-Output "foreground not confirmed"; exit 3 } }
  "keys" { [void][FxInput]::Focus($handle); $n = [FxInput]::Chord([uint16[]]($Codes -split "," | ForEach-Object { [uint16]$_ })); if ($n -eq 0) { exit 4 } }
  "type" { [void][FxInput]::Focus($handle); $n = [FxInput]::Type($Text); if ($n -eq 0) { exit 4 } }
  "click" { $n = [FxInput]::Click($Width, $Height); if ($n -eq 0) { exit 4 } }
  "down" { $n = [FxInput]::Button($Width, $Height, 0x0002); if ($n -eq 0) { exit 4 } }
  "move" { $n = [FxInput]::Button($Width, $Height, 0x0001); if ($n -eq 0) { exit 4 } }
  "up" { $n = [FxInput]::Button($Width, $Height, 0x0004); if ($n -eq 0) { exit 4 } }
  "closedialogs" { Write-Output ([FxInput]::CloseDialogs([uint32]$ProcessId)) }
  "focusowner" { Write-Output ([FxInput]::FocusOwner()) }
  "wheel" { $n = [FxInput]::Wheel($Width, $Height, [int]$Codes); if ($n -eq 0) { exit 4 } }
  "resize" { if (-not [FxInput]::SetWindowPos($handle, [IntPtr]::Zero, 40, 40, $Width, $Height, 0x0004)) { exit 5 } }
  "upscale" {
    # Writes <Text> (3x grayscale) and <Text>.neg.png (2x grayscale inverted): light-on-dark UI
    # text OCRs far better inverted.
    Add-Type -AssemblyName System.Drawing
    $src = [System.Drawing.Image]::FromFile($Path)
    foreach ($negate in @($false, $true)) {
      $factor = if ($negate) { 2 } else { 3 }
      $dst = New-Object System.Drawing.Bitmap ($src.Width * $factor), ($src.Height * $factor)
      $g = [System.Drawing.Graphics]::FromImage($dst)
      $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
      $rect = New-Object System.Drawing.Rectangle 0, 0, $dst.Width, $dst.Height
      if ($negate) {
        $m = New-Object System.Drawing.Imaging.ColorMatrix
        $m.Matrix00 = -0.299; $m.Matrix01 = -0.299; $m.Matrix02 = -0.299
        $m.Matrix10 = -0.587; $m.Matrix11 = -0.587; $m.Matrix12 = -0.587
        $m.Matrix20 = -0.114; $m.Matrix21 = -0.114; $m.Matrix22 = -0.114
        $m.Matrix33 = 1; $m.Matrix40 = 1; $m.Matrix41 = 1; $m.Matrix42 = 1; $m.Matrix44 = 1
        $attributes = New-Object System.Drawing.Imaging.ImageAttributes
        $attributes.SetColorMatrix($m)
        $g.DrawImage($src, $rect, 0, 0, $src.Width, $src.Height, [System.Drawing.GraphicsUnit]::Pixel, $attributes)
        $dst.Save($Text + ".neg.png", [System.Drawing.Imaging.ImageFormat]::Png)
      } else {
        $m = New-Object System.Drawing.Imaging.ColorMatrix
        $m.Matrix00 = 0.299; $m.Matrix01 = 0.299; $m.Matrix02 = 0.299
        $m.Matrix10 = 0.587; $m.Matrix11 = 0.587; $m.Matrix12 = 0.587
        $m.Matrix20 = 0.114; $m.Matrix21 = 0.114; $m.Matrix22 = 0.114
        $m.Matrix33 = 1; $m.Matrix44 = 1
        $attributes = New-Object System.Drawing.Imaging.ImageAttributes
        $attributes.SetColorMatrix($m)
        $g.DrawImage($src, $rect, 0, 0, $src.Width, $src.Height, [System.Drawing.GraphicsUnit]::Pixel, $attributes)
        $dst.Save($Text, [System.Drawing.Imaging.ImageFormat]::Png)
      }
      $g.Dispose(); $dst.Dispose()
    }
    $src.Dispose()
  }
  "shot" {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
  }
}
`,
  );
}

const host = { linux, darwin: mac, win32: win }[platform];
if (!host) throw new Error(`unsupported platform ${platform}`);

// ---------------------------------------------------------------- OCR (tesseract)
const tesseract = (() => {
  for (const candidate of ["tesseract", "C:/Program Files/Tesseract-OCR/tesseract.exe", "/opt/homebrew/bin/tesseract", "/usr/local/bin/tesseract"]) {
    if (run(candidate, ["--version"]).ok) return candidate;
  }
  return null;
})();
summary.ocr = tesseract ? { tesseract } : { unavailable: true };

let ocrIndex = 0;
const imageMagick = ["magick", "convert"].find((candidate) => run(candidate, ["-version"]).ok) ?? null;
summary.ocr.image_magick = imageMagick ?? (platform === "win32" ? "System.Drawing" : "missing: raw-only OCR, nav labels may be unreadable");
/** OCR inputs: raw, 2x, and 2x inverted grayscale (sidebar/nav labels are light on dark). */
function ocrVariants(path) {
  const x2 = path.replace(/\.png$/, ".x2.png");
  const neg = path.replace(/\.png$/, ".x2n.png");
  if (imageMagick && platform !== "win32") {
    // Linux: the 1920x1080 root shot is mostly black around the 1280x870 window, and tesseract
    // reads nothing from some of those frames (measured on run 41334976522 probes: 0 words full
    // frame, 47 lines cropped). Crop every variant to the window; words are shifted back.
    const box = platform === "linux" ? linux.geometry() : null;
    const crop = box ? ["-crop", `${box.width}x${box.height}+${box.x}+${box.y}`, "+repage"] : [];
    const base = { dx: box?.x ?? 0, dy: box?.y ?? 0 };
    const variants = [];
    if (box) {
      const cropped = path.replace(/\.png$/, ".crop.png");
      if (run(imageMagick, [path, ...crop, cropped]).ok) variants.push({ file: cropped, factor: 1, ...base });
    }
    if (!variants.length) variants.push({ file: path, factor: 1, dx: 0, dy: 0 });
    // Measured on run 41327378231 screenshots: 3x gray reads dark-on-light and Windows/macOS
    // settings rows; 2x inverted reads Linux light-on-dark nav and sidebar labels.
    if (run(imageMagick, [path, ...crop, "-colorspace", "gray", "-resize", "300%", x2]).ok) variants.push({ file: x2, factor: 3, ...base });
    if (run(imageMagick, [path, ...crop, "-colorspace", "gray", "-negate", "-resize", "200%", neg]).ok) variants.push({ file: neg, factor: 2, ...base });
    return variants;
  }
  if (platform === "win32" && win.ps("upscale", ["-Path", path, "-Text", x2]).ok) {
    return [{ file: path, factor: 1 }, { file: x2, factor: 3 }, { file: `${x2}.neg.png`, factor: 2 }].filter((variant) => existsSync(variant.file));
  }
  return [{ file: path, factor: 1 }];
}
function ocr(path) {
  if (!tesseract || !path || !existsSync(path)) return null;
  const words = [];
  let anyPass = false;
  for (const { file, factor, dx = 0, dy = 0 } of ocrVariants(path)) {
    const result = run(tesseract, [file, "-", "--psm", "11", "tsv"], { timeout: 120000 });
    if (!result.ok) continue;
    anyPass = true;
    const lines = result.stdout.split(/\r?\n/);
    const header = lines.shift()?.split("\t") ?? [];
    const col = (name) => header.indexOf(name);
    for (const cells of lines.map((line) => line.split("\t"))) {
      if (cells.length < header.length || !cells[col("text")]?.trim() || Number(cells[col("conf")]) <= 0) continue;
      words.push({
        text: cells[col("text")].trim(),
        left: Math.round(Number(cells[col("left")]) / factor) + dx,
        top: Math.round(Number(cells[col("top")]) / factor) + dy,
        width: Math.round(Number(cells[col("width")]) / factor),
        height: Math.round(Number(cells[col("height")]) / factor),
      });
    }
  }
  return anyPass ? words : null;
}

// ---------------------------------------------------------------- PNG pixels (switch detection)
/** Minimal 8-bit non-interlaced PNG decoder (RGB/RGBA/gray), enough for the three screenshot tools. */
function decodePng(path) {
  const data = readFileSync(path);
  let offset = 8;
  let width = 0, height = 0, colorType = 0, bitDepth = 0;
  const idat = [];
  while (offset < data.length) {
    const length = data.readUInt32BE(offset);
    const type = data.toString("ascii", offset + 4, offset + 8);
    const body = data.subarray(offset + 8, offset + 8 + length);
    if (type === "IHDR") {
      width = body.readUInt32BE(0);
      height = body.readUInt32BE(4);
      bitDepth = body[8];
      colorType = body[9];
    } else if (type === "IDAT") idat.push(body);
    else if (type === "IEND") break;
    offset += 12 + length;
  }
  if (bitDepth !== 8) throw new Error(`unsupported PNG bit depth ${bitDepth}`);
  const channels = { 0: 1, 2: 3, 4: 2, 6: 4 }[colorType];
  if (!channels) throw new Error(`unsupported PNG color type ${colorType}`);
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * channels;
  const pixels = Buffer.alloc(height * stride);
  for (let y = 0; y < height; y += 1) {
    const filter = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let x = 0; x < stride; x += 1) {
      const a = x >= channels ? pixels[y * stride + x - channels] : 0;
      const b = y > 0 ? pixels[(y - 1) * stride + x] : 0;
      const c = x >= channels && y > 0 ? pixels[(y - 1) * stride + x - channels] : 0;
      let value = line[x];
      if (filter === 1) value += a;
      else if (filter === 2) value += b;
      else if (filter === 3) value += (a + b) >> 1;
      else if (filter === 4) {
        const p = a + b - c;
        const pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
        value += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      pixels[y * stride + x] = value & 0xff;
    }
  }
  const rgb = (x, y) => {
    const i = y * stride + x * channels;
    return channels >= 3 ? [pixels[i], pixels[i + 1], pixels[i + 2]] : [pixels[i], pixels[i], pixels[i]];
  };
  return { width, height, rgb };
}
/**
 * The toggle switch on a settings row. A row label's OCR box centre is not the switch centre, so
 * scan every line in label.y-12..+12; on each line, take runs of pixels that differ from the
 * line's dominant colour by more than 18, merging gaps under 6px. Keep runs
 * 22-70 px wide (narrower are scrollbars/borders, wider are select boxes) whose right edge is the
 * rightmost such run on that line, then pick the run seen on the most lines. Returns its centre.
 */
function findRowSwitch(path, label, { below = 12 } = {}) {
  const image = decodePng(path);
  const votes = new Map();
  for (let dy = -12; dy <= below; dy += 1) {
    const y = Math.min(image.height - 1, Math.max(0, label.y + dy));
    // Background = the most common colour on this line (a single sample can land on a glyph).
    const counts = new Map();
    for (let x = 0; x < image.width; x += 3) {
      const key = image.rgb(x, y).map((value) => value >> 3).join(",");
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    const background = [...counts.entries()].sort((a, b) => b[1] - a[1])[0][0].split(",").map((value) => (Number(value) << 3) + 4);
    const differs = (x) => image.rgb(x, y).reduce((sum, value, index) => sum + Math.abs(value - background[index]), 0) > 24;
    const runs = [];
    let start = -1, lastHit = -10;
    for (let x = label.right + 40; x < image.width; x += 1) {
      if (!differs(x)) continue;
      if (start < 0 || x - lastHit > 6) {
        if (start >= 0) runs.push([start, lastHit]);
        start = x;
      }
      lastHit = x;
    }
    if (start >= 0) runs.push([start, lastHit]);
    const best = runs.filter(([a, b]) => b - a >= 22 && b - a <= 70).at(-1);
    if (!best) continue;
    const key = `${Math.round(best[0] / 4)}:${Math.round(best[1] / 4)}`;
    const vote = votes.get(key) ?? { count: 0, run: best, ys: [] };
    vote.count += 1;
    vote.ys.push(y);
    votes.set(key, vote);
  }
  const winner = [...votes.values()].sort((a, b) => b.count - a.count)[0];
  if (!winner || winner.count < 3) return null;
  const ys = winner.ys.sort((a, b) => a - b);
  return { x: Math.round((winner.run[0] + winner.run[1]) / 2), y: ys[Math.floor(ys.length / 2)], lines: winner.count, width: winner.run[1] - winner.run[0] };
}

const norm = (text) => text.toLowerCase().replace(/[^a-z0-9:%./]/g, "");
/**
 * The destructive (red) button on the line of `prompt` (an OCR phrase hit), right of the
 * prompt's text: the bounding box of strongly red pixels within 24px above or below the
 * prompt's centre line. Returns its centre and size, or null when no button-sized red area.
 */
function findRedButton(path, prompt) {
  const image = decodePng(path);
  const y0 = Math.max(0, prompt.y - 24), y1 = Math.min(image.height - 1, prompt.y + 24);
  let left = Infinity, right = -1, top = Infinity, bottom = -1, count = 0;
  for (let y = y0; y <= y1; y += 1) {
    for (let x = Math.round(prompt.right ?? prompt.x); x < image.width; x += 1) {
      const [r, g, b] = image.rgb(x, y);
      if (r > 170 && r - g > 80 && r - b > 80) {
        left = Math.min(left, x); right = Math.max(right, x); top = Math.min(top, y); bottom = Math.max(bottom, y);
        count += 1;
      }
    }
  }
  const width = right - left + 1, height = bottom - top + 1;
  if (count < 200 || width < 30 || height < 14) return null;
  return { x: Math.round((left + right) / 2), y: Math.round((top + bottom) / 2), width, height };
}
/**
 * The controls at the right end of a multi-line row whose first-line label is `label`: column
 * runs that differ from the row background anywhere in the band from 6px above the label to
 * 36px below it, right of the label's text. Returns the cluster at the right end (runs closer
 * than 120px to each other), e.g. pin, trash and switch of an extension row, left to right.
 */
function findRowIcons(path, label) {
  const image = decodePng(path);
  const y0 = Math.max(0, label.y - 6), y1 = Math.min(image.height - 1, label.y + 36);
  const counts = new Map();
  for (let x = 0; x < image.width; x += 3) {
    const key = image.rgb(x, Math.min(image.height - 1, label.y + 10)).map((value) => value >> 3).join(",");
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  const background = [...counts.entries()].sort((a, b) => b[1] - a[1])[0][0].split(",").map((value) => (Number(value) << 3) + 4);
  const differs = (x, y) => image.rgb(x, y).reduce((sum, value, index) => sum + Math.abs(value - background[index]), 0) > 40;
  const runs = [];
  let start = -1, last = -99;
  for (let x = label.right + 40; x < image.width; x += 1) {
    let hit = false;
    for (let y = y0; y <= y1 && !hit; y += 2) hit = differs(x, y);
    if (!hit) continue;
    if (start < 0 || x - last > 4) {
      if (start >= 0) runs.push([start, last]);
      start = x;
    }
    last = x;
  }
  if (start >= 0) runs.push([start, last]);
  // A trailing run wider than any control is the window edge or the screen outside it.
  while (runs.length && runs.at(-1)[1] - runs.at(-1)[0] > 100) runs.pop();
  // The switch is the rightmost run 26-48px wide (a scrollbar or window edge can follow it); the
  // controls left of it are the narrow icon runs before it (pin, trash), up to 60px apart.
  const switchIndex = runs.findLastIndex(([a, b]) => b - a + 1 >= 26 && b - a + 1 <= 48);
  if (switchIndex < 0) return null;
  const cluster = [runs[switchIndex]];
  for (let index = switchIndex - 1; index >= 0 && cluster[0][0] - runs[index][1] <= 60 && runs[index][1] - runs[index][0] < 26; index -= 1) cluster.unshift(runs[index]);
  return cluster.map(([a, b]) => ({ x: Math.round((a + b) / 2), width: b - a + 1 }));
}
function findWord(words, text, { nth = 0, minX = 0, maxX = Infinity, pick = "first", match = null } = {}) {
  const test = match ?? ((word) => norm(word.text) === norm(text));
  const hits = (words ?? []).filter((word) => test(word) && word.left >= minX && word.left <= maxX).sort((a, b) => a.top - b.top || a.left - b.left);
  const word = pick === "rightmost" ? [...hits].sort((a, b) => b.left - a.left)[0] : pick === "leftmost" ? [...hits].sort((a, b) => a.left - b.left)[0] : hits[nth];
  return word ? { x: word.left + Math.round(word.width / 2), y: word.top + Math.round(word.height / 2), right: word.left + word.width, height: word.height, left: word.left, hits: hits.length } : null;
}
function probeShot() {
  ocrIndex += 1;
  const path = join(ocrDir, `probe-${String(ocrIndex).padStart(3, "0")}.png`);
  host.screenshot(path);
  return path;
}
/** Screenshot, OCR, click the label's centre with real OS mouse input. */
async function clickText(text, options = {}) {
  const deadline = Date.now() + (options.timeout ?? 8000);
  do {
    const words = ocr(probeShot());
    if (!words) return { ok: false, stderr: "OCR unavailable" };
    const hit = findWord(words, text, options);
    if (hit) {
      const result = host.click(hit.x + (options.dx ?? 0), hit.y);
      return { ...result, stderr: result.ok ? "" : result.stderr, at: [hit.x + (options.dx ?? 0), hit.y] };
    }
    await sleep(1000);
  } while (Date.now() < deadline);
  return { ok: false, stderr: `text not visible: ${text}` };
}

// ---------------------------------------------------------------- launch
const env = {
  ...process.env,
  FERRYX_DATA_DIR: dirs.data,
  FERRYX_RUNTIME_DIR: dirs.runtime,
  FERRYX_SESSION_DIR: dirs.session,
  RUST_LOG: process.env.RUST_LOG ?? "info",
  FERRYX_BROWSER_ENGINE_MANIFEST_URL: `${fixtureBase}/manifest.json`,
  ...(engineKeyPublic ? { FERRYX_BROWSER_ENGINE_PUBLIC_KEY: engineKeyPublic } : {}),
  ...(dragEngineDir ? { FERRYX_CEF_ENGINE_DIR: dragEngineDir } : {}),
};
if (platform === "linux") Object.assign(env, { GDK_BACKEND: "x11", LIBGL_ALWAYS_SOFTWARE: "1", WEBKIT_DISABLE_DMABUF_RENDERER: "1", WEBKIT_DISABLE_COMPOSITING_MODE: "1" });

// `ferryx browser ...` from this harness carries only the transport token. Settings > Browser >
// Agent access "require pairing" is persisted in <data>/browser/agent-authority.json; turning it
// off for this isolated profile lets `browser list` / `navigate` answer instead of
// BROWSER_CONTROLLER_PAIRING_REQUIRED.
const agentAuthority = join(dirs.data, "browser", "agent-authority.json");
mkdirSync(join(dirs.data, "browser"), { recursive: true });
writeFileSync(agentAuthority, JSON.stringify({ settings: { requirePairing: false } }));
summary.agent_access = { requirePairing: false, file: agentAuthority };

let app = null;
/** Starts the app with the isolated env; its exit is recorded unless the harness restarted it. */
function launchApp(logName) {
  const log = openSync(join(out, logName), "w");
  const child = spawn(binary, [], { env, stdio: ["ignore", log, log], cwd: dirs.tmp, detached: platform !== "win32" });
  child.on("exit", (code, signal) => {
    if (child !== app) return;
    summary.app_exit = { code, signal, at: new Date().toISOString(), during_teardown: Boolean(summary.finished_at) };
    writeSummary();
  });
  mac.pid = win.pid = child.pid;
  return child;
}
if (!existsSync(binary)) {
  summary.launch = { ok: false, error: `binary not found: ${binary}` };
} else {
  app = launchApp("app.log");
  summary.launch = { ok: true, pid: app.pid };
}
writeSummary();

/** `ferryx browser <args>` against the running app (same isolated env), recorded verbatim. */
function cli(...args) {
  const result = run(binary, ["browser", ...args], { env, timeout: 30000 });
  return { ok: result.ok, status: result.status, stdout: result.stdout.slice(0, 4000), stderr: result.stderr };
}
function sessions() {
  const result = cli("list");
  try {
    return { list: JSON.parse(result.stdout), raw: result };
  } catch {
    return { list: [], raw: result };
  }
}

/**
 * `cli` without blocking this process: the fixture server runs on this event loop, so a page
 * the command waits on (wait, snapshot, act postconditions) could not load during spawnSync.
 */
async function cliAsync(...args) {
  const child = Bun.spawn([binary, "browser", ...args], { env, stdout: "pipe", stderr: "pipe" });
  const timer = setTimeout(() => child.kill(), 30000);
  const [stdout, stderr, status] = await Promise.all([new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited]);
  clearTimeout(timer);
  return { ok: status === 0, status, stdout: stdout.trim().slice(0, 4000), stderr: stderr.trim().slice(0, 2000) };
}

/** The error code a failed CLI call printed (first JSON line on stderr). */
function cliCode(result) {
  for (const line of result.stderr.split("\n")) {
    try {
      const parsed = JSON.parse(line);
      if (parsed?.code) return parsed.code;
    } catch {}
  }
  return null;
}

/**
 * Agent automation against /agent.html in tab `browserId`, through the CLI only: a ref from
 * before a reload must fail as stale, then snapshot, fill the Email box, click Save with an
 * --expect-text postcondition and read the page back. Recorded in summary.agent[engine].
 */
async function agentQa(engine, browserId) {
  const record = { browser_id: browserId, checks: {} };
  summary.agent[engine] = record;
  if (!browserId) {
    record.error = `no ${engine} browser tab listed`;
    return { ok: false, stderr: record.error };
  }
  const id = ["--browser-id", browserId];
  const load = async (label) => {
    const waited = await cliAsync("wait", ...id, "--text", "Agent fixture", "--timeout-ms", "20000");
    record[label] = waited.ok ? "ok" : { status: waited.status, code: cliCode(waited), stderr: waited.stderr.slice(0, 500) };
    return waited.ok;
  };
  const snapshot = async (label) => {
    const result = await cliAsync("snapshot", ...id);
    try {
      const parsed = JSON.parse(result.stdout);
      const ref = (predicate) => parsed.elements.find(predicate)?.reference ?? null;
      const taken = {
        generation: parsed.generation,
        isolated: parsed.isolated,
        url: parsed.url,
        title: parsed.title,
        elements: parsed.elements.length,
        seen: parsed.elements.slice(0, 8).map((element) => `${element.tagName}/${element.role}/${element.name}`.slice(0, 80)),
        save: ref((element) => element.tagName === "button" && /save/i.test(element.name)),
        email: ref((element) => element.tagName === "input"),
      };
      record[label] = taken;
      return taken;
    } catch {
      record[label] = { error: `snapshot not JSON (status ${result.status})`, code: cliCode(result), stderr: result.stderr.slice(0, 500) };
      return null;
    }
  };
  const act = async (verb, taken, ...args) => {
    const result = await cliAsync(verb, ...id, "--generation", String(taken.generation), ...args);
    let outcome = null;
    try {
      outcome = JSON.parse(result.stdout);
    } catch {}
    return { ok: result.ok, status: result.status, outcome, code: result.ok ? null : cliCode(result), stderr: result.ok ? undefined : result.stderr.slice(0, 500) };
  };

  // `navigate` returns once the load is requested; a navigation still settling from the UI
  // steps can replace it, so a page that did not arrive is navigated once more.
  const navigate = async (label) => {
    const navigated = await cliAsync("navigate", ...id, "--url", `${fixtureBase}/agent.html`);
    record[label] = navigated.ok ? "ok" : { status: navigated.status, code: cliCode(navigated), stderr: navigated.stderr.slice(0, 500) };
    return navigated.ok;
  };
  const loaded = async (label) => {
    const waited = await cliAsync("wait", ...id, "--url-contains", "/agent.html", "--timeout-ms", "10000");
    record[label] = waited.ok ? "ok" : { status: waited.status, code: cliCode(waited) };
    return waited.ok;
  };
  let arrived = (await navigate("navigate")) && (await loaded("wait_url"));
  if (!arrived) arrived = (await navigate("navigate_retry")) && (await loaded("wait_url_retry"));
  record.checks.fixture_loaded = arrived && (await load("wait_loaded"));
  // What the page script sees when the wait does not match: where it runs and what it reads.
  if (!record.checks.fixture_loaded) {
    const probe = await cliAsync("eval", ...id, "--script", "JSON.stringify({href: location.href, title: document.title, ready: document.readyState, text: (document.body && document.body.innerText || '').slice(0, 120)})");
    record.eval_probe = probe.ok ? probe.stdout.slice(0, 600) : { status: probe.status, code: cliCode(probe), stderr: probe.stderr.slice(0, 400) };
    const listed = sessions().list.find((entry) => entry.browserId === browserId);
    record.listed = listed ? { url: listed.url, title: listed.title, engine: listed.engine ?? null } : null;
  }

  // A ref observed before a reload names a node of the old document.
  const before = await snapshot("snapshot_before_reload");
  record.checks.snapshot = Boolean(before?.save && before?.email);
  record.checks.isolated_world = before?.isolated ?? null;
  if (before?.save) {
    const reloaded = await cliAsync("reload", ...id);
    record.reload = reloaded.ok ? "ok" : { status: reloaded.status, code: cliCode(reloaded) };
    await load("wait_reloaded");
    record.stale_click = await act("click", before, "--ref", before.save);
    record.checks.stale_ref_refused = !record.stale_click.ok && ["BROWSER_STALE_REF", "BROWSER_INVALID_REFERENCE"].includes(record.stale_click.code);
  }

  const current = await snapshot("snapshot");
  if (current?.save && current?.email) {
    record.fill = await act("fill", current, "--ref", current.email, "--value", "agentqa");
    record.checks.fill_dispatched = record.fill.ok && record.fill.outcome?.dispatched === true;
    record.click = await act("click", current, "--ref", current.save, "--expect-text", "Saved agentqa", "--expect-timeout-ms", "8000");
    record.checks.click_applied = record.click.ok && record.click.outcome?.applied === true;
    const read = await cliAsync("eval", ...id, "--script", "document.getElementById('s').textContent");
    record.page_text = read.ok ? read.stdout.slice(0, 200) : { status: read.status, code: cliCode(read) };
    record.checks.page_reflects_act = read.ok && read.stdout.includes("Saved agentqa");
  }
  const required = ["fixture_loaded", "snapshot", "stale_ref_refused", "fill_dispatched", "click_applied", "page_reflects_act"];
  record.ok = required.every((name) => record.checks[name] === true);
  // Observations outside `required` (isolated_world) do not fail the job.
  record.informational = Object.keys(record.checks).filter((name) => !required.includes(name));
  writeSummary();
  return { ok: record.ok, checks: record.checks, stderr: record.ok ? "" : `failed: ${required.filter((name) => record.checks[name] !== true).join(", ")}` };
}

let shotIndex = 0;
function screenshot(name) {
  shotIndex += 1;
  const file = `${String(shotIndex).padStart(2, "0")}-${name}.png`;
  const path = join(shots, file);
  const result = host.screenshot(path);
  const exists = existsSync(path);
  return {
    path: exists ? path : null,
    file: exists ? `screenshots/${file}` : null,
    ok: result.ok && exists,
    sha256: exists ? createHash("sha256").update(readFileSync(path)).digest("hex") : null,
    error: result.ok ? undefined : result.stderr,
  };
}

/**
 * actions: [label, fn] pairs (fn may be async and returns {ok, stderr}) or a number (sleep ms).
 * expect: { any: [words] } visible-text check on the final screenshot, or { none: [words] }.
 */
async function step(name, description, actions, expect = null) {
  const record = { name, description, started_at: new Date().toISOString(), actions: [], outcome: "ok" };
  if (summary.app_exit) {
    record.outcome = "skipped_app_exited";
  } else {
    try {
      for (const entry of actions) {
        if (typeof entry === "number") {
          await sleep(entry);
          continue;
        }
        const [label, action] = entry;
        const result = await action();
        const { ok, status, stderr, ...rest } = result ?? { ok: false, stderr: "no result" };
        const detail = Object.fromEntries(Object.entries(rest).filter(([key]) => !["stdout"].includes(key)));
        record.actions.push({ label, ok, ...(ok ? {} : { status, error: stderr }), ...detail });
        if (!ok && !label.startsWith("optional")) record.outcome = "input_failed";
        await sleep(350);
      }
    } catch (error) {
      record.outcome = "harness_error";
      record.error = String(error?.stack ?? error);
    }
  }
  const shot = screenshot(name);
  record.screenshot = { file: shot.file, ok: shot.ok, sha256: shot.sha256, error: shot.error };
  if (!shot.ok && record.outcome === "ok") record.outcome = "screenshot_failed";
  if (expect) {
    const extra = record.actions.flatMap((action) => action.seen_text ?? []).map((text) => ({ text }));
    const words = shot.path ? [...(ocr(shot.path) ?? []), ...extra] : null;
    if (!words) {
      record.ui_check = { result: "unavailable", expect };
    } else {
      // "text" = exact word, "text*" = word starts with text, "~text" = word contains text
      // (tab titles and caret-adjacent page text glue extra characters onto the word).
      const visible = (text) => (word) => {
        const read = norm(word.text);
        if (text.startsWith("~")) {
          const needle = norm(text.slice(1));
          return needle.length > 0 && read.includes(needle);
        }
        return read === norm(text) || (text.endsWith("*") && read.startsWith(norm(text.slice(0, -1))));
      };
      const seen = (list) => list.filter((text) => words.some(visible(text)));
      const anySeen = expect.any ? seen(expect.any) : [];
      const noneSeen = expect.none ? seen(expect.none) : [];
      const pass = (expect.any ? anySeen.length > 0 : true) && noneSeen.length === 0;
      record.ui_check = { result: pass ? "pass" : "fail", expect, seen: anySeen, unexpected: noneSeen };
    }
  }
  summary.steps.push(record);
  writeSummary();
  console.log(`[ui-audit] ${name}: ${record.outcome}${record.ui_check ? ` (ui_check ${record.ui_check.result})` : ""}`);
  return record;
}

const focus = ["focus window", () => host.focus()];
const key = (label, spec) => [label, () => host.key(spec)];
const repeat = (count, label, spec) => Array.from({ length: count }, (_, index) => key(`${label} ${index + 1}/${count}`, spec));
/** Prefer an OCR-located click; fall back to keyboard actions when the label is not visible. */
const clickOr = (text, fallback, options = {}) => [
  `click "${text}" (keyboard fallback)`,
  async () => {
    const clicked = await clickText(text, options);
    if (clicked.ok) return { ...clicked, via: "ocr-click" };
    for (const [, action] of fallback) {
      const result = await action();
      if (!result.ok) return { ...result, via: "keyboard", ocr_error: clicked.stderr };
      await sleep(300);
    }
    return { ok: true, via: "keyboard", ocr_error: clicked.stderr };
  },
];

// ---------------------------------------------------------------- UI audit steps
/** Waits for the app's main window and its first render; the result goes to summary.window. */
async function waitForWindow() {
  const deadline = Date.now() + 90000;
  let windowId = null;
  while (!windowId && Date.now() < deadline && !summary.app_exit) {
    await sleep(2000);
    windowId = host.findWindow(app.pid);
  }
  linux.windowId = windowId;
  win.hwnd = windowId;
  const found = { found: Boolean(windowId), id: windowId, waited_ms: 90000 - Math.max(0, deadline - Date.now()) };
  await sleep(15000); // first render of the web UI and daemon startup
  return found;
}
if (app) summary.window = await waitForWindow();

/** Click a sidebar label so keyboard focus leaves an embedded webview and app shortcuts reach the UI. */
const chromeFocus = [
  "optional click sidebar 'Worktrees' (focus app chrome)",
  () => clickText("Worktrees", { timeout: 3000, match: (word) => /^worktrees$/i.test(word.text), pick: "leftmost" }),
];
const exactly = (label) => (word) => word.text === label;
/** OCR the browser toolbar zoom label (e.g. "100%"). */
function zoomLabel(words = ocr(probeShot()) ?? []) {
  return words.map((word) => word.text).find((text) => /^\d{2,3}%$/.test(text)) ?? null;
}
/** The address bar's own text: about:blank, a URL, or the fixture host. Leftmost hit wins. */
const ADDRESS_TEXT = /^(about:blank|https?:\/\/\S+|\S*127\.0\.0\.1\S*|\S*example\.com\S*)$/i;
async function clickAddressBar() {
  const deadline = Date.now() + 6000;
  do {
    const words = ocr(probeShot());
    const hit = findWord(words, "", { match: (word) => ADDRESS_TEXT.test(word.text), pick: "leftmost" });
    if (hit) return { ...host.click(hit.x, hit.y), via: "ocr-click", at: [hit.x, hit.y], text_seen: true };
    await sleep(800);
  } while (Date.now() < deadline);
  return { ...host.key({ key: "l", mods: ["mod"] }), via: "keyboard Mod+L" };
}
const typeUrl = (url) => [
  ["click address bar by its own text (Mod+L fallback)", clickAddressBar],
  key("Mod+A", { key: "a", mods: ["mod"] }),
  ["type URL", () => host.type(url)],
  key("Enter", { key: "Enter" }),
  ["optional address bar shows the typed URL (retyped once if not)", async () => {
    const path = new URL(url).pathname.split("/").pop() || new URL(url).host;
    const shows = (words) => (words ?? []).some((word) => word.text.includes(path));
    if (shows(ocr(probeShot()))) return { ok: true, retyped: false };
    await clickAddressBar();
    host.key({ key: "a", mods: ["mod"] });
    host.type(url);
    host.key({ key: "Enter" });
    await sleep(1500);
    const ok = shows(ocr(probeShot()));
    return { ok, retyped: true, stderr: ok ? "" : `address bar does not show ${path}` };
  }],
];
/** Poll OCR until one of `texts` is visible (page painted), bounded; records how long it took. */
const waitForText = (texts, timeout = 15000) => [
  `optional wait for page text (${texts.join("/")})`,
  async () => {
    const started = Date.now();
    do {
      const words = ocr(probeShot()) ?? [];
      const seen = texts.find((text) => words.some((word) => norm(word.text) === norm(text)));
      if (seen) return { ok: true, seen, waited_ms: Date.now() - started };
      await sleep(1000);
    } while (Date.now() - started < timeout);
    return { ok: false, waited_ms: Date.now() - started, stderr: `page text not visible after ${timeout}ms: ${texts.join("/")}` };
  },
];
/**
 * A Settings nav item, located by its own label in the nav column (the column of the leftmost
 * "General"). When OCR misses the label itself, interpolate from the neighbours it did read.
 */
const NAV_ORDER = ["General", "Appearance", "Terminal", "Keyboard", "Agents", "Browser", "Notifications", "Permissions", "Remote"];
function navTarget(words, name) {
  // Column anchor: the leftmost of any nav label (or "Back" of "Back to app"); "General" alone is
  // not enough because the content header also reads "General" and OCR may miss the nav one.
  const anchors = (words ?? []).filter((word) => [...NAV_ORDER, "Back"].includes(word.text));
  if (!anchors.length) return null;
  const columnLeft = Math.min(...anchors.map((word) => word.left));
  const inColumn = (word) => Math.abs(word.left - columnLeft) <= 14;
  const general = findWord(words, "", { match: (word) => word.text === "General" && inColumn(word) })
    ?? findWord(words, "", { match: (word) => NAV_ORDER.includes(word.text) && inColumn(word) });
  if (!general) return null;
  const direct = findWord(words, "", { match: (word) => word.text === name && inColumn(word) });
  if (direct) return { ...direct, how: "label" };
  const seen = NAV_ORDER.map((label, index) => {
    const hit = findWord(words, "", { match: (word) => word.text === label && inColumn(word) });
    return hit ? { index, y: hit.y } : null;
  }).filter(Boolean);
  if (seen.length < 2) return null;
  const pitch = (seen.at(-1).y - seen[0].y) / (seen.at(-1).index - seen[0].index);
  return { x: columnLeft + 24, y: Math.round(seen[0].y + (NAV_ORDER.indexOf(name) - seen[0].index) * pitch), how: `interpolated, pitch ${pitch.toFixed(1)}` };
}
async function clickNav(name) {
  const deadline = Date.now() + 6000;
  do {
    const target = navTarget(ocr(probeShot()), name);
    if (target) return { ...host.click(target.x, target.y), via: "ocr-click", at: [target.x, target.y], how: target.how };
    await sleep(800);
  } while (Date.now() < deadline);
  return { ok: false, stderr: `Settings nav not readable (looking for ${name})` };
}
/** The switch on the row whose label starts with `label`, found from the row's own pixels. */
async function clickRowSwitch(label) {
  const deadline = Date.now() + 6000;
  do {
    const path = probeShot();
    const row = findWord(ocr(path), "", { match: exactly(label), pick: "leftmost" });
    if (row) {
      const knob = findRowSwitch(path, row);
      if (knob) return { ...host.click(knob.x, knob.y), via: "pixel-switch", at: [knob.x, knob.y], label_at: [row.x, row.y] };
      return { ok: false, stderr: `no switch-sized control right of "${label}"`, label_at: [row.x, row.y] };
    }
    await sleep(800);
  } while (Date.now() < deadline);
  return { ok: false, stderr: `row label not visible: ${label}` };
}
/** Left edge of the Settings nav column (leftmost nav label or "Back"), or null. */
function navColumnLeft(words) {
  const anchors = (words ?? []).filter((word) => [...NAV_ORDER, "Back"].includes(word.text));
  return anchors.length ? Math.min(...anchors.map((word) => word.left)) : null;
}
/**
 * A multi-word phrase read left to right on one OCR line: each next word starts within 40px of
 * the previous word's end and within 8px vertically. Also reports the word that directly follows
 * the phrase on that line, so a group heading ("Import") is not confused with a button label that
 * merely starts with it ("Import Cookies", "Import from Chrome").
 */
function findPhrase(words, phrase, { minX = 0 } = {}) {
  const parts = phrase.split(/\s+/);
  const after = (prev, test) =>
    (words ?? []).find((word) => test(word) && Math.abs(word.top - prev.top) <= 8 && word.left > prev.left && word.left - (prev.left + prev.width) < 40);
  const hits = [];
  for (const first of (words ?? []).filter((word) => word.text === parts[0] && word.left >= minX)) {
    let prev = first;
    for (const part of parts.slice(1)) {
      prev = after(prev, (word) => word.text === part);
      if (!prev) break;
    }
    if (!prev) continue;
    const right = prev.left + prev.width;
    hits.push({
      x: Math.round((first.left + right) / 2),
      y: first.top + Math.round(first.height / 2),
      left: first.left,
      top: first.top,
      right,
      height: first.height,
      followedBy: after(prev, (word) => word !== prev && !parts.includes(word.text))?.text ?? null,
    });
  }
  // The multi-pass OCR reports the same word once per variant; keep one hit per position.
  const unique = new Map(hits.map((hit) => [`${Math.round(hit.left / 8)}:${Math.round(hit.top / 8)}`, hit]));
  return [...unique.values()].sort((a, b) => a.top - b.top);
}
/**
 * A Settings group heading: the exact title phrase with nothing glued after it on the same line,
 * starting at the content column's left edge (the most common left edge of words right of the
 * nav: headings and row labels share it; buttons and later words of descriptions do not).
 */
function findSettingsHeading(words, title) {
  const nav = navColumnLeft(words);
  const minX = nav === null ? 0 : nav + 150;
  const lefts = new Map();
  for (const word of words ?? []) {
    if (word.left < minX) continue;
    const bucket = Math.round(word.left / 6);
    lefts.set(bucket, (lefts.get(bucket) ?? 0) + 1);
  }
  const column = [...lefts.entries()].sort((a, b) => b[1] - a[1])[0];
  const contentLeft = column ? column[0] * 6 : null;
  const candidates = findPhrase(words, title, { minX });
  const heading = candidates.find((hit) => !hit.followedBy && (contentLeft === null || Math.abs(hit.left - contentLeft) <= 12)) ?? null;
  return { heading, candidates, contentLeft, nav };
}
/** Content-pane text with positions, to tell "the wheel moved the pane" from "end of pane". */
const paneFingerprint = (words, minX) =>
  [...new Set((words ?? []).filter((word) => word.left >= minX).map((word) => `${word.text}@${Math.round(word.top / 12)}`))].sort().join("|");
/**
 * Scroll the Settings content pane with the mouse wheel until a group heading is visible.
 * Starts downwards; when two consecutive probes read the same pane text that end is reached, so
 * it turns around. Fails after both ends were seen (the group is absent, not off-screen) or after
 * maxSteps wheel steps. A heading in the lower half is nudged up so its body is in the shot.
 */
async function scrollSettingsTo(title, { notches = 4, maxSteps = 30, direction = 1 } = {}) {
  if (!host.wheel) return { ok: false, stderr: "no wheel input on this platform" };
  let dir = direction;
  let previous = null;
  const ends = [];
  const trail = [];
  for (let count = 0; count <= maxSteps; count += 1) {
    const path = probeShot();
    const words = ocr(path);
    if (!words) return { ok: false, stderr: "OCR unavailable" };
    const found = findSettingsHeading(words, title);
    const general = findWord(words, "", { match: exactly("General"), pick: "leftmost" });
    // Wheel over the content pane: well right of the nav column, mid-height.
    const wheelAt = [found.nav !== null ? found.nav + 640 : 900, general ? general.y + 300 : 500];
    if (found.heading) {
      let hit = found.heading;
      const nudged = hit.y > wheelAt[1] + 60;
      if (nudged) {
        host.wheel(...wheelAt, 2);
        await sleep(700);
        const again = findSettingsHeading(ocr(probeShot()), title).heading;
        if (again) hit = again;
        else {
          host.wheel(...wheelAt, -2);
          await sleep(700);
        }
      }
      return { ok: true, at: [hit.x, hit.y], wheel_steps: count, nudged, ends_seen: ends, probe: basename(path) };
    }
    const fingerprint = paneFingerprint(words, found.nav === null ? 0 : found.nav + 150);
    if (previous !== null && fingerprint === previous) {
      ends.push(dir > 0 ? "bottom" : "top");
      if (ends.length >= 2) break;
      dir = -dir;
    }
    previous = fingerprint;
    trail.push({ probe: basename(path), dir, near_misses: found.candidates.map((hit) => `${hit.followedBy ? `${title} ${hit.followedBy}…` : title}@${hit.left},${hit.top}`) });
    host.wheel(...wheelAt, dir * notches);
    await sleep(700);
  }
  return {
    ok: false,
    stderr: ends.length >= 2 ? `group '${title}' absent: scrolled Settings > Browser to both ends` : `group heading not visible after ${maxSteps} wheel steps: ${title}`,
    ends_seen: ends,
    trail: trail.slice(-8),
  };
}
/** Open Settings: Mod+, then, when the nav is not visible, click the app sidebar and retry once. */
async function openSettings() {
  const settingsVisible = (words) => Boolean(findWord(words, "", { match: (word) => /^(Appearance|Keyboard)$/.test(word.text) }));
  host.key({ key: ",", mods: ["mod"] });
  await sleep(1800);
  if (settingsVisible(ocr(probeShot()))) return { ok: true, attempts: 1 };
  const focused = await clickText("Worktrees", { timeout: 3000, match: (word) => /^worktrees$/i.test(word.text), pick: "leftmost" });
  await sleep(400);
  host.key({ key: ",", mods: ["mod"] });
  await sleep(1800);
  const visible = settingsVisible(ocr(probeShot()));
  return { ok: visible, attempts: 2, refocus_click: focused.ok, stderr: visible ? "" : "Settings did not open after Mod+, twice (second after clicking the sidebar)" };
}
/** Optional: dismiss the Windows "moving to the Microsoft Store" toast so it does not cover rows. */
/**
 * OCR never reads the toast's "Got it" button, but reads the toast body reliably. Measured on
 * runs 41322108840/41331914682 at two window sizes: the button centre is at
 * (moving.left + 200, moving.top + 41), where "moving" is from "is moving to the Microsoft Store".
 */
async function dismissToastNow() {
  if (platform !== "win32") return { ok: true, skipped: "not Windows" };
  const words = ocr(probeShot());
  const direct = findWord(words, "", { match: exactly("Got") });
  if (direct) return { ...host.click(direct.x + 8, direct.y), via: "Got label", at: [direct.x + 8, direct.y] };
  const anchor = (words ?? []).find((word) => word.text === "moving");
  if (!anchor) return { ok: true, skipped: "no toast visible" };
  const at = [anchor.left + 200, anchor.top + 41];
  const clicked = host.click(...at);
  await sleep(800);
  const still = (ocr(probeShot()) ?? []).some((word) => word.text === "moving");
  return { ...clicked, ok: clicked.ok && !still, via: "offset from 'moving'", at, stderr: still ? "toast still visible after click" : "" };
}
const dismissStoreToast = ["optional dismiss Windows store toast", dismissToastNow];
/** A stray native dialog (file picker) owned by the app swallows all later input; close it. */
const closeStrayDialogs = [
  "optional close stray native dialogs",
  () => (platform === "win32" ? win.closeDialogs() : { ok: true, skipped: "not Windows" }),
];

/** Every ferryx_engine process (the engine and its helpers): pgrep on Linux and macOS, tasklist on Windows. */
const enginePids = () => {
  if (platform === "win32") {
    return run("tasklist", ["/FI", "IMAGENAME eq ferryx_engine.exe", "/FO", "CSV", "/NH"]).stdout
      .split(/\r?\n/)
      .map((line) => /^"ferryx_engine\.exe","(\d+)"/i.exec(line.trim())?.[1])
      .filter(Boolean)
      .map(Number);
  }
  return run("pgrep", ["-f", "ferryx_engine"]).stdout.split(/\s+/).filter(Boolean).map(Number);
};
/**
 * Which process holds keyboard focus, as { engine, detail }. Linux: the raw X focus window is a
 * window other than the Ferryx toplevel (the engine pane is a child X window). macOS: the
 * frontmost app is the engine (its windows belong to a separate app ordered above Ferryx).
 * Windows: the foreground thread's focus HWND belongs to an engine process (a child HWND of the
 * Ferryx window owned by the engine).
 */
function engineKeyboardFocus() {
  if (platform === "linux") {
    const focused = run("xdotool", ["getwindowfocus", "-f"]).stdout;
    return { engine: /^\d+$/.test(focused) && Number(focused) > 1 && focused !== String(linux.windowId), detail: { focused, ferryx: linux.windowId } };
  }
  if (platform === "darwin") {
    const front = Number(mac.osa('tell application "System Events" to get unix id of first process whose frontmost is true').stdout);
    return { engine: enginePids().includes(front), detail: { frontmost_pid: front, ferryx_pid: mac.pid } };
  }
  const [hwnd, pid] = win.ps("focusowner").stdout.split(/\s+/).filter(Boolean).slice(-2).map(Number);
  return { engine: Boolean(pid) && enginePids().includes(pid), detail: { focus_hwnd: hwnd ?? null, focus_pid: pid ?? null, ferryx_pid: win.pid } };
}

// ---------------------------------------------------------------- drag-mask scenario
// While a tab is dragged, TerminalSplitView renders browser panes hidden (browserPanesVisible is
// activeDrag === null), which hides the Chromium engine's native window. That window sits above
// the web UI, so fixture green in a mid-drag screenshot means the mask failed.
const FIXTURE_GREEN = [0, 168, 107];
const FIXTURE_YELLOW = [255, 212, 0];
const FIXTURE_TOLERANCE = 30;
const FIXTURE_SAMPLE_STEP = 4;
// Counts are of every 4th pixel in both directions. The fixture's 40px green frame around a
// ~1000x700 pane gives several thousand samples; a hidden pane gives (near) none.
const FIXTURE_VISIBLE_MIN = 800;
const FIXTURE_MASKED_MAX = 40;
function fixturePixels(path) {
  if (!path || !existsSync(path)) return { error: "no screenshot" };
  try {
    const image = decodePng(path);
    const near = (pixel, expected) => pixel.every((value, index) => Math.abs(value - expected[index]) <= FIXTURE_TOLERANCE);
    let green = 0, yellow = 0;
    for (let y = 0; y < image.height; y += FIXTURE_SAMPLE_STEP) {
      for (let x = 0; x < image.width; x += FIXTURE_SAMPLE_STEP) {
        const pixel = image.rgb(x, y);
        if (near(pixel, FIXTURE_GREEN)) green += 1;
        else if (near(pixel, FIXTURE_YELLOW)) yellow += 1;
      }
    }
    return { green, yellow, width: image.width, height: image.height };
  } catch (error) {
    return { error: String(error?.message ?? error) };
  }
}
/** Any ferryx_engine process (pgrep on mac/linux, tasklist on Windows). */
const engineProcessRunning = () =>
  platform === "win32"
    ? run("tasklist", ["/FI", "IMAGENAME eq ferryx_engine.exe", "/NH"]).stdout.toLowerCase().includes("ferryx_engine")
    : run("pgrep", ["-f", "ferryx_engine"]).ok;

async function dragMaskFlow() {
  const record = summary.drag_mask;
  record.thresholds = { visible_min: FIXTURE_VISIBLE_MIN, masked_max: FIXTURE_MASKED_MAX, tolerance: FIXTURE_TOLERANCE, sample_step: FIXTURE_SAMPLE_STEP };
  const shotPath = (entry) => (entry.screenshot?.file ? join(out, entry.screenshot.file) : null);

  await step(
    "drag-mask-open-browser",
    "Mod+Shift+B opens a browser tab (Chromium via FERRYX_CEF_ENGINE_DIR); the CLI navigates it to the fixture page; wait until its pixels are on screen",
    [
      focus,
      chromeFocus,
      key("Mod+Shift+B", { key: "b", mods: ["mod", "shift"] }),
      ["optional wait for the engine process", async () => {
        const deadline = Date.now() + 30000;
        while (Date.now() < deadline) {
          if (engineProcessRunning()) return { ok: true };
          await sleep(1000);
        }
        return { ok: false, stderr: "no ferryx_engine process after 30s" };
      }],
      ["navigate the tab to the fixture (CLI)", async () => {
        const deadline = Date.now() + 30000;
        let listed = sessions();
        while (!listed.list.length && Date.now() < deadline) {
          await sleep(1000);
          listed = sessions();
        }
        record.browser_list_after_open = listed.raw;
        const tab = listed.list.find((entry) => entry.engine === "chromium") ?? listed.list.at(-1);
        record.browser_id = tab?.browserId ?? null;
        record.engine = tab?.engine ?? null;
        record.chromium = tab?.engine === "chromium";
        if (!tab) return { ok: false, stderr: "browser list reported no tab" };
        const navigated = await cliAsync("navigate", "--browser-id", tab.browserId, "--url", `${fixtureBase}/fixture.html`);
        return { ok: navigated.ok, status: navigated.status, engine: record.engine, stderr: navigated.stderr };
      }],
      ["wait for fixture pixels", async () => {
        const started = Date.now();
        let pixels = null;
        do {
          pixels = fixturePixels(probeShot());
          if ((pixels.green ?? 0) >= FIXTURE_VISIBLE_MIN) return { ok: true, waited_ms: Date.now() - started, pixels };
          await sleep(1000);
        } while (Date.now() - started < 60000);
        return { ok: false, waited_ms: Date.now() - started, pixels, stderr: "fixture green not on screen after 60s" };
      }],
    ],
  );
  const listedAfter = sessions();
  record.browser_list_after_navigate = listedAfter.raw;
  record.title = listedAfter.list.find((entry) => entry.browserId === record.browser_id)?.title ?? null;

  const before = await step("drag-mask-before", "Fixture page in the browser tab before any drag: fixture green on screen", [focus, 1000]);
  record.before = fixturePixels(shotPath(before));
  record.visible_before = (record.before.green ?? 0) >= FIXTURE_VISIBLE_MIN;

  // dnd-kit's PointerSensor activates after 12px of movement; 8 moves of 10px with ~100ms
  // between them start the drag well before the hold, and the button stays down for the shot.
  const drag = { start: null, at: null, pressed: false };
  const mid = await step(
    "drag-mask-mid-drag",
    "Press the left button on the browser tab (found by its 'fixture' title), drag 80px right in 8 steps, hold for 1s; the engine window must be hidden",
    [
      ["locate the browser tab by its title (OCR, topmost hit)", async () => {
        const deadline = Date.now() + 10000;
        do {
          const words = ocr(probeShot());
          if (!words) return { ok: false, stderr: "OCR unavailable" };
          // Topmost: the tab bar is above the address bar, which also contains "fixture".
          const hit = findWord(words, "", { match: (word) => norm(word.text).includes("fixture") });
          if (hit) {
            drag.start = [hit.x, hit.y];
            return { ok: true, at: drag.start, hits: hit.hits };
          }
          await sleep(1000);
        } while (Date.now() < deadline);
        return { ok: false, stderr: "no OCR word containing 'fixture' (the browser tab title) on screen" };
      }],
      ["press, drag right 80px in 8 steps, keep holding", async () => {
        if (!drag.start) return { ok: false, stderr: "browser tab not located" };
        const [x, y] = drag.start;
        const down = host.mouseDown(x, y);
        if (!down.ok) return { ...down, stage: "down" };
        drag.pressed = true;
        await sleep(150);
        for (let index = 1; index <= 8; index += 1) {
          const moved = host.mouseMove(x + index * 10, y);
          if (!moved.ok) return { ...moved, stage: `move ${index}` };
          drag.at = [x + index * 10, y];
          await sleep(100);
        }
        return { ok: true, from: drag.start, to: drag.at };
      }],
      1000,
    ],
  );
  record.tab_found = Boolean(drag.start);
  record.tab_at = drag.start;
  record.mid_drag = fixturePixels(shotPath(mid));
  record.masked_mid_drag = drag.pressed && Boolean(drag.at) && (record.mid_drag.green ?? Infinity) <= FIXTURE_MASKED_MAX;

  const after = await step(
    "drag-mask-after",
    "Move back to the start and release (no reorder); 2s later the fixture is visible again",
    [
      ["move back and release", async () => {
        if (!drag.pressed) return { ok: false, stderr: "the button was never pressed" };
        const [x, y] = drag.start;
        for (let px = (drag.at ?? drag.start)[0] - 10; px > x; px -= 10) {
          host.mouseMove(px, y);
          await sleep(100);
        }
        const up = host.mouseUp(x, y);
        if (up.ok) drag.pressed = false;
        return { ...up, stage: "up" };
      }],
      2000,
    ],
  );
  record.after = fixturePixels(shotPath(after));
  record.restored_after_drop = (record.after.green ?? 0) >= FIXTURE_VISIBLE_MIN;
  record.pass = record.visible_before && record.masked_mid_drag && record.restored_after_drop;
  record.checks = {
    chromium: record.chromium === true,
    visible_before: record.visible_before === true,
    masked_mid_drag: record.masked_mid_drag === true,
    restored_after_drop: record.restored_after_drop === true,
  };
  writeSummary();
}

// ---------------------------------------------------------------- base flow
await step("launch", "App window after startup", [focus, dismissStoreToast]);
await step(
  "dismiss-onboarding",
  "Click 'Skip setup' in the first-run wizard (fallback: Tab into the wizard, Escape)",
  [focus, clickOr("Skip", [key("Tab", { key: "Tab" }), key("Escape", { key: "Escape" })], { timeout: 5000 }), 2000],
  { none: ["Skip"] },
);
if (scenario === "drag-mask") {
  await dragMaskFlow();
  await teardown();
}
await step("new-browser-tab", "Mod+Shift+B (tab.newBrowser) opens a browser tab", [focus, chromeFocus, key("Mod+Shift+B", { key: "b", mods: ["mod", "shift"] }), 5000], {
  any: ["about:blank", "Browser"],
});
summary.cli_after_new_tab = sessions().raw;
await step(
  "address-bar-example",
  "Click the address bar (located by its own text), select all, type https://example.com, Enter",
  [focus, ...typeUrl("https://example.com"), 8000],
  { any: ["Domain", "documentation", "illustrative"] },
);
await step(
  "page-load-fixture",
  "Address bar: local example page (fixture server), Enter; the page renders its heading",
  [focus, ...typeUrl(`${fixtureBase}/example.html`), waitForText(["Domain", "illustrative"])],
  { any: ["Domain", "illustrative"] },
);
await step(
  "agent-webview",
  "Agent automation in the system webview tab (CLI): stale ref after reload refused, snapshot, fill Email, click Save with --expect-text, page shows 'Saved agentqa'; then back to the example page",
  [
    focus,
    ["agent snapshot/act", () => {
      const list = sessions().list;
      const tab = list.find((entry) => entry.engine === "system") ?? list.at(-1);
      return agentQa("webview", tab?.browserId ?? null);
    }],
    1500,
    ["screenshot agent page", () => ({ ok: screenshot("agent-webview-page").ok })],
    ["optional back to the example page", () => {
      const browserId = summary.agent.webview?.browser_id;
      if (!browserId) return { ok: false, stderr: "no tab" };
      const result = cli("navigate", "--browser-id", browserId, "--url", `${fixtureBase}/example.html`);
      return { ok: result.ok, stderr: result.stderr };
    }],
    2000,
  ],
);
await step("settings-open", "Click the sidebar, Mod+, opens Settings (retried once after re-focusing the app)", [focus, chromeFocus, ["open Settings", openSettings], dismissStoreToast], {
  any: ["General", "Appearance"],
});
await step(
  "settings-enable-advanced",
  "Click the switch on General > 'Advanced features' (row found by label, switch by its pixels); Browser/Notifications appear in the nav",
  [focus, dismissStoreToast, ["click Advanced features switch", () => clickRowSwitch("Advanced")], 1500],
  { any: ["Notifications"] },
);
await step(
  "settings-browser",
  "Click 'Browser' in the Settings nav (nav column found from 'General')",
  [focus, dismissStoreToast, ["click nav Browser", () => clickNav("Browser")], 2500],
  { any: ["Web", "Navigation", "Chromium"] },
);
// Settings > Browser group order (integration): Web & Navigation, Default Browser, Chromium Engine,
// Extensions, Site settings, Agent access, Passwords, Link Routing, File Links, Session & Cookies,
// Active Browser Tabs, Import. scrollSettingsTo scans both directions, so order only affects time.
for (const [name, title] of [
  ["default-browser", "Default Browser"],
  ["extensions", "Extensions"],
  ["site-settings", "Site settings"],
  ["passwords", "Passwords"],
  ["import", "Import"],
]) {
  await step(
    `settings-browser-${name}`,
    `Settings > Browser: scroll the content pane with the mouse wheel to the '${title}' group`,
    [focus, dismissStoreToast, [`scroll to ${title}`, () => scrollSettingsTo(title)], 800],
    { any: [title.split(" ")[0]] },
  );
}
await step(
  "settings-browser-import-dialog",
  "Click 'Import browser data…' in the Import group; the import dialog opens",
  [
    focus,
    dismissStoreToast,
    ["click Import browser data", async () => {
      // Target = the phrase "Import browser" (never "Import Cookies", which opens a native file
      // dialog), on the Import group's heading line (the button is the group's header action).
      const locate = (words) => {
        const group = findSettingsHeading(words, "Import").heading;
        const button = findPhrase(words, "Import browser").find((hit) => !group || Math.abs(hit.top - group.top) <= 40);
        return { group, button };
      };
      let words = ocr(probeShot()) ?? [];
      let { group, button } = locate(words);
      if (!button) {
        const scrolled = await scrollSettingsTo("Import");
        if (!scrolled.ok) return { ...scrolled, stderr: `Import group not reachable: ${scrolled.stderr}` };
        words = ocr(probeShot()) ?? [];
        ({ group, button } = locate(words));
      }
      if (!button) return { ok: false, stderr: "'Import browser data…' button not visible next to the Import heading", group_at: group ? [group.x, group.y] : null };
      const at = [button.x, button.y];
      const clicked = host.click(...at);
      await sleep(1500);
      // Verify: the dialog title "Import browser data" is on screen and the button row is not the
      // only match (dialog overlays the page, so the title is a second, separate hit).
      const after = ocr(probeShot()) ?? [];
      const dialogOpen = findPhrase(after, "Import browser data").some((hit) => Math.abs(hit.y - button.y) > 20) || findPhrase(after, "browser data").length > 1;
      return { ...clicked, ok: clicked.ok && dialogOpen, at, group_at: group ? [group.x, group.y] : null, dialog_open: dialogOpen, stderr: dialogOpen ? "" : "clicked 'Import browser data…' but no import dialog title appeared" };
    }],
    1000,
  ],
  { any: ["Bookmarks", "History", "Cookies", "Passwords", "Chrome", "Firefox", "Safari", "Edge", "Choose", "Cancel"] },
);
await step("settings-import-dialog-closed", "Escape closes the import dialog", [focus, key("Escape", { key: "Escape" }), 1000, closeStrayDialogs]);
if (engineZip) {
  await step(
    "engine-install",
    "Settings > Browser > Chromium Engine: click 'Download', wait for the install",
    [
      focus,
      // The Import steps can leave Settings closed (on Windows run 41635903718 the wheel did not
      // scroll, the import dialog never opened and its Escape closed Settings): reopen it here.
      ["Settings > Browser on screen", async () => {
        const words = ocr(probeShot()) ?? [];
        if (findWord(words, "", { match: (word) => /^(Appearance|Keyboard)$/.test(word.text) })) return { ok: true, already_open: true };
        const opened = await openSettings();
        if (!opened.ok) return opened;
        await sleep(500);
        return { ...(await clickNav("Browser")), reopened: true };
      }],
      1500,
      // The Import step left the pane scrolled to the bottom; Chromium Engine sits above Extensions.
      ["scroll up to the Chromium Engine group", () => scrollSettingsTo("Chromium Engine", { direction: -1 })],
      // Rightmost: the group also has a "Download size" row label on the left.
      ["click Download", () => clickText("Download", { timeout: 8000, match: exactly("Download"), pick: "rightmost" })],
      3000,
      ["wait for ferryx_engine on disk", async () => {
        // The install unpacks into <root>/.staging-<uuid>/ (the executable is there before the
        // unpack ends), then renames it and writes the "active" pointer last. Only the directory
        // the pointer names counts as installed.
        const root = join(dirs.data, "browser-engine");
        const pointer = join(root, "active");
        const deadline = Date.now() + 240000;
        while (Date.now() < deadline) {
          if (existsSync(pointer)) {
            const name = readFileSync(pointer, "utf8").trim();
            if (name && !name.startsWith(".") && !/[\\/]/.test(name) && existsSync(join(root, name, ENGINE_EXE))) {
              return { ok: true, dir: join(root, name), active: name };
            }
          }
          await sleep(2000);
        }
        return { ok: false, stderr: `no "active" pointer naming a directory with ${ENGINE_EXE} under ${root}` };
      }],
      // The UI resolves the install after the pointer is written; wait for its result, bounded.
      waitForText(["Remove", "Installed"], 90000),
    ],
    { any: ["Remove", "Installed"] },
  );
  summary.engine.checks.installed_from_settings = summary.steps.at(-1).actions.find((action) => action.label === "wait for ferryx_engine on disk")?.ok ?? false;
}

await step("settings-closed", "Escape closes Settings", [focus, key("Escape", { key: "Escape" }), 1500], { none: ["Appearance"] });

// ---------------------------------------------------------------- page interactions (every OS)
await step(
  "download-trigger",
  "Load the local download page and click its download link; the downloads surface (prompt or shelf) shows ferryx-qa.txt, Save or Download",
  [closeStrayDialogs, focus, dismissStoreToast, ...typeUrl(`${fixtureBase}/download.html`), waitForText(["FERRYXDOWNLOAD"]), ["click download link", () => clickText("FERRYXDOWNLOAD", { timeout: 15000 })], 4000],
  { any: ["ferryx-qa.txt", "Save", "Download"] },
);
if (platform === "darwin") {
  // WKWebView routes camera and microphone requests through Ferryx's own permission prompt.
  await step(
    "media-prompt",
    "Load the local media page and click its button (getUserMedia camera+microphone); Ferryx's prompt shows '<origin> wants to use camera and microphone' with Block and Allow",
    [focus, ...typeUrl(`${fixtureBase}/media.html`), waitForText(["MEDIAREQUEST"]), ["click media request button", () => clickText("MEDIAREQUEST", { timeout: 15000 })], waitForText(["wants", "Allow", "Block"], 15000)],
    { any: ["wants", "Allow", "Block"] },
  );
  summary.media_prompt = summary.steps.at(-1).ui_check?.result ?? null;
  await step("media-prompt-dismissed", "Block dismisses the media prompt so later steps are not covered", [
    ["optional click Block", () => clickText("Block")],
  ]);
}
await step(
  "page-for-find",
  "Load the local example page again (find and zoom target)",
  [focus, ...typeUrl(`${fixtureBase}/example.html`), waitForText(["Domain", "illustrative"])],
  { any: ["Domain", "illustrative"] },
);
await step("find-open", "Mod+F opens the find bar; type 'Example' (page has 6 case-insensitive matches)", [
  focus,
  chromeFocus,
  key("Mod+F", { key: "f", mods: ["mod"] }),
  1000,
  ["type query", () => host.type("Example")],
  1500,
  ["optional read find-bar match count", () => {
    // The "N matches" label is small grey-on-dark text that full-frame OCR misses on every OS.
    // Re-OCR a strip right of the find input (the input holds the query "Example") at 6x, normalized.
    const path = probeShot();
    const words = ocr(path) ?? [];
    // The find input: an "Example" with nothing after it on its line (tab title and heading read "Example Domain").
    const query = findPhrase(words, "Example").find((hit) => !hit.followedBy);
    if (!query || !imageMagick || !tesseract) return { ok: false, stderr: "find input not located" };
    const top = Math.max(0, query.y - 14);
    const readStrip = (strip, box) => {
      const file = path.replace(/\.png$/, `.${strip}.png`);
      run(imageMagick, [path, "-crop", box, "+repage", "-colorspace", "gray", "-normalize", "-resize", "600%", file]);
      const text = run(tesseract, [file, "-", "--psm", "7"]).stdout.replace(/\s+/g, " ").trim();
      return { strip, box, text, count: /(\d+|[GbS])?\s*mat[ce][hn]es/i.test(text) };
    };
    const tries = [readStrip("findcount", `120x28+${query.left + 600}+${top}`)];
    if (!tries[0].count) {
      // Linux: the 1280-wide window puts the count ~870px right of the query, outside the box
      // above; read a strip from just right of the query to the window (or screen) right edge.
      const x0 = query.left + 300;
      const frame = platform === "linux" ? linux.geometry() : null;
      const right = frame ? frame.x + frame.width : decodePng(path).width;
      if (right > x0) tries.push(readStrip("findcount-right", `${right - x0}x28+${x0}+${top}`));
    }
    const hit = tries.find((attempt) => attempt.count) ?? null;
    const last = hit ?? tries.at(-1);
    return {
      ok: Boolean(hit),
      text: last.text,
      box: last.box,
      matched_strip: hit?.strip ?? null,
      tries: tries.map(({ strip, box, text }) => ({ strip, box, text })),
      seen_text: hit ? ["matches"] : [],
      stderr: hit ? "" : "match count not read",
    };
  }],
], {
  any: ["matches", "match", "6matches", "of"],
});
await step("find-close", "Escape closes the find bar (no alert expected)", [focus, key("Escape", { key: "Escape" }), 1500], {
  none: ["matches", "failed:", "BROWSER_FIND_FAILED"],
});
let zoomBefore = null;
await step(
  "zoom-in",
  "Mod+= twice; the toolbar zoom label must change from 100%",
  [
    focus,
    chromeFocus,
    ["read zoom label", () => ((zoomBefore = zoomLabel()), { ok: true, zoom_before: zoomBefore })],
    key("Mod+= 1", { key: "=", mods: ["mod"] }),
    key("Mod+= 2", { key: "=", mods: ["mod"] }),
    2000,
    ["optional zoom label changed", () => {
      const after = zoomLabel();
      const changed = Boolean(after) && after !== zoomBefore;
      return { ok: changed, zoom_before: zoomBefore, zoom_after: after, stderr: changed ? "" : "zoom label unchanged or unreadable" };
    }],
  ],
);
await step("zoom-reset", "Mod+0 resets zoom", [focus, key("Mod+0", { key: "0", mods: ["mod"] }), 1500]);
await step("narrow-900", "Resize the window to 900x800 (toolbar must stay legible)", [focus, ["resize 900x800", () => host.resize(900, 800)], 2500], {
  any: ["127.0.0.1", "example.html", "Domain"],
});
await step("narrow-900-settings", "At 900px, open Settings > Browser", [closeStrayDialogs, focus, chromeFocus, ["open Settings", openSettings], dismissStoreToast, ["click nav Browser", () => clickNav("Browser")], 2000], {
  any: ["Web", "Navigation"],
});
await step("narrow-900-settings-closed", "Escape closes Settings at 900px", [focus, key("Escape", { key: "Escape" }), 1500]);

// ---------------------------------------------------------------- Chromium engine checks
let xStateIndex = 0;
/** Writes the X window tree, the raw focus window and its properties to x-state/<n>-<name>.txt. */
const captureXState = (name) => {
  if (platform !== "linux") return { file: null };
  xStateIndex += 1;
  const dir = join(out, "x-state");
  mkdirSync(dir, { recursive: true });
  const rawFocus = run("xdotool", ["getwindowfocus", "-f"]).stdout;
  const file = `x-state/${String(xStateIndex).padStart(2, "0")}-${name}.txt`;
  writeFileSync(join(out, file), [
    `ferryx toplevel: ${linux.windowId}`,
    `raw focus (xdotool getwindowfocus -f): ${rawFocus}`,
    `client focus (xdotool getwindowfocus): ${run("xdotool", ["getwindowfocus"]).stdout}`,
    "--- xprop -id <raw focus>",
    /^\d+$/.test(rawFocus) ? run("xprop", ["-id", rawFocus]).stdout : "(none)",
    "--- xwininfo -root -tree",
    run("xwininfo", ["-root", "-tree"]).stdout,
  ].join("\n"));
  return { file, raw_focus: rawFocus };
};
/** Waits until keyboard focus moves to the engine (it does so asynchronously after Mod+L or a
 * click), bounded by `timeout`; see engineKeyboardFocus for what that means on each platform.
 * Linux reads the raw focus window (`-f`): the engine window is a child of the Ferryx window
 * without WM_STATE, and plain `getwindowfocus` walks up from it to the Ferryx client window. */
const waitForEngineFocus = (label, timeout = 8000) => [label, async () => {
  const deadline = Date.now() + timeout;
  let focus = null;
  while (Date.now() < deadline) {
    focus = engineKeyboardFocus();
    if (focus.engine) {
      return { ok: true, ...focus.detail, x_state: captureXState(label.replace(/\W+/g, "-")).file };
    }
    await sleep(200);
  }
  return { ok: false, ...(focus?.detail ?? {}), x_state: captureXState(label.replace(/\W+/g, "-")).file, stderr: "keyboard focus stayed off the engine" };
}];
if (engineZip && summary.engine.checks.installed_from_settings) {
  await step("engine-restore-size", "Resize back to 1280x850 before the Chromium tab", [focus, ["resize", () => host.resize(1280, 850)], 1500]);
  let browserId = null;
  await step(
    "chromium-tab",
    "Mod+Shift+B opens a browser tab, now on the Chromium engine",
    [
      focus,
      key("Mod+Shift+B", { key: "b", mods: ["mod", "shift"] }),
      ["wait for the engine process", async () => {
        const deadline = Date.now() + 30000;
        while (Date.now() < deadline) {
          if (enginePids().length) return { ok: true, pids: enginePids() };
          await sleep(1000);
        }
        return { ok: false, stderr: "no ferryx_engine process" };
      }],
      5000,
    ],
  );
  summary.engine.checks.engine_process_started = enginePids().length > 0;
  await step(
    "chromium-navigate-fixture",
    "Mod+L inside the Chromium tab, type the local fixture URL, Enter (CLI navigate as fallback)",
    [
      focus,
      key("Mod+L", { key: "l", mods: ["mod"] }),
      waitForEngineFocus("engine window takes keyboard focus after Mod+L"),
      ["type URL", () => host.type(`${fixtureBase}/fixture.html`)],
      ["screenshot after typing the URL", () => {
        const shot = screenshot("chromium-typed-url");
        return { ok: shot.ok, file: shot.file };
      }],
      key("Enter", { key: "Enter" }),
      5000,
      ["optional title via CLI", async () => {
        let current = sessions();
        const chromium = current.list.find((entry) => entry.engine === "chromium") ?? current.list.at(-1);
        browserId = chromium?.browserId ?? null;
        // Whether the typed URL loaded the page, before any CLI fallback.
        const typed = chromium?.title === "fixture-ready";
        summary.engine.fixture_loaded_by = typed ? "typed-url" : "cli-navigate";
        if (browserId && !typed) {
          cli("navigate", "--browser-id", browserId, "--url", `${fixtureBase}/fixture.html`);
          await sleep(5000);
          current = sessions();
        }
        const titles = current.list.map((entry) => entry.title);
        return { ok: titles.includes("fixture-ready"), via: summary.engine.fixture_loaded_by, title_after_typing: chromium?.title ?? null, titles, cli: current.raw.ok ? "ok" : current.raw.stderr, stderr: titles.includes("fixture-ready") ? "" : "fixture title not reported" };
      }],
    ],
    { any: ["Ferryx", "fixture:", "CEF"] },
  );
  summary.engine.checks.fixture_loaded = summary.steps.at(-1).actions.at(-1)?.ok ?? false;
  await step(
    "chromium-typing",
    "Click the page centre, type 'ferryxqa'; the fixture echoes it into its title and page (substring match); no downloads shelf on this tab",
    [
      focus,
      ["click page", async () => {
        const words = ocr(probeShot());
        const hint = findWord(words, "fixture:") ?? findWord(words, "CEF");
        return hint ? host.click(hint.x, hint.y + 60) : host.click(760, 520);
      }],
      waitForEngineFocus("engine window takes keyboard focus after the click"),
      ["type", () => host.type("ferryxqa")],
      2000,
      ["optional title via CLI", async () => {
        const titles = sessions().list.map((entry) => entry.title);
        return { ok: titles.includes("typed:ferryxqa"), titles, stderr: titles.includes("typed:ferryxqa") ? "" : "typed title not reported" };
      }],
    ],
    { any: ["~ferryxqa"], none: ["DOWNLOADS"] },
  );
  summary.engine.checks.typing_reaches_page = summary.steps.at(-1).ui_check?.result === "pass" || (summary.steps.at(-1).actions.at(-1)?.ok ?? false);
  await step(
    "agent-chromium",
    "Agent automation in the Chromium engine tab (CLI over DevTools, isolated world): stale ref after reload refused, snapshot, fill, click with --expect-text",
    [
      ["agent snapshot/act", () => {
        const list = sessions().list;
        const tab = list.find((entry) => entry.engine === "chromium") ?? (browserId ? { browserId } : null);
        return agentQa("chromium", tab?.browserId ?? null);
      }],
      1500,
    ],
    { any: ["agentqa", "Saved"] },
  );
  summary.engine.checks.agent_automation = summary.agent.chromium?.ok ?? false;
  await step(
    "chromium-tab-switch",
    "Mod+T (terminal tab) hides the engine view; Mod+Shift+[ returns to the Chromium tab",
    [focus, key("Mod+T", { key: "t", mods: ["mod"] }), 3000, ["screenshot hidden", () => ({ ok: screenshot("chromium-hidden-on-terminal-tab").ok })], key("Mod+Shift+[", { key: "[", mods: ["mod", "shift"] }), 3000],
    // agent-chromium navigated this tab to /agent.html, so its page/title read "agentqa" now.
    { any: ["~agentqa", "~ferryxqa", "fixture:"] },
  );
  await step(
    "engine-kill",
    "SIGKILL the engine; its tabs must close instead of leaving a blank pane",
    [
      ["kill engine", () => {
        const pids = enginePids();
        for (const pid of pids) {
          try {
            process.kill(pid, "SIGKILL");
          } catch {}
        }
        return { ok: pids.length > 0, pids, stderr: pids.length ? "" : "no engine process" };
      }],
      8000,
      ["optional tabs closed via CLI", () => {
        const list = sessions().list;
        const gone = !browserId || !list.some((entry) => entry.browserId === browserId);
        return { ok: gone, remaining: list.map((entry) => entry.browserId), stderr: gone ? "" : "engine tab still listed" };
      }],
    ],
  );
  summary.engine.checks.engine_kill_closes_tabs = summary.steps.at(-1).actions.at(-1)?.ok ?? false;
}

// ---------------------------------------------------------------- extension: install, remove, restart
// uBlock Origin Lite comes from the Chrome Web Store in an engine tab: Ferryx has no other way to
// install an extension (Settings > Browser > Extensions only opens the store). It is removed with
// the trash button of its row in Settings > Browser > Extensions, then the app restarts and the
// row and the profile's extension data must stay gone.
const UBOL_ID = "ddkjiahejlhfcafbddmgiahcphecmpfh";
const UBOL_URL = `https://chromewebstore.google.com/detail/ublock-origin-lite/${UBOL_ID}`;
/** uBlock Origin Lite in the engine's default profile data: its Extensions/<id> dir and its
 * extensions.settings entry in Preferences / Secure Preferences. */
function ubolOnDisk() {
  const root = join(dirs.data, "browser-cef", "default");
  const result = { dir: false, prefs: false, prefs_read: [] };
  for (const profile of [join(root, "Default"), root]) {
    if (existsSync(join(profile, "Extensions", UBOL_ID))) result.dir = true;
    for (const file of ["Preferences", "Secure Preferences"]) {
      const path = join(profile, file);
      if (!existsSync(path)) continue;
      try {
        const settings = JSON.parse(readFileSync(path, "utf8"))?.extensions?.settings ?? {};
        result.prefs_read.push(`${basename(profile)}/${file}`);
        if (settings[UBOL_ID]) result.prefs = true;
      } catch {}
    }
  }
  return result;
}
/** OCR words of the extension's row label ("uBlock Origin Lite"). */
const ubolWords = (words) => (words ?? []).filter((word) => /^u?b[li1|]ock$/i.test(word.text.replace(/[^a-z0-9|]/gi, "")));
/** Settings > Browser > Extensions on screen: opens Settings, the Browser nav, scrolls to the group. */
const openExtensionsGroup = [
  closeStrayDialogs,
  focus,
  chromeFocus,
  ["open Settings", openSettings],
  dismissStoreToast,
  ["click nav Browser", () => clickNav("Browser")],
  2000,
  ["scroll to Extensions", () => scrollSettingsTo("Extensions")],
  3000,
];
if (engineZip && summary.engine.checks.installed_from_settings) {
  const record = {
    extension: "uBlock Origin Lite",
    id: UBOL_ID,
    install_path: "Chrome Web Store page in a Chromium engine tab ('Add to Chrome', then Chrome's 'Add extension' dialog)",
    checks: Object.fromEntries(["installed_from_web_store", "listed_in_settings", "removed_from_settings", "removed_from_profile", "stays_removed_after_restart"].map((name) => [name, false])),
  };
  summary.extension = record;
  let storeTab = null;
  let installButton = null;
  let confirmPrompt = null;
  let confirmed = false;
  let cliRemove = null;
  await step(
    "extension-store-open",
    "Mod+Shift+B opens a Chromium tab; the CLI navigates it to uBlock Origin Lite's Chrome Web Store page",
    [
      focus,
      chromeFocus,
      key("Mod+Shift+B", { key: "b", mods: ["mod", "shift"] }),
      ["navigate to the store page (CLI)", async () => {
        const deadline = Date.now() + 30000;
        while (Date.now() < deadline && !storeTab) {
          storeTab = sessions().list.find((entry) => entry.engine === "chromium") ?? null;
          if (!storeTab) await sleep(1000);
        }
        if (!storeTab) return { ok: false, stderr: "no chromium tab listed" };
        const navigated = await cliAsync("navigate", "--browser-id", storeTab.browserId, "--url", UBOL_URL);
        return { ok: navigated.ok, browser_id: storeTab.browserId, stderr: navigated.stderr };
      }],
      waitForText(["Chrome"], 45000),
    ],
    { any: ["Chrome", "uBlock"] },
  );
  await step(
    "extension-store-install",
    "Click 'Add to Chrome', then 'Add extension' in Chrome's dialog; wait until the extension is in the profile",
    [
      // The store shows "Switch to Chrome?" to every Chromium that is not Google Chrome; it does
      // not block the install. It can sit off screen right of the pane, so it is answered in the
      // page.
      ["optional dismiss the 'Switch to Chrome?' prompt (CLI eval)", async () => {
        const script = "(() => { const b = [...document.querySelectorAll('button,a,[role=button]')].find((e) => (e.textContent || '').trim() === 'No thanks'); if (!b) return 'absent'; b.click(); return 'dismissed'; })()";
        const result = storeTab ? await cliAsync("eval", "--browser-id", storeTab.browserId, "--script", script) : { ok: false, stderr: "no store tab" };
        record.switch_to_chrome_prompt = result.ok ? result.stdout.replace(/"/g, "") : null;
        return { ok: result.ok, prompt: record.switch_to_chrome_prompt, stderr: result.stderr };
      }],
      1500,
      // The store page is wider than the pane: the CLI scrolls the install button into view and
      // gives it dark-on-white colours so OCR reads its label (white on blue it reads nothing);
      // the click on it is still real OS input.
      ["scroll the install button into view (CLI eval)", async () => {
        // Also reports the "Share" link's rect, a fallback anchor from page CSS pixels to screen.
        // Centred: on the 1024px Windows runner screen the window is wider than the screen, so a
        // button at the pane's right edge is off screen.
        const script = "(() => { const b = [...document.querySelectorAll('button')].find((e) => /Add to Chrome/i.test(e.textContent || '')); if (!b) return 'none'; b.scrollIntoView({block: 'center', inline: 'center'}); b.style.setProperty('background', '#ffffff', 'important'); b.style.setProperty('color', '#000000', 'important'); b.querySelectorAll('*').forEach((e) => e.style.setProperty('color', '#000000', 'important')); const r = b.getBoundingClientRect(); const s = [...document.querySelectorAll('a,button,span')].find((e) => (e.textContent || '').trim() === 'Share'); const q = s && s.getBoundingClientRect(); return JSON.stringify({x: Math.round(r.x + r.width / 2), y: Math.round(r.y + r.height / 2), disabled: b.disabled, text: b.textContent.trim(), share: q ? {x: Math.round(q.x + q.width / 2), y: Math.round(q.y + q.height / 2)} : null, dpr: devicePixelRatio, vw: innerWidth}); })()";
        const deadline = Date.now() + 30000;
        let result = null;
        do {
          result = storeTab ? await cliAsync("eval", "--browser-id", storeTab.browserId, "--script", script) : { ok: false, stderr: "no store tab" };
          if (result.ok && !result.stdout.includes("none")) {
            try {
              let parsed = JSON.parse(result.stdout);
              if (typeof parsed === "string") parsed = JSON.parse(parsed);
              installButton = parsed;
            } catch {}
            return { ok: Boolean(installButton), button: installButton ?? result.stdout };
          }
          await sleep(2000);
        } while (Date.now() < deadline);
        record.error = "the store page offers no 'Add to Chrome' button";
        return { ok: false, last: result?.stdout ?? null, stderr: result?.stderr || "no 'Add to Chrome' button in the page" };
      }],
      1500,
      ["click Add to Chrome", async () => {
        const deadline = Date.now() + 20000;
        do {
          const words = ocr(probeShot()) ?? [];
          const hit = findPhrase(words, "Add to Chrome")[0];
          if (hit) return { ...host.click(hit.x, hit.y), at: [hit.x, hit.y], via: "ocr" };
          // The label is white on blue: map the button's page rect through the OCR'd "Share" link.
          const share = installButton?.share && installButton.share.x > 0 ? words.find((word) => word.text === "Share") : null;
          if (share) {
            const scale = installButton.dpr || 1;
            const at = [
              Math.round(share.left + share.width / 2 + (installButton.x - installButton.share.x) * scale),
              Math.round(share.top + share.height / 2 + (installButton.y - installButton.share.y) * scale),
            ];
            return { ...host.click(...at), at, via: "page rect anchored on 'Share'" };
          }
          await sleep(1500);
        } while (Date.now() < deadline);
        record.error ??= `'Add to Chrome' could not be located on screen (page rect ${JSON.stringify(installButton)})`;
        return { ok: false, stderr: "'Add to Chrome' not visible" };
      }],
      ["click Add extension", async () => {
        const deadline = Date.now() + 30000;
        do {
          await sleep(1500);
          const words = ocr(probeShot()) ?? [];
          const hit = findPhrase(words, "Add extension").at(-1);
          if (hit) return { ...host.click(hit.x, hit.y), at: [hit.x, hit.y] };
        } while (Date.now() < deadline);
        const text = storeTab ? await cliAsync("eval", "--browser-id", storeTab.browserId, "--script", "(document.body && document.body.innerText || '').slice(0, 600)") : null;
        record.error ??= "Chrome's 'Add extension' dialog did not appear after clicking 'Add to Chrome'";
        record.page_after_click = text?.stdout ?? null;
        return { ok: false, stderr: "Chrome's 'Add extension' button not visible" };
      }],
      ["wait for the extension in the profile", async () => {
        const deadline = Date.now() + 120000;
        let disk = ubolOnDisk();
        while (!disk.dir && Date.now() < deadline) {
          await sleep(2000);
          disk = ubolOnDisk();
        }
        if (!disk.dir) record.error ??= `no Extensions/${UBOL_ID} in the engine profile after 120s`;
        return { ok: disk.dir, disk, stderr: disk.dir ? "" : `no Extensions/${UBOL_ID} in the engine profile after 120s` };
      }],
      // Chrome commits Preferences on a 10s timer; the Settings list reads them when no engine runs.
      12000,
    ],
    { any: ["added", "uBlock"] },
  );
  record.checks.installed_from_web_store = summary.steps.at(-1).actions.at(-1)?.ok ?? false;
  await step("extension-store-tab-closed", "Close the store tab (CLI) so its page cannot show through Settings", [
    ["close the store tab", () => (storeTab ? cli("close", "--browser-id", storeTab.browserId) : { ok: false, stderr: "no store tab" })],
    2000,
  ]);
  let row = null;
  await step(
    "extension-listed",
    "Settings > Browser > Extensions lists uBlock Origin Lite",
    [
      ...openExtensionsGroup,
      ["find the uBlock Origin Lite row", async () => {
        const deadline = Date.now() + 30000;
        do {
          const path = probeShot();
          const label = ubolWords(ocr(path))[0];
          if (label) {
            const word = { ...label, x: label.left + Math.round(label.width / 2), y: label.top + Math.round(label.height / 2), right: label.left + label.width };
            // The name is the row's first line; the switch is centred on the whole (3-line) row,
            // about 17px lower, so the probe looks further down than for single-line rows.
            row = { label: word, knob: findRowSwitch(path, word, { below: 32 }), icons: findRowIcons(path, word) };
            return { ok: true, label_at: [word.x, word.y], switch: row.knob, icons: row.icons };
          }
          await sleep(2000);
        } while (Date.now() < deadline);
        return { ok: false, stderr: "no uBlock row in the Extensions group" };
      }],
    ],
    { any: ["uBlock", "Origin"] },
  );
  record.checks.listed_in_settings = Boolean(row);
  await step(
    "extension-remove",
    "Click the row's trash button (left of its switch), confirm with Remove, then Remove in Chrome's own dialog; the row disappears",
    [
      ["click the trash button", async () => {
        // The row ends in pin, trash and the switch: the trash is the control left of the switch.
        // The controls are centred on the row, about 17px below its first-line name.
        const candidates = [];
        if (row?.icons?.length >= 2) candidates.push([row.icons.at(-2).x, row.label.y + 17]);
        if (row?.knob) {
          // Fallback: switch w-9 (36px), gap-1.5, trash size-6: 18px left of the switch's edge.
          const scale = row.knob.width / 34;
          const switchLeft = row.knob.x - row.knob.width / 2;
          for (const offset of [18, 22, 14]) candidates.push([Math.round(switchLeft - offset * scale), row.knob.y]);
        }
        if (!candidates.length) return { ok: false, stderr: "row controls not located" };
        for (const at of candidates) {
          host.click(...at);
          await sleep(1200);
          const words = ocr(probeShot()) ?? [];
          const prompt = findPhrase(words, "Chrome may ask")[0] ?? findPhrase(words, "once more")[0];
          if (prompt) {
            confirmPrompt = prompt;
            return { ok: true, at, tried: candidates.indexOf(at) + 1 };
          }
        }
        return { ok: false, stderr: "the remove confirmation did not appear" };
      }],
      // The confirmation's Remove is white text on the destructive red: OCR does not read it
      // (runs 41635903718 and 41642552143 clicked off it), so it is the red button on the
      // prompt's line, right of the prompt. Only that one: the Chromium Engine group's own
      // "Remove" uninstalls the engine.
      ["click Remove (Ferryx confirmation)", async () => {
        if (!confirmPrompt) return { ok: false, stderr: "no row confirmation open" };
        const path = probeShot();
        const button = findRedButton(path, confirmPrompt);
        if (!button) return { ok: false, probe: basename(path), stderr: "no red Remove button on the confirmation line" };
        const at = [button.x, button.y];
        const clicked = host.click(...at);
        await sleep(1500);
        // The confirmation closes when its Remove is clicked (the row's change starts).
        const after = ocr(probeShot()) ?? [];
        confirmed = !(findPhrase(after, "Chrome may ask").length || findPhrase(after, "once more").length);
        return { ...clicked, ok: clicked.ok && confirmed, at, button, stderr: confirmed ? "" : "the confirmation stayed open after clicking its Remove" };
      }],
      // The product path from here: Ferryx asks the engine to uninstall and Chrome shows its own
      // dialog. When the click above did not get there, `ferryx browser extensions remove` runs
      // the same engine call (the CLI's admitted path), and Chrome's dialog is answered below.
      ["optional CLI fallback (ferryx browser extensions remove)", () => {
        if (confirmed) return { ok: true, skipped: "the Settings row started the removal" };
        cliRemove = cliAsync("extensions", "remove", UBOL_ID);
        record.removed_via = "cli";
        return { ok: true, started: true };
      }],
      ["click Remove in Chrome's dialog", async () => {
        if (confirmed) record.removed_via = "settings-row";
        // Chrome's uninstall dialog: 'Remove "uBlock Origin Lite"?' over Cancel and Remove, with
        // Remove right of Cancel on one line. Ferryx's own rows have Remove buttons too (the
        // engine's, the confirmation's Remove left of its Cancel), so only that pair counts.
        const deadline = Date.now() + 30000;
        do {
          await sleep(1500);
          const words = ocr(probeShot()) ?? [];
          const cancels = words.filter((word) => word.text === "Cancel");
          const hit = words.find((word) => word.text === "Remove"
            && cancels.some((cancel) => Math.abs(cancel.top - word.top) <= 12 && word.left > cancel.left && word.left - (cancel.left + cancel.width) < 120));
          if (hit) {
            const at = [hit.left + Math.round(hit.width / 2), hit.top + Math.round(hit.height / 2)];
            return { ...host.click(...at), at };
          }
          if (!ubolOnDisk().prefs && !ubolWords(words).length) return { ok: true, skipped: "already removed, no Chrome dialog" };
        } while (Date.now() < deadline);
        return { ok: false, stderr: "Chrome's Remove dialog not visible" };
      }],
      ["optional CLI fallback answered", async () => {
        if (!cliRemove) return { ok: true, skipped: "no CLI fallback" };
        const result = await cliRemove;
        let listing = null;
        try {
          listing = JSON.parse(result.stdout);
        } catch {}
        record.cli_remove = { ok: result.ok, status: result.status, still_listed: listing ? listing.extensions?.some((entry) => entry.id === UBOL_ID) : null, stderr: result.stderr.slice(0, 400) };
        return { ok: result.ok, ...record.cli_remove };
      }],
      ["wait for the row to disappear", async () => {
        const deadline = Date.now() + 60000;
        let words = [];
        do {
          await sleep(2000);
          words = ocr(probeShot()) ?? [];
          if (!ubolWords(words).length && findSettingsHeading(words, "Extensions").heading) return { ok: true };
        } while (Date.now() < deadline);
        return { ok: false, stderr: "uBlock row still listed (or the Extensions group is off screen)", seen: ubolWords(words).map((word) => word.text) };
      }],
    ],
    { none: ["uBlock"] },
  );
  // Removal checks only count once the extension was really installed: without it the row and
  // the profile data are absent anyway.
  const installed = record.checks.installed_from_web_store;
  record.checks.removed_from_settings = installed && (summary.steps.at(-1).actions.at(-1)?.ok ?? false);
  // Which product path removed it: the Settings row (real clicks) or the CLI's extensions
  // remove (the same engine call); either way Chrome's own dialog was answered by a click.
  record.removed_via ??= null;
  // Chrome commits Preferences on a 10s timer.
  await sleep(15000);
  record.disk_after_remove = ubolOnDisk();
  record.checks.removed_from_profile = installed && !record.disk_after_remove.prefs && !record.disk_after_remove.dir;
  await step("extension-settings-closed", "Escape closes Settings", [focus, key("Escape", { key: "Escape" }), 1500]);

  // Restart: stop only the app process, so its engine sees stdin close and quits by itself
  // (as when the user quits Ferryx), then launch the same binary on the same data dirs.
  await step("extension-restart", "Quit the app (its engine follows), launch it again on the same data", [
    ["stop the app", async () => {
      const old = app;
      app = null;
      const exited = new Promise((done) => old.once("exit", done));
      if (platform === "win32") run("taskkill", ["/PID", String(old.pid), "/F"]);
      else old.kill("SIGTERM");
      const stopped = await Promise.race([exited.then(() => true), sleep(20000).then(() => false)]);
      const deadline = Date.now() + 30000;
      while (enginePids().length && Date.now() < deadline) await sleep(1000);
      const leftover = enginePids();
      for (const pid of leftover) {
        try {
          process.kill(pid, "SIGKILL");
        } catch {}
      }
      return { ok: stopped, engine_left_running: leftover, stderr: stopped ? "" : "the app did not exit within 20s" };
    }],
    ["launch again", async () => {
      app = launchApp("app-restart.log");
      record.restart_window = await waitForWindow();
      return { ok: record.restart_window.found, pid: app.pid, stderr: record.restart_window.found ? "" : "no window after the restart" };
    }],
    ["optional dismiss onboarding", () => clickText("Skip", { timeout: 4000 })],
  ]);
  record.restarted = summary.steps.at(-1).actions.slice(0, 2).every((action) => action.ok);
  await step(
    "extension-after-restart",
    "After the restart, Settings > Browser > Extensions still has no uBlock Origin Lite row",
    [
      ...openExtensionsGroup,
      ["the group is shown without the uBlock row", async () => {
        let words = [];
        for (let attempt = 0; attempt < 5; attempt += 1) {
          words = ocr(probeShot()) ?? [];
          if (findSettingsHeading(words, "Extensions").heading && findPhrase(words, "Open Chrome Web Store").length) break;
          await sleep(2000);
        }
        const heading = Boolean(findSettingsHeading(words, "Extensions").heading);
        const listed = ubolWords(words).length > 0;
        return { ok: heading && !listed, heading, listed, empty_message: findPhrase(words, "No extensions installed").length > 0, stderr: !heading ? "Extensions group not on screen" : listed ? "uBlock row is back" : "" };
      }],
    ],
    { any: ["Extensions"], none: ["uBlock"] },
  );
  record.disk_after_restart = ubolOnDisk();
  record.checks.stays_removed_after_restart = installed
    && record.restarted
    && (summary.steps.at(-1).actions.at(-1)?.ok ?? false)
    && !record.disk_after_restart.prefs
    && !record.disk_after_restart.dir;
  await step("extension-settings-closed-after-restart", "Escape closes Settings", [focus, key("Escape", { key: "Escape" }), 1500]);
  writeSummary();
}

// ---------------------------------------------------------------- teardown
await teardown();

/** Totals, stop the app, its engine and the fixture server, print the verdict, exit 0. */
async function teardown() {
  summary.finished_at = new Date().toISOString();
  summary.totals = summary.steps.reduce((counts, entry) => ({ ...counts, [entry.outcome]: (counts[entry.outcome] ?? 0) + 1 }), {});
  summary.ui_checks = summary.steps.reduce((counts, entry) => (entry.ui_check ? { ...counts, [entry.ui_check.result]: (counts[entry.ui_check.result] ?? 0) + 1 } : counts), {});
  writeSummary();
  if (app && !summary.app_exit) {
    if (platform === "win32") run("taskkill", ["/PID", String(app.pid), "/T", "/F"]);
    else {
      try {
        process.kill(-app.pid, "SIGTERM");
      } catch {}
    }
  }
  for (const pid of enginePids()) {
    try {
      process.kill(pid, "SIGKILL");
    } catch {}
  }
  // A local engine dir is launched on every OS; do not leave it running for the next step.
  if (scenario === "drag-mask") {
    if (platform === "win32") run("taskkill", ["/F", "/IM", "ferryx_engine.exe"]);
    else run("pkill", ["-f", "ferryx_engine"]);
  }
  await sleep(1500);
  server.stop(true);
  writeSummary();
  const dragMask = summary.drag_mask;
  console.log(JSON.stringify({
    scenario,
    totals: summary.totals,
    ui_checks: summary.ui_checks,
    engine: summary.engine.checks ?? summary.engine.skipped,
    agent: Object.fromEntries(Object.entries(summary.agent).map(([engine, record]) => [engine, { ok: record.ok ?? false, checks: record.checks }])),
    ...(dragMask
      ? {
          drag_mask: {
            pass: dragMask.pass ?? false,
            chromium: dragMask.chromium ?? null,
            visible_before: dragMask.visible_before ?? null,
            masked_mid_drag: dragMask.masked_mid_drag ?? null,
            restored_after_drop: dragMask.restored_after_drop ?? null,
            green: [dragMask.before?.green, dragMask.mid_drag?.green, dragMask.after?.green],
          },
        }
      : {}),
  }));
  process.exit(0);
}
