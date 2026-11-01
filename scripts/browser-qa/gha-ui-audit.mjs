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
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  public static IntPtr FindTopWindow(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { found = h; return false; } return true; }, IntPtr.Zero);
    return found;
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
  public static uint Click(int x, int y) {
    SetCursorPos(x, y);
    return SendInput(2, new INPUT[] { Mouse(0x0002), Mouse(0x0004) }, Marshal.SizeOf(typeof(INPUT)));
  }
  public static bool Focus(IntPtr h) {
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
  "resize" { if (-not [FxInput]::SetWindowPos($handle, [IntPtr]::Zero, 40, 40, $Width, $Height, 0x0004)) { exit 5 } }
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
function ocr(path) {
  if (!tesseract || !path || !existsSync(path)) return null;
  const result = run(tesseract, [path, "-", "--psm", "11", "tsv"], { timeout: 90000 });
  if (!result.ok) return null;
  const lines = result.stdout.split(/\r?\n/);
  const header = lines.shift()?.split("\t") ?? [];
  const col = (name) => header.indexOf(name);
  return lines
    .map((line) => line.split("\t"))
    .filter((cells) => cells.length >= header.length && cells[col("text")]?.trim() && Number(cells[col("conf")]) > 0)
    .map((cells) => ({
      text: cells[col("text")].trim(),
      left: Number(cells[col("left")]),
      top: Number(cells[col("top")]),
      width: Number(cells[col("width")]),
      height: Number(cells[col("height")]),
    }));
}
const norm = (text) => text.toLowerCase().replace(/[^a-z0-9:%./]/g, "");
function findWord(words, text, { nth = 0, minX = 0 } = {}) {
  const hits = (words ?? []).filter((word) => norm(word.text) === norm(text) && word.left >= minX).sort((a, b) => a.top - b.top || a.left - b.left);
  const word = hits[nth];
  return word ? { x: word.left + Math.round(word.width / 2), y: word.top + Math.round(word.height / 2), hits: hits.length } : null;
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
    const words = ocr(shot.path);
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

await step("launch", "App window after startup", [focus]);
await step(
  "dismiss-onboarding",
  "Click 'Skip setup' in the first-run wizard (fallback: Tab into the wizard, Escape)",
  [focus, clickOr("Skip", [key("Tab", { key: "Tab" }), key("Escape", { key: "Escape" })], { timeout: 5000 }), 2000],
  { none: ["Skip"] },
);
await step("new-browser-tab", "Mod+Shift+B (tab.newBrowser) opens a browser tab", [focus, key("Mod+Shift+B", { key: "b", mods: ["mod", "shift"] }), 5000], {
  any: ["about:blank", "Browser", "Search"],
});
summary.cli_after_new_tab = sessions().raw;
await step(
  "address-bar-example",
  "Mod+L, type https://example.com, Enter",
  [focus, key("Mod+L", { key: "l", mods: ["mod"] }), ["type URL", () => host.type("https://example.com")], key("Enter", { key: "Enter" }), 8000],
  { any: ["Domain", "illustrative"] },
);
await step("settings-open", "Mod+, opens Settings (General)", [focus, key("Mod+,", { key: ",", mods: ["mod"] }), 2000], { any: ["General", "Appearance"] });
await step(
  "settings-enable-advanced",
  "Tab x11 from 'Back to app' reaches General > Advanced features; Space turns it on (Browser/Notifications appear in the nav)",
  [focus, ...repeat(11, "Tab", { key: "Tab" }), key("Space", { key: "Space" }), 1500],
  { any: ["Notifications"] },
);
await step(
  "settings-browser",
  "Click 'Browser' in the Settings nav (fallback: reopen Settings, Tab x6, Enter)",
  [
    focus,
    clickOr("Browser", [key("Escape", { key: "Escape" }), key("Mod+,", { key: ",", mods: ["mod"] }), ...repeat(6, "Tab", { key: "Tab" }), key("Enter", { key: "Enter" })]),
    2500,
  ],
  { any: ["Chromium", "Engine", "Search"] },
);

if (engineZip) {
  await step(
    "engine-install",
    "Settings > Browser > Chromium Engine: click 'Download', wait for the install",
    [
      focus,
      ["optional scroll to the engine group", async () => {
        for (let attempt = 0; attempt < 4; attempt += 1) {
          const words = ocr(probeShot());
          if (findWord(words, "Chromium")) return { ok: true };
          if (platform === "linux") run("xdotool", ["mousemove", "1100", "600", "click", "--repeat", "8", "5"]);
          await sleep(600);
        }
        return { ok: false, stderr: "Chromium group not visible" };
      }],
      ["click Download", () => clickText("Download", { timeout: 8000 })],
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
await step("narrow-900", "Resize the window to 900x800", [focus, ["resize 900x800", () => host.resize(900, 800)], 2500]);
await step(
  "downloads-shelf",
  "Navigate to a local attachment URL so the downloads shelf appears",
  [focus, key("Mod+L", { key: "l", mods: ["mod"] }), ["type URL", () => host.type(`${fixtureBase}/download/ferryx-qa.txt`)], key("Enter", { key: "Enter" }), 8000],
  { any: ["Downloads*", "ferryx-qa.txt", "Save"] },
);
await step(
  "back-to-example",
  "Mod+L, https://example.com, Enter (page for find and zoom)",
  [focus, key("Mod+L", { key: "l", mods: ["mod"] }), ["type URL", () => host.type("https://example.com")], key("Enter", { key: "Enter" }), 6000],
  { any: ["Domain", "illustrative"] },
);
await step("find-bar", "Mod+F opens the find bar; type 'Example'", [focus, key("Mod+F", { key: "f", mods: ["mod"] }), 1000, ["type query", () => host.type("Example")], 1500], {
  any: ["1/1", "1/2", "1/3", "of", "matches"],
});
await step(
  "zoom-in",
  "Escape closes find, Mod+= twice zooms in",
  [focus, key("Escape", { key: "Escape" }), key("Mod+= 1", { key: "=", mods: ["mod"] }), key("Mod+= 2", { key: "=", mods: ["mod"] }), 2000],
  { any: ["110%", "120%", "125%"] },
);

// ---------------------------------------------------------------- Chromium engine checks (Linux)
const enginePids = () => (platform === "linux" ? run("pgrep", ["-f", "ferryx_engine"]).stdout.split(/\s+/).filter(Boolean).map(Number) : []);
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
