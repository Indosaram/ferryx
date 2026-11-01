#!/usr/bin/env bun
// Real-OS-input UI audit for the Ferryx built-in browser, run on GitHub-hosted runners.
//
// Launches the debug executable with isolated FERRYX_* directories, drives it with the host's
// own input injection (xdotool / osascript+cliclick / Win32 SendInput), and captures a
// full-screen screenshot after every step. A missing UI element never fails the run: each
// step's outcome is written to summary.json and the script always exits 0 once it starts.
//
// Usage: bun scripts/browser-qa/gha-ui-audit.mjs --binary <path> --out <dir>
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync, existsSync, openSync } from "node:fs";
import { join, resolve } from "node:path";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, value, index, all) => {
    if (value.startsWith("--")) pairs.push([value.slice(2), all[index + 1]]);
    return pairs;
  }, []),
);
const platform = process.platform; // linux | darwin | win32
const isMac = platform === "darwin";
const binary = resolve(args.binary ?? "");
const out = resolve(args.out ?? "ui-audit");
const shots = join(out, "screenshots");
mkdirSync(shots, { recursive: true });

const isolation = join(process.env.RUNNER_TEMP ?? out, "ferryx-ui-audit-isolation");
const dirs = Object.fromEntries(["data", "runtime", "session", "tmp"].map((name) => [name, join(isolation, name)]));
for (const dir of Object.values(dirs)) mkdirSync(dir, { recursive: true });

const summary = {
  platform,
  binary,
  started_at: new Date().toISOString(),
  isolation: dirs,
  notes: [
    "New browser tab uses the registered shortcut tab.newBrowser (Mod+Shift+B); the command palette has no browser-tab command.",
    "The downloads shelf has no shortcut or palette command; it renders only when a download exists, so the step navigates to an attachment URL.",
    "Settings > Browser is hidden until General > Advanced features is on (fresh install default), so the run enables it first.",
  ],
  steps: [],
};
const writeSummary = () => writeFileSync(join(out, "summary.json"), JSON.stringify(summary, null, 2));

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

function run(command, argv, options = {}) {
  const result = spawnSync(command, argv, { encoding: "utf8", timeout: options.timeout ?? 30000, ...options });
  const ok = result.status === 0 && !result.error;
  return { ok, status: result.status, stdout: (result.stdout ?? "").trim(), stderr: (result.stderr ?? "").trim() || String(result.error ?? "") };
}

// ---------------------------------------------------------------- platform: Linux (X11)
const linux = {
  windowId: null,
  findWindow(pid) {
    for (const query of [["search", "--onlyvisible", "--pid", String(pid)], ["search", "--onlyvisible", "--name", "Ferryx"]]) {
      const result = run("xdotool", query);
      const ids = result.stdout.split(/\s+/).filter(Boolean);
      if (ids.length) return ids[ids.length - 1];
    }
    return null;
  },
  focus() {
    if (!this.windowId) return { ok: false, stderr: "no window" };
    return run("xdotool", ["windowactivate", "--sync", this.windowId]);
  },
  keyName(key) {
    return { Tab: "Tab", Enter: "Return", Escape: "Escape", Space: "space", ",": "comma", "=": "equal" }[key] ?? key;
  },
  key({ key, mods = [] }) {
    const chord = [...mods.map((mod) => (mod === "mod" ? "ctrl" : mod)), this.keyName(key)].join("+");
    return run("xdotool", ["key", "--clearmodifiers", chord]);
  },
  type(text) {
    return run("xdotool", ["type", "--delay", "40", text]);
  },
  resize(width, height) {
    return run("xdotool", ["windowsize", "--sync", this.windowId, String(width), String(height)]);
  },
  screenshot(path) {
    const result = run("import", ["-window", "root", path]);
    return result.ok ? result : run("sh", ["-c", `xwd -root -silent | convert xwd:- png:'${path}'`]);
  },
};

// ---------------------------------------------------------------- platform: macOS
const macKeyCodes = { Tab: 48, Enter: 36, Escape: 53, Space: 49 };
const mac = {
  pid: null,
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
    // Fallback: cliclick posts CGEvents directly.
    const cliMods = mods.map((mod) => ({ mod: "cmd", shift: "shift", alt: "alt", ctrl: "ctrl" })[mod]);
    const named = { Tab: "tab", Enter: "return", Escape: "esc", Space: "space" }[key];
    const sequence = [
      ...(cliMods.length ? [`kd:${cliMods.join(",")}`] : []),
      named ? `kp:${named}` : `t:${key}`,
      ...(cliMods.length ? [`ku:${cliMods.join(",")}`] : []),
    ];
    const fallback = run("cliclick", sequence);
    return { ...fallback, stderr: `osascript: ${result.stderr}; cliclick: ${fallback.stderr}` };
  },
  type(text) {
    const result = this.osa(`tell application "System Events" to keystroke "${text}"`);
    return result.ok ? result : run("cliclick", [`t:${text}`]);
  },
  resize(width, height) {
    return this.osa(
      `tell application "System Events" to tell (first process whose unix id is ${this.pid}) to set size of window 1 to {${width}, ${height}}`,
    );
  },
  screenshot(path) {
    return run("screencapture", ["-x", "-t", "png", path]);
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
  resize(width, height) {
    return this.ps("resize", ["-Hwnd", this.hwnd, "-Width", String(width), "-Height", String(height)]);
  },
  screenshot(path) {
    return this.ps("shot", ["-Path", path]);
  },
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
$handle = [IntPtr]::new([long]$Hwnd)
switch ($Action) {
  "find" { $h = [FxInput]::FindTopWindow([uint32]$ProcessId); if ($h -eq [IntPtr]::Zero) { foreach ($p in Get-Process -Name ferryx -ErrorAction SilentlyContinue) { if ($p.MainWindowHandle -ne 0) { $h = $p.MainWindowHandle } } }; Write-Output ([long]$h) }
  "focus" { if (-not [FxInput]::Focus($handle)) { Write-Output "foreground not confirmed"; exit 3 } }
  "keys" { [void][FxInput]::Focus($handle); $n = [FxInput]::Chord([uint16[]]($Codes -split "," | ForEach-Object { [uint16]$_ })); if ($n -eq 0) { exit 4 } }
  "type" { [void][FxInput]::Focus($handle); $n = [FxInput]::Type($Text); if ($n -eq 0) { exit 4 } }
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

// ---------------------------------------------------------------- launch
const env = {
  ...process.env,
  FERRYX_DATA_DIR: dirs.data,
  FERRYX_RUNTIME_DIR: dirs.runtime,
  FERRYX_SESSION_DIR: dirs.session,
  RUST_LOG: process.env.RUST_LOG ?? "info",
};
if (platform === "linux") {
  Object.assign(env, { GDK_BACKEND: "x11", LIBGL_ALWAYS_SOFTWARE: "1", WEBKIT_DISABLE_DMABUF_RENDERER: "1", WEBKIT_DISABLE_COMPOSITING_MODE: "1" });
}
const appLog = openSync(join(out, "app.log"), "w");
let app = null;
if (!existsSync(binary)) {
  summary.launch = { ok: false, error: `binary not found: ${binary}` };
} else {
  app = spawn(binary, [], { env, stdio: ["ignore", appLog, appLog], cwd: dirs.tmp, detached: platform !== "win32" });
  app.on("exit", (code, signal) => {
    summary.app_exit = { code, signal, at: new Date().toISOString() };
    writeSummary();
  });
  summary.launch = { ok: true, pid: app.pid };
  mac.pid = win.pid = app.pid;
}
writeSummary();

let shotIndex = 0;
function screenshot(name) {
  shotIndex += 1;
  const file = `${String(shotIndex).padStart(2, "0")}-${name}.png`;
  const path = join(shots, file);
  const result = host.screenshot(path);
  const exists = existsSync(path);
  return {
    file: exists ? `screenshots/${file}` : null,
    ok: result.ok && exists,
    sha256: exists ? createHash("sha256").update(readFileSync(path)).digest("hex") : null,
    error: result.ok ? undefined : result.stderr,
  };
}

async function step(name, description, actions) {
  const record = { name, description, started_at: new Date().toISOString(), actions: [], outcome: "ok" };
  if (summary.app_exit) {
    record.outcome = "skipped_app_exited";
  } else {
    try {
      for (const [label, action] of actions) {
        if (typeof action === "number") {
          await sleep(action);
          continue;
        }
        const result = action();
        record.actions.push({ label, ok: result.ok, ...(result.ok ? {} : { status: result.status, error: result.stderr }) });
        if (!result.ok) record.outcome = "input_failed";
        await sleep(350);
      }
    } catch (error) {
      record.outcome = "harness_error";
      record.error = String(error?.stack ?? error);
    }
  }
  record.screenshot = screenshot(name);
  if (!record.screenshot.ok && record.outcome === "ok") record.outcome = "screenshot_failed";
  summary.steps.push(record);
  writeSummary();
  console.log(`[ui-audit] ${name}: ${record.outcome}`);
}

const focus = ["focus window", () => host.focus()];
const key = (label, spec) => [label, () => host.key(spec)];
const repeat = (count, label, spec) => Array.from({ length: count }, (_, index) => key(`${label} ${index + 1}/${count}`, spec));

// ---------------------------------------------------------------- run
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
  // Let the UI finish its first render and the daemon come up.
  await sleep(15000);
}

await step("launch", "App window after startup", [focus]);
await step("dismiss-onboarding", "Escape dismisses the first-run welcome wizard (Remind me later)", [focus, key("Escape", { key: "Escape" }), 1500]);
await step("new-browser-tab", "Mod+Shift+B (tab.newBrowser) opens a browser tab", [focus, key("Mod+Shift+B", { key: "b", mods: ["mod", "shift"] }), 5000]);
await step("address-bar-example", "Mod+L, type https://example.com, Enter", [
  focus,
  key("Mod+L", { key: "l", mods: ["mod"] }),
  ["type URL", () => host.type("https://example.com")],
  key("Enter", { key: "Enter" }),
  8000,
]);
await step("settings-open", "Mod+, opens Settings (General)", [focus, key("Mod+,", { key: ",", mods: ["mod"] }), 2000]);
await step("settings-enable-advanced", "Tab x11 from Back to app reaches General > Advanced features; Space toggles it on", [
  focus,
  ...repeat(11, "Tab", { key: "Tab" }),
  key("Space", { key: "Space" }),
  1500,
]);
await step("settings-browser", "Close and reopen Settings, Tab x6 to the Browser section, Enter", [
  focus,
  key("Escape", { key: "Escape" }),
  1000,
  key("Mod+,", { key: ",", mods: ["mod"] }),
  1500,
  ...repeat(6, "Tab", { key: "Tab" }),
  key("Enter", { key: "Enter" }),
  2500,
]);
await step("settings-closed", "Escape closes Settings", [focus, key("Escape", { key: "Escape" }), 1500]);
await step("narrow-900", "Resize the window to 900x800", [focus, ["resize 900x800", () => host.resize(900, 800)], 2500]);
await step("downloads-shelf", "Navigate to an attachment URL so the downloads shelf appears", [
  focus,
  key("Mod+L", { key: "l", mods: ["mod"] }),
  ["type URL", () => host.type("https://httpbin.org/response-headers?Content-Disposition=attachment%3B%20filename%3Dferryx-qa.txt")],
  key("Enter", { key: "Enter" }),
  8000,
]);
await step("back-to-example", "Mod+L, https://example.com, Enter (page for find/zoom)", [
  focus,
  key("Mod+L", { key: "l", mods: ["mod"] }),
  ["type URL", () => host.type("https://example.com")],
  key("Enter", { key: "Enter" }),
  6000,
]);
await step("find-bar", "Mod+F opens the find bar; type 'Example'", [
  focus,
  key("Mod+F", { key: "f", mods: ["mod"] }),
  1000,
  ["type query", () => host.type("Example")],
  1500,
]);
await step("zoom-in", "Escape closes find, Mod+= twice zooms in", [
  focus,
  key("Escape", { key: "Escape" }),
  key("Mod+= 1", { key: "=", mods: ["mod"] }),
  key("Mod+= 2", { key: "=", mods: ["mod"] }),
  2000,
]);

// ---------------------------------------------------------------- teardown
summary.finished_at = new Date().toISOString();
summary.totals = summary.steps.reduce((counts, entry) => ({ ...counts, [entry.outcome]: (counts[entry.outcome] ?? 0) + 1 }), {});
writeSummary();
if (app && !summary.app_exit) {
  if (platform === "win32") run("taskkill", ["/PID", String(app.pid), "/T", "/F"]);
  else {
    try {
      process.kill(-app.pid, "SIGTERM");
    } catch {}
  }
}
await sleep(1500);
console.log(JSON.stringify(summary.totals));
process.exit(0);
