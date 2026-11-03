#!/usr/bin/env bun
// Native Wayland check for the Ferryx browser, run on a GitHub-hosted Ubuntu runner inside an
// already-started headless wlroots compositor (cage; WAYLAND_DISPLAY and XDG_RUNTIME_DIR come
// from the workflow). No input injection: tabs are opened and read through `ferryx browser`, and
// the screen is captured with grim.
//
// Phases, each a fresh app launch with its own FERRYX_* directories:
//   wayland-system            native Wayland, no engine: the system webview must show the page.
//   wayland-engine-installed  native Wayland with FERRYX_CEF_ENGINE_DIR: Chromium cannot hold a
//                             tab without an X11 window, so the tab must fall back to the system
//                             engine (never blank, never a crash) and the pane explains why.
//   xwayland-engine           GDK_BACKEND=x11 on the compositor's Xwayland (only with
//                             --engine-dir and DISPLAY_XWAYLAND): the tab should be Chromium.
//                             Recorded, never a failure.
// Writes summary.json, screenshots/<phase>.png and app-<phase>.log to --out and prints a one-line
// JSON verdict per phase. Exits 0 once it has started.
//
// Usage: bun scripts/browser-qa/gha-wayland-check.mjs --binary <path> --out <dir> [--engine-dir <dir>]
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, openSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { inflateSync } from "node:zlib";

const argv = process.argv.slice(2);
const option = (name) => {
  const index = argv.indexOf(`--${name}`);
  return index >= 0 ? argv[index + 1] : undefined;
};
const binary = resolve(option("binary") ?? "");
const out = resolve(option("out") ?? "wayland-check");
const engineDir = option("engine-dir") ? resolve(option("engine-dir")) : null;
const shots = join(out, "screenshots");
mkdirSync(shots, { recursive: true });

const summary = {
  binary,
  engine_dir: engineDir,
  engine_dir_has_exe: engineDir ? existsSync(join(engineDir, "ferryx_engine")) : null,
  commit: process.env.GITHUB_SHA ?? null,
  wayland_display: process.env.WAYLAND_DISPLAY ?? null,
  xdg_runtime_dir: process.env.XDG_RUNTIME_DIR ?? null,
  display_xwayland: process.env.DISPLAY_XWAYLAND ?? null,
  started_at: new Date().toISOString(),
  phases: [],
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
// Solid fixture green, so a screenshot tells a painted page from a blank pane.
const FIXTURE = `<!doctype html><html><head><title>fixture-ready</title><style>
html,body{margin:0;height:100%;background:#00a86b}
p{font:28px sans-serif;color:#fff;margin:40px}
</style></head><body><p>Ferryx Wayland fixture</p></body></html>`;
const server = Bun.serve({
  port: 0,
  hostname: "127.0.0.1",
  fetch(request) {
    if (new URL(request.url).pathname === "/fixture.html") return new Response(FIXTURE, { headers: { "Content-Type": "text/html" } });
    return new Response("not found", { status: 404 });
  },
});
const fixtureUrl = `http://127.0.0.1:${server.port}/fixture.html`;
summary.fixture_url = fixtureUrl;

// ---------------------------------------------------------------- PNG pixels
/** Minimal 8-bit non-interlaced PNG decoder (RGB/RGBA/gray), same as gha-ui-audit.mjs. */
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
const FIXTURE_GREEN = [0, 168, 107];
// Samples are every 4th pixel both ways; a painted pane of even 400x300 gives ~7500.
const FIXTURE_VISIBLE_MIN = 800;
function fixturePixels(path) {
  if (!path || !existsSync(path)) return { error: "no screenshot" };
  try {
    const image = decodePng(path);
    let green = 0;
    for (let y = 0; y < image.height; y += 4) {
      for (let x = 0; x < image.width; x += 4) {
        if (image.rgb(x, y).every((value, index) => Math.abs(value - FIXTURE_GREEN[index]) <= 30)) green += 1;
      }
    }
    return { green, width: image.width, height: image.height, visible: green >= FIXTURE_VISIBLE_MIN };
  } catch (error) {
    return { error: String(error?.message ?? error) };
  }
}

const tesseract = ["tesseract", "/usr/bin/tesseract"].find((candidate) => run(candidate, ["--version"]).ok) ?? null;
summary.ocr = tesseract ?? "unavailable";

const enginePids = () => run("pgrep", ["-f", "ferryx_engine"]).stdout.split(/\s+/).filter(Boolean).map(Number);
function killEngines() {
  for (const pid of enginePids()) {
    try {
      process.kill(pid, "SIGKILL");
    } catch {}
  }
}

// ---------------------------------------------------------------- one phase
async function phase(name, { extraEnv = {}, x11 = false, expectEngine, required = true }) {
  const record = { name, required, started_at: new Date().toISOString(), checks: {} };
  summary.phases.push(record);
  const root = join(process.env.RUNNER_TEMP ?? out, `ferryx-wayland-${name}`);
  rmSync(root, { recursive: true, force: true });
  const dirs = Object.fromEntries(["data", "runtime", "session", "tmp"].map((dir) => [dir, join(root, dir)]));
  for (const dir of Object.values(dirs)) mkdirSync(dir, { recursive: true });
  // `ferryx browser ...` carries only the transport token; with "require pairing" off in this
  // isolated profile, list/open answer instead of BROWSER_CONTROLLER_PAIRING_REQUIRED.
  mkdirSync(join(dirs.data, "browser"), { recursive: true });
  writeFileSync(join(dirs.data, "browser", "agent-authority.json"), JSON.stringify({ settings: { requirePairing: false } }));

  const env = {
    ...process.env,
    FERRYX_DATA_DIR: dirs.data,
    FERRYX_RUNTIME_DIR: dirs.runtime,
    FERRYX_SESSION_DIR: dirs.session,
    RUST_LOG: process.env.RUST_LOG ?? "info",
    GDK_BACKEND: "wayland",
    WEBKIT_DISABLE_DMABUF_RENDERER: "1",
    LIBGL_ALWAYS_SOFTWARE: "1",
    ...extraEnv,
  };
  // Native Wayland phases must not find an X server, or GTK/Chromium could quietly use it.
  if (!x11) delete env.DISPLAY;
  record.env = Object.fromEntries(["GDK_BACKEND", "DISPLAY", "WAYLAND_DISPLAY", "FERRYX_CEF_ENGINE_DIR"].map((key) => [key, env[key] ?? null]));

  const log = openSync(join(out, `app-${name}.log`), "w");
  const app = spawn(binary, [], { env, stdio: ["ignore", log, log], cwd: dirs.tmp, detached: true });
  let exited = null;
  app.on("exit", (code, signal) => {
    exited = { code, signal, at: new Date().toISOString() };
  });
  const cli = (...args) => {
    const result = run(binary, ["browser", ...args], { env, timeout: 30000 });
    return { ok: result.ok, status: result.status, stdout: result.stdout.slice(0, 4000), stderr: result.stderr };
  };
  // Blocking spawnSync would stall the fixture server on this event loop, so `open` (which may
  // wait for the page) runs asynchronously.
  const cliAsync = async (...args) => {
    const child = Bun.spawn([binary, "browser", ...args], { env, stdout: "pipe", stderr: "pipe" });
    const timer = setTimeout(() => child.kill(), 30000);
    const [stdout, stderr, status] = await Promise.all([new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited]);
    clearTimeout(timer);
    return { ok: status === 0, status, stdout: stdout.trim().slice(0, 4000), stderr: stderr.trim().slice(0, 2000) };
  };
  const list = () => {
    const result = cli("list");
    try {
      return { list: JSON.parse(result.stdout), raw: result };
    } catch {
      return { list: null, raw: result };
    }
  };

  try {
    const started = Date.now();
    let listed = list();
    while (!Array.isArray(listed.list) && Date.now() - started < 120000 && !exited) {
      await sleep(2000);
      listed = list();
    }
    record.cli_ready_ms = Array.isArray(listed.list) ? Date.now() - started : null;
    record.checks.cli_answers = Array.isArray(listed.list);
    if (!record.checks.cli_answers) record.cli_last = listed.raw;
    if (record.checks.cli_answers) {
      // Let the first render settle so the opened tab is shown, not queued behind startup.
      await sleep(5000);
      record.open = await cliAsync("open", "--url", fixtureUrl);
      await sleep(10000);
      // A Chromium tab that cannot attach falls back to the system engine; give a fallback a
      // few more seconds to reload the page before reading the result.
      let after = list();
      for (let attempt = 0; attempt < 5 && !(after.list ?? []).some((entry) => entry.title === "fixture-ready"); attempt += 1) {
        await sleep(3000);
        after = list();
      }
      record.browser_list = after.raw.stdout;
      const tab = (after.list ?? []).find((entry) => (entry.url ?? "").includes("/fixture.html")) ?? (after.list ?? []).at(-1) ?? null;
      record.tab = tab ? { browserId: tab.browserId, engine: tab.engine ?? null, title: tab.title ?? null, url: tab.url ?? null } : null;
    }
    record.checks.tab_listed = Boolean(record.tab);
    record.checks.title_fixture_ready = record.tab?.title === "fixture-ready";

    const shot = join(shots, `${name}.png`);
    const grim = run("grim", [shot]);
    record.screenshot = grim.ok && existsSync(shot) ? `screenshots/${name}.png` : null;
    if (!grim.ok) record.screenshot_error = grim.stderr;
    record.pixels = fixturePixels(record.screenshot ? shot : null);
    record.checks.fixture_visible = record.pixels.visible === true;

    if (tesseract && record.screenshot) {
      const text = run(tesseract, [shot, "-", "--psm", "11"], { timeout: 120000 }).stdout;
      writeFileSync(join(out, `ocr-${name}.txt`), text);
      record.ocr = { wayland: /wayland/i.test(text), gdk_backend: /GDK_BACKEND/.test(text), system_browser_engine: /system browser engine/i.test(text.replace(/\s+/g, " ")) };
    }

    record.engine_pids = enginePids();
    record.checks.app_alive = !exited;
    if (exited) record.app_exit = exited;
    if (expectEngine) record.checks[`engine_${expectEngine}`] = record.tab?.engine === expectEngine;
    if (expectEngine === "system" && extraEnv.FERRYX_CEF_ENGINE_DIR) record.checks.no_engine_process = record.engine_pids.length === 0;
  } catch (error) {
    record.error = String(error?.stack ?? error);
  }

  const c = record.checks;
  const basePass = c.app_alive === true && c.tab_listed === true && (c.fixture_visible === true || c.title_fixture_ready === true);
  // Either signal proves the page loaded; scripts/browser-qa/check-summary.mjs gates on
  // fixture_shown, and an optional phase gates only on its engine check.
  c.fixture_shown = c.fixture_visible === true || c.title_fixture_ready === true;
  record.informational = ["fixture_visible", "title_fixture_ready", ...(required ? [] : Object.keys(c).filter((name) => !name.startsWith("engine_")))];
  record.pass = Boolean(basePass && (!expectEngine || c[`engine_${expectEngine}`] === true) && c.no_engine_process !== false);
  record.finished_at = new Date().toISOString();

  // Stop the whole process group (app, daemon children) and any engine before the next launch.
  if (!exited) {
    try {
      process.kill(-app.pid, "SIGTERM");
    } catch {}
    await sleep(3000);
    try {
      process.kill(-app.pid, "SIGKILL");
    } catch {}
  }
  killEngines();
  await sleep(1000);
  writeSummary();
  console.log(JSON.stringify({ phase: name, pass: record.pass, required, engine: record.tab?.engine ?? null, title: record.tab?.title ?? null, green: record.pixels?.green ?? null, checks: record.checks }));
  return record;
}

// ---------------------------------------------------------------- phases
if (!existsSync(binary)) {
  summary.error = `binary not found: ${binary}`;
} else if (!process.env.WAYLAND_DISPLAY) {
  summary.error = "WAYLAND_DISPLAY not set: start the compositor first";
} else {
  await phase("wayland-system", {});
  if (engineDir) {
    await phase("wayland-engine-installed", { extraEnv: { FERRYX_CEF_ENGINE_DIR: engineDir }, expectEngine: "system" });
  } else {
    summary.phases.push({ name: "wayland-engine-installed", skipped: "no --engine-dir" });
  }
  if (engineDir && process.env.DISPLAY_XWAYLAND) {
    await phase("xwayland-engine", {
      extraEnv: { FERRYX_CEF_ENGINE_DIR: engineDir, GDK_BACKEND: "x11", DISPLAY: process.env.DISPLAY_XWAYLAND },
      x11: true,
      expectEngine: "chromium",
      required: false,
    });
  } else {
    summary.phases.push({ name: "xwayland-engine", skipped: engineDir ? "DISPLAY_XWAYLAND not set" : "no --engine-dir" });
  }
}

summary.finished_at = new Date().toISOString();
summary.pass = summary.phases.filter((entry) => entry.required && !entry.skipped).every((entry) => entry.pass);
writeSummary();
server.stop(true);
console.log(JSON.stringify({ pass: summary.pass, error: summary.error ?? null, phases: summary.phases.map((entry) => ({ name: entry.name, pass: entry.pass ?? null, skipped: entry.skipped ?? null })) }));
process.exit(0);
