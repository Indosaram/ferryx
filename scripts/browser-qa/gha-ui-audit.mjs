#!/usr/bin/env bun
// Real-OS-input UI audit for the Ferryx built-in browser, run on GitHub-hosted runners.
//
// Launches the debug executable with isolated FERRYX_* directories, drives it only through the
// host's own input injection (Linux xdotool, macOS osascript/cliclick, Windows SendInput), finds
// on-screen labels with tesseract OCR so clicks land on real controls, and captures a full-screen
// screenshot after every step. A missing UI element never fails the run: each step's outcome and
// its visible-text check go to summary.json, and the script exits 0 once it has started.
//
// Optional Chromium engine checks (Linux): set FERRYX_UI_AUDIT_ENGINE_ZIP to a bundled
// engine-linux-<arch>.zip. The script serves it with a manifest signed by a throwaway ed25519 key
// that only debug builds accept (FERRYX_BROWSER_ENGINE_PUBLIC_KEY), installs it from
// Settings > Browser, opens a Chromium tab, types into a fixture page and kills the engine.
//
// Usage: bun scripts/browser-qa/gha-ui-audit.mjs --binary <path> --out <dir>
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
const shots = join(out, "screenshots");
const ocrDir = join(out, "ocr");
mkdirSync(shots, { recursive: true });
mkdirSync(ocrDir, { recursive: true });

const isolation = join(process.env.RUNNER_TEMP ?? out, "ferryx-ui-audit-isolation");
const dirs = Object.fromEntries(["data", "runtime", "session", "tmp", "home"].map((name) => [name, join(isolation, name)]));
for (const dir of Object.values(dirs)) mkdirSync(dir, { recursive: true });

const engineZip = process.env.FERRYX_UI_AUDIT_ENGINE_ZIP && existsSync(process.env.FERRYX_UI_AUDIT_ENGINE_ZIP)
  ? resolve(process.env.FERRYX_UI_AUDIT_ENGINE_ZIP)
  : null;

const summary = {
  platform,
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
  ],
  engine: engineZip ? { zip: engineZip, checks: {} } : { skipped: "FERRYX_UI_AUDIT_ENGINE_ZIP not set or missing" },
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
      [`linux-${process.arch === "arm64" ? "aarch64" : "x86_64"}`]: {
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
    return { Tab: "Tab", Enter: "Return", Escape: "Escape", Space: "space", ",": "comma", "=": "equal" }[key] ?? key;
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
const winVk = { Tab: 0x09, Enter: 0x0d, Escape: 0x1b, Space: 0x20, ",": 0xbc, "=": 0xbb, mod: 0x11, ctrl: 0x11, shift: 0x10, alt: 0x12 };
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
  "closedialogs" { Write-Output ([FxInput]::CloseDialogs([uint32]$ProcessId)) }
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
function findRowSwitch(path, label) {
  const image = decodePng(path);
  const votes = new Map();
  for (let dy = -12; dy <= 12; dy += 1) {
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
  return { x: Math.round((winner.run[0] + winner.run[1]) / 2), y: ys[Math.floor(ys.length / 2)], lines: winner.count };
}

const norm = (text) => text.toLowerCase().replace(/[^a-z0-9:%./]/g, "");
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

const appLog = openSync(join(out, "app.log"), "w");
let app = null;
if (!existsSync(binary)) {
  summary.launch = { ok: false, error: `binary not found: ${binary}` };
} else {
  app = spawn(binary, [], { env, stdio: ["ignore", appLog, appLog], cwd: dirs.tmp, detached: platform !== "win32" });
  app.on("exit", (code, signal) => {
    summary.app_exit = { code, signal, at: new Date().toISOString(), during_teardown: Boolean(summary.finished_at) };
    writeSummary();
  });
  summary.launch = { ok: true, pid: app.pid };
  mac.pid = win.pid = app.pid;
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
      const seen = (list) => list.filter((text) => words.some((word) => norm(word.text) === norm(text) || (text.endsWith("*") && norm(word.text).startsWith(norm(text.slice(0, -1))))));
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
if (app) {
  const deadline = Date.now() + 90000;
  let windowId = null;
  while (!windowId && Date.now() < deadline && !summary.app_exit) {
    await sleep(2000);
    windowId = host.findWindow(app.pid);
  }
  linux.windowId = windowId;
  win.hwnd = windowId;
  summary.window = { found: Boolean(windowId), id: windowId, waited_ms: 90000 - Math.max(0, deadline - Date.now()) };
  await sleep(15000); // first render of the web UI and daemon startup
}

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

// ---------------------------------------------------------------- base flow
await step("launch", "App window after startup", [focus, dismissStoreToast]);
await step(
  "dismiss-onboarding",
  "Click 'Skip setup' in the first-run wizard (fallback: Tab into the wizard, Escape)",
  [focus, clickOr("Skip", [key("Tab", { key: "Tab" }), key("Escape", { key: "Escape" })], { timeout: 5000 }), 2000],
  { none: ["Skip"] },
);
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
      // The Import step left the pane scrolled to the bottom; Chromium Engine sits above Extensions.
      ["scroll up to the Chromium Engine group", () => scrollSettingsTo("Chromium Engine", { direction: -1 })],
      // Rightmost: the group also has a "Download size" row label on the left.
      ["click Download", () => clickText("Download", { timeout: 8000, match: exactly("Download"), pick: "rightmost" })],
      3000,
      ["wait for ferryx_engine on disk", async () => {
        const root = join(dirs.data, "browser-engine");
        const deadline = Date.now() + 240000;
        while (Date.now() < deadline) {
          if (existsSync(root)) {
            for (const name of readdirSync(root)) {
              if (existsSync(join(root, name, "ferryx_engine"))) return { ok: true, dir: join(root, name) };
            }
          }
          await sleep(2000);
        }
        return { ok: false, stderr: `no ferryx_engine under ${root}` };
      }],
      2000,
    ],
    { any: ["Remove", "Installed"] },
  );
  summary.engine.checks.installed_from_settings = summary.steps.at(-1).actions.find((action) => action.label === "wait for ferryx_engine on disk")?.ok ?? false;
}

await step("settings-closed", "Escape closes Settings", [focus, key("Escape", { key: "Escape" }), 1500], { none: ["Appearance"] });

// ---------------------------------------------------------------- page interactions (every OS)
await step(
  "download-trigger",
  "Load the local download page and click its download link; the download prompt shows ferryx-qa.txt",
  [closeStrayDialogs, focus, dismissStoreToast, ...typeUrl(`${fixtureBase}/download.html`), waitForText(["FERRYXDOWNLOAD"]), ["click download link", () => clickText("FERRYXDOWNLOAD", { timeout: 15000 })], 4000],
  { any: ["ferryx-qa.txt", "Save", "Download"] },
);
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
    const strip = path.replace(/\.png$/, ".findcount.png");
    const box = `120x28+${query.left + 600}+${Math.max(0, query.y - 14)}`;
    run(imageMagick, [path, "-crop", box, "+repage", "-colorspace", "gray", "-normalize", "-resize", "600%", strip]);
    const text = run(tesseract, [strip, "-", "--psm", "7"]).stdout.replace(/\s+/g, " ").trim();
    const count = /(\d+|[GbS])?\s*mat[ce][hn]es/i.exec(text);
    return { ok: Boolean(count), text, box, seen_text: count ? ["matches"] : [], stderr: count ? "" : "match count not read" };
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

// ---------------------------------------------------------------- Chromium engine checks (Linux)
const enginePids = () => (platform === "linux" ? run("pgrep", ["-f", "ferryx_engine"]).stdout.split(/\s+/).filter(Boolean).map(Number) : []);
let xStateIndex = 0;
/** Writes the X window tree, the raw focus window and its properties to x-state/<n>-<name>.txt. */
const captureXState = (name) => {
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
/** Waits until X keyboard focus leaves the Ferryx toplevel for the engine's pane window (the
 * engine moves it asynchronously after Mod+L or a click), bounded by `timeout`. Reads the raw
 * focus window (`-f`): the engine window is a child of the Ferryx window without WM_STATE, and
 * plain `getwindowfocus` walks up from it to the Ferryx client window. */
const waitForEngineFocus = (label, timeout = 8000) => [label, async () => {
  const deadline = Date.now() + timeout;
  let focused = "";
  while (Date.now() < deadline) {
    focused = run("xdotool", ["getwindowfocus", "-f"]).stdout;
    if (/^\d+$/.test(focused) && Number(focused) > 1 && focused !== String(linux.windowId)) {
      return { ok: true, focused, ferryx: linux.windowId, x_state: captureXState(label.replace(/\W+/g, "-")).file };
    }
    await sleep(200);
  }
  return { ok: false, focused, ferryx: linux.windowId, x_state: captureXState(label.replace(/\W+/g, "-")).file, stderr: "X keyboard focus stayed off the engine window" };
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
      key("Enter", { key: "Enter" }),
      5000,
      ["optional title via CLI", async () => {
        let current = sessions();
        const chromium = current.list.find((entry) => entry.engine === "chromium") ?? current.list.at(-1);
        browserId = chromium?.browserId ?? null;
        if (browserId && chromium.title !== "fixture-ready") {
          cli("navigate", "--browser-id", browserId, "--url", `${fixtureBase}/fixture.html`);
          await sleep(5000);
          current = sessions();
        }
        const titles = current.list.map((entry) => entry.title);
        return { ok: titles.includes("fixture-ready"), titles, cli: current.raw.ok ? "ok" : current.raw.stderr, stderr: titles.includes("fixture-ready") ? "" : "fixture title not reported" };
      }],
    ],
    { any: ["Ferryx", "fixture:", "CEF"] },
  );
  summary.engine.checks.fixture_loaded = summary.steps.at(-1).actions.at(-1)?.ok ?? false;
  await step(
    "chromium-typing",
    "Click the page centre, type 'ferryxqa'; the fixture echoes it into its title",
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
    { any: ["ferryxqa"] },
  );
  summary.engine.checks.typing_reaches_page = summary.steps.at(-1).ui_check?.result === "pass" || (summary.steps.at(-1).actions.at(-1)?.ok ?? false);
  await step(
    "chromium-tab-switch",
    "Mod+T (terminal tab) hides the engine view; Mod+Shift+[ returns to the Chromium tab",
    [focus, key("Mod+T", { key: "t", mods: ["mod"] }), 3000, ["screenshot hidden", () => ({ ok: screenshot("chromium-hidden-on-terminal-tab").ok })], key("Mod+Shift+[", { key: "bracketleft", mods: ["mod", "shift"] }), 3000],
    { any: ["ferryxqa", "fixture:", "CEF"] },
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

// ---------------------------------------------------------------- teardown
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
await sleep(1500);
server.stop(true);
writeSummary();
console.log(JSON.stringify({ totals: summary.totals, ui_checks: summary.ui_checks, engine: summary.engine.checks ?? summary.engine.skipped }));
process.exit(0);
