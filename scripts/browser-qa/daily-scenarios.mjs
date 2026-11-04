// Daily-browser scenarios for gha-ui-audit.mjs (F4 gaps G1 G2 G3 G6 G8 G9 G11 G13), driven with
// real OS input like the rest of the audit. gha-ui-audit.mjs builds a context `ctx` from its own
// helpers and calls these functions at fixed points of the full flow:
//   dailyWebviewBefore(ctx)  system-webview tabs, before the Chromium engine is installed
//   dailyRestart(ctx)        quit like a user (Mod+Q / Alt+F4), relaunch on the same dirs
//   dailyWebviewAfter(ctx)   what must have survived the restart, then private mode
//   dailyChromium(ctx)       Chromium-engine tabs: upload/drag-in, PDF, named profile separation
// Every check starts false and turns true only from an observed effect: `ferryx browser list`
// fields, the app's data files (history.sqlite, bookmarks.json, vault.json, browser-site-zoom.json),
// the OS keystore, D-Bus traffic, a written file, or OCR of the screen. Results go to
// summary.daily.<gap>.checks; check-summary.mjs fails the job on a false or missing gap.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

/** The gaps each platform must report (G6 notifications and G9 print run on Linux and Windows). */
export function expectedDailyGaps(platform) {
  const all = ["g1", "g2", "g3", "g8", "g11", "g13"];
  return platform === "darwin" ? all : [...all, "g6", "g9"];
}

// ---------------------------------------------------------------- fixtures
/** A valid 2-page PDF (Helvetica 48pt), xref offsets computed. */
export function makePdf() {
  const page = (text) => `BT /F1 48 Tf 60 600 Td (${text}) Tj ET`;
  const streams = [page("FERRYXPDF ONE"), page("FERRYXPDF TWO")];
  const objects = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 6 0 R >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 7 0 R >>",
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ...streams.map((stream) => `<< /Length ${stream.length} >>\nstream\n${stream}\nendstream`),
  ];
  let body = "%PDF-1.4\n";
  const offsets = [];
  objects.forEach((object, index) => {
    offsets.push(body.length);
    body += `${index + 1} 0 obj\n${object}\nendobj\n`;
  });
  const xref = body.length;
  body += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  for (const offset of offsets) body += `${String(offset).padStart(10, "0")} 00000 n \n`;
  body += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(body, "latin1");
}

const page = (title, heading, extra = "") => `<!doctype html><html><head><title>${title}</title><style>
body{margin:0;background:#fff;color:#000;font:bold 44px sans-serif}h1{font-size:64px;margin:40px}p{margin:20px 40px}
</style></head><body><h1>${heading}</h1>${extra}</body></html>`;

/**
 * Fixture routes added to the audit's fixture server(s). `siteB` is the other server's origin
 * (a different port: another site for per-site zoom and a cross-origin iframe).
 */
export function dailyFixtures({ siteB }) {
  const login = `<!doctype html><html><head><title>qa-login</title><style>
body{margin:0;background:#fff;font:bold 36px sans-serif}form{margin:40px}input{display:block;font:36px sans-serif;margin:16px 0;width:520px}
input::placeholder{color:#000;opacity:1}
button{font:bold 40px sans-serif;margin-top:16px}</style></head><body>
<p id=s>LOGINPAGE</p>
<form method="post" action="/login.html"><input id=u name=username autocomplete=username placeholder=USERNAME>
<input id=p name=password type=password autocomplete=current-password placeholder=PASSWORD><button id=go type=submit>LOGIN</button></form>
<script>addEventListener('message',(e)=>{if(e.data&&e.data.type==='report')e.source.postMessage({type:'state',u:u.value,p:p.value},'*')})</script>
</body></html>`;
  const frame = `<!doctype html><html><head><title>frame:wait</title></head><body style="font:bold 36px sans-serif">
<p>FRAMEHOST</p><iframe id=f src="${siteB}/login.html" style="width:900px;height:420px;border:4px solid #000"></iframe>
<script>addEventListener('message',(e)=>{if(e.data&&e.data.type==='state')document.title='frame:'+e.data.u+'|'+e.data.p});
setInterval(()=>{try{f.contentWindow.postMessage({type:'report'},'*')}catch(_){}} ,500)</script></body></html>`;
  const notify = `<!doctype html><title>perm:idle</title><style>html,body{margin:0;height:100%}
button{position:fixed;inset:0;width:100%;height:100%;font:bold 48px sans-serif;background:#fff;color:#000;border:0}</style>
<button id=n type=button>NOTIFYME</button>
<script>n.addEventListener('click',()=>{document.title='perm:asking';Notification.requestPermission().then((p)=>{document.title='perm:'+p;
if(p==='granted'){new Notification('ferryx-qa',{body:'ferryx qa body'});document.title='perm:granted:shown'}})})</script>`;
  const upload = `<!doctype html><title>upload-idle</title><style>html,body{margin:0;height:100%;font:bold 44px sans-serif}
#pick{position:fixed;left:0;right:0;top:0;height:45%;display:flex;align-items:center;justify-content:center;background:#ffd400}
#f{position:absolute;inset:0;opacity:0;width:100%;height:100%;cursor:pointer}
#drop{position:fixed;left:0;right:0;bottom:0;height:50%;display:flex;align-items:center;justify-content:center;background:#9fe2bf}</style>
<label id=pick>UPLOADPICK<input id=f type=file></label><div id=drop>DROPZONE</div>
<script>f.addEventListener('change',()=>{const x=f.files[0];document.title=x?'picked:'+x.name+':'+x.size:'picked:none'});
for(const t of ['dragenter','dragover'])drop.addEventListener(t,(e)=>{e.preventDefault();drop.textContent='DROPHOVER'});
drop.addEventListener('drop',(e)=>{e.preventDefault();const x=e.dataTransfer.files[0];document.title=x?'dropped:'+x.name+':'+x.size:'dropped:none';drop.textContent='DROPPED'})</script>`;
  const cookies = (request) => {
    const header = request.headers.get("cookie") ?? "";
    return page(`cookies:${header || "none"}`, "COOKIES", `<p>COOKIES:${header || "none"}</p>`);
  };
  return new Map([
    ["/qa/a.html", { body: page("QA Alpha Page", "ALPHAPAGE"), type: "text/html" }],
    ["/qa/b.html", { body: page("QA Bravo Page", "BRAVOPAGE"), type: "text/html" }],
    ["/qa/c.html", { body: page("QA Charlie Page", "CHARLIEPAGE"), type: "text/html" }],
    ["/qa/second.html", { body: page("QA Second Tab", "SECONDTAB"), type: "text/html" }],
    ["/login.html", { body: login, type: "text/html" }],
    ["/login-frame.html", { body: frame, type: "text/html" }],
    ["/secret.html", { body: page("qa-secret", "SECRETPAGE"), type: "text/html", headers: { "Set-Cookie": "qa_secret=1; Path=/" } }],
    ["/set-work.html", { body: page("qa-work-set", "WORKSET"), type: "text/html", headers: { "Set-Cookie": "qa_work=1; Path=/" } }],
    ["/cookies.html", { dynamic: cookies, type: "text/html", headers: { "Cache-Control": "no-store" } }],
    ["/notify.html", { body: notify, type: "text/html" }],
    ["/upload.html", { body: upload, type: "text/html" }],
    ["/print.html", { body: page("qa-print", "PRINTPAGE"), type: "text/html" }],
    ["/doc.pdf", { body: makePdf(), type: "application/pdf" }],
  ]);
}

// ---------------------------------------------------------------- shared helpers
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

/** Starts summary.daily[gap] with every named check false. */
function gap(ctx, name, checks) {
  ctx.summary.daily ??= {};
  const record = ctx.summary.daily[name] ?? { checks: {} };
  for (const check of checks) record.checks[check] ??= false;
  ctx.summary.daily[name] = record;
  return record;
}

/** Polls `probe` until it returns a truthy value, bounded; returns the last value. */
async function until(probe, timeout = 15000, every = 1000) {
  const deadline = Date.now() + timeout;
  let value = await probe();
  while (!value && Date.now() < deadline) {
    await sleep(every);
    value = await probe();
  }
  return value;
}

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return null;
  }
}

async function historyRows(ctx) {
  const path = join(ctx.dirs.data, "browser", "history.sqlite");
  if (!existsSync(path)) return [];
  const { Database } = await import("bun:sqlite");
  const db = new Database(path, { readonly: true });
  try {
    return db.query("SELECT url, title, last_visited_at AS at FROM urls").all();
  } catch {
    return [];
  } finally {
    db.close();
  }
}

const bookmarksFile = (ctx) => readJson(join(ctx.dirs.data, "browser", "bookmarks.json")) ?? { folders: [], bookmarks: [] };

/** All OCR words on screen now. */
const screenWords = (ctx) => ctx.ocr(ctx.probeShot()) ?? [];
const wordSeen = (words, test) => words.some((word) => test(ctx_norm(word.text)));
function ctx_norm(text) {
  return text.toLowerCase().replace(/[^a-z0-9:%./|_-]/g, "");
}
const has = (words, needle) => wordSeen(words, (text) => text.includes(needle.toLowerCase()));

/** Waits until OCR reads a word containing one of `needles`; returns the needle seen or null. */
async function waitWords(ctx, needles, timeout = 15000) {
  const seen = await until(() => {
    const words = screenWords(ctx);
    return needles.find((needle) => has(words, needle)) ?? null;
  }, timeout);
  return seen ?? null;
}

/** The listed tab, refreshed. */
function tab(ctx, browserId) {
  return ctx.sessions().list.find((entry) => entry.browserId === browserId) ?? null;
}

/**
 * The tab that actually shows a URL whose path ends in `suffix`: the visible one first, else any.
 * Reading a remembered id went wrong whenever a visit landed in another tab (linux run
 * 41801115761: login-frame loaded in tab A, so tab B's title "perm:idle" was read as the frame).
 */
function tabAt(ctx, suffix) {
  const list = ctx.sessions().list.filter((entry) => (entry.url ?? "").split(/[?#]/)[0].endsWith(suffix));
  return list.find((entry) => entry.visible) ?? list[0] ?? null;
}

/**
 * The value a page script returned through `ferryx browser eval`. Its stdout is a JSON string
 * around the script's own JSON (run 41801115761: "\"[\\\"qauser\\\",11]\""), so a substring
 * test for '"qauser"' never matched; decode while the value is still a string.
 */
function parseEval(stdout) {
  let value = (stdout ?? "").trim();
  for (let index = 0; index < 3 && typeof value === "string"; index += 1) {
    try {
      value = JSON.parse(value);
    } catch {
      break;
    }
  }
  return value;
}

/** The login fixture's two fields in tab `browserId` as [username, password], or null. */
async function loginFields(ctx, browserId) {
  if (!browserId) return null;
  const result = await ctx.cliAsync("eval", "--browser-id", browserId, "--script", "JSON.stringify([document.getElementById('u').value, document.getElementById('p').value])");
  const value = result.ok ? parseEval(result.stdout) : null;
  return Array.isArray(value) ? value : null;
}

/**
 * Closes the library panel (History / Bookmarks / Downloads) when it is open: its close X is the
 * last control on its tab row; confirmed by reading the screen again, retried once. Windows run
 * 41801115761 left the panel over the page from the history step to the PDF step (screenshots
 * 13-17): the LOGINPAGE click found nothing, the typed login went nowhere (typed ["",0]) and the
 * zoom and PDF visits were typed while the panel had the focus. This was the first divergence.
 */
async function closeLibrary(ctx) {
  const record = { open: false, clicks: [] };
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const path = ctx.probeShot();
    const tabs = (ctx.ocr(path) ?? []).find((word) => /^downloads$/i.test(word.text));
    if (!tabs) return { ...record, ok: true };
    record.open = true;
    if (attempt === 2) break;
    const image = ctx.decodePng(path);
    const y = tabs.top + Math.round(tabs.height / 2);
    const runs = rowRuns(image, y, tabs.left + tabs.width + 20, image.width);
    // Past the window's right edge the screen differs from the row in one wide run.
    while (runs.length && runs.at(-1)[1] - runs.at(-1)[0] > 60) runs.pop();
    const close = runs.filter(([a, b]) => b - a >= 4 && b - a <= 30).at(-1);
    if (!close) break;
    const at = [Math.round((close[0] + close[1]) / 2), y];
    record.clicks.push(at);
    ctx.host.click(...at);
    await sleep(1000);
  }
  return { ...record, ok: false };
}

/**
 * macOS: closes a native open panel (and its "Go to folder" sheet) left on screen. A leaked
 * Chromium open panel covered the app for the rest of macOS run 41801115761 (probes 234-295), so
 * Settings never opened for the profile scenario. Escape twice (the sheet, then the panel),
 * confirmed by OCR, retried once.
 */
async function closeFilePanel(ctx) {
  if (ctx.platform !== "darwin") return null;
  const panelOpen = () => {
    const words = screenWords(ctx);
    return has(words, "cancel") && words.some((word) => /^(new|folder|recents|applications|go|to:|open)$/i.test(word.text));
  };
  const record = { open: false, escapes: 0 };
  for (let attempt = 0; attempt < 2; attempt += 1) {
    if (!panelOpen()) return { ...record, ok: true };
    record.open = true;
    // Escape goes to the frontmost app, which owns the panel (the engine for a Chromium tab), so
    // the Ferryx window is not focused first.
    for (let press = 0; press < 2; press += 1) {
      ctx.host.key({ key: "Escape" });
      record.escapes += 1;
      await sleep(800);
    }
  }
  return { ...record, ok: !panelOpen() };
}

/** Closes the OS file-manager window a drag source opened, so it covers nothing later. */
function closeFileManager(ctx) {
  if (ctx.platform === "darwin") return ctx.run("osascript", ["-e", 'tell application "Finder" to close every window']).ok;
  if (ctx.platform === "win32") {
    // Explorer stayed open behind the app from G8 on (Windows run 41801115761 screenshots 20-22).
    return ctx.run("powershell.exe", ["-NoProfile", "-Command",
      "(New-Object -ComObject Shell.Application).Windows() | Where-Object { $_.LocationURL -like '*ui-audit*' } | ForEach-Object { $_.Quit() }"], { timeout: 20000 }).ok;
  }
  return null;
}

/**
 * Navigates through the real address bar (click, select all, type, Enter), confirms through
 * `ferryx browser list` that a visible tab reached the URL's path (retyped once otherwise), then
 * waits for OCR text. Windows run 41801115761: the /doc.pdf visit never landed and the PDF check
 * read example.html.
 */
async function visit(ctx, url, expect, timeout = 20000) {
  const path = new URL(url).pathname;
  const landed = () => ctx.sessions().list.some((entry) => entry.visible !== false && (entry.url ?? "").split(/[?#]/)[0].endsWith(path));
  for (let attempt = 0; attempt < 2; attempt += 1) {
    for (const [, action] of ctx.typeUrl(url)) await action();
    if (await until(landed, 8000)) break;
  }
  return expect ? waitWords(ctx, [].concat(expect), timeout) : null;
}

/** Focus the app chrome (not the page) and send an app shortcut to the active browser tab. */
async function shortcut(ctx, spec) {
  ctx.host.focus();
  await ctx.chromeFocusClick();
  await sleep(300);
  return ctx.host.key(spec);
}

/**
 * The address bar's own URL text: the topmost URL-looking word (the toolbar sits above the
 * page, the omnibox rows and the library list), or null.
 */
const URL_WORD = /^(about:blank|https?:\/\/\S+|\S*127\.0\.0\.1\S*)$/i;
function addressWord(words) {
  return words.filter((word) => URL_WORD.test(word.text)).sort((a, b) => a.top - b.top || a.left - b.left)[0] ?? null;
}

/**
 * Column runs on line `y` between x0 and x1 that differ from the line's background (the most
 * common colour near x0), probing 6px above and below so 14px glyphs are not missed.
 */
function rowRuns(image, y, x0, x1) {
  const clampY = (value) => Math.min(image.height - 1, Math.max(0, value));
  const from = Math.max(0, x0), to = Math.min(image.width, x1);
  const counts = new Map();
  for (let x = from; x < Math.min(to, from + 700); x += 2) {
    const key = image.rgb(x, clampY(y)).map((value) => value >> 3).join(",");
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  if (!counts.size) return [];
  const background = [...counts.entries()].sort((a, b) => b[1] - a[1])[0][0].split(",").map((value) => (Number(value) << 3) + 4);
  const differs = (x, yy) => image.rgb(x, yy).reduce((sum, value, index) => sum + Math.abs(value - background[index]), 0) > 60;
  const runs = [];
  let start = -1, last = -99;
  for (let x = from; x < to; x += 1) {
    let hit = false;
    for (let dy = -6; dy <= 6 && !hit; dy += 2) hit = differs(x, clampY(y + dy));
    if (!hit) continue;
    if (start < 0 || x - last > 5) {
      if (start >= 0) runs.push([start, last]);
      start = x;
    }
    last = x;
  }
  if (start >= 0) runs.push([start, last]);
  return runs;
}

/**
 * The browser toolbar's icon buttons right of the "NNN%" zoom readout, left to right:
 * [Zoom in, History, Focus, Sync, DevTools, Select, Print, PDF, Extensions, External].
 * OCR misses the small "100%" readout on every OS (run 41767162699: 0 icons, so "History menu
 * did not open" and the PDF button was never clicked), so the row is anchored on the address
 * bar's URL text, and the readout is the last run on that row clearly wider than an icon.
 * Measured on linux probe-115 and macOS step 13: icons 10-13px wide, readout 22-23px, star 10px.
 */
function toolbarIcons(ctx) {
  const path = ctx.probeShot();
  const words = ctx.ocr(path) ?? [];
  const url = addressWord(words);
  const label = words.filter((word) => /^\d{2,3}%$/.test(word.text)).sort((a, b) => a.top - b.top)[0] ?? null;
  const anchor = url ?? label;
  if (!anchor) return { icons: [], label: null, via: "neither the address bar nor the zoom readout was read" };
  const image = ctx.decodePng(path);
  const y = anchor.top + Math.round(anchor.height / 2);
  const runs = rowRuns(image, y, anchor.left + anchor.width + 4, image.width);
  // A trailing run wider than any control is the window edge or the screen outside the window.
  while (runs.length && runs.at(-1)[1] - runs.at(-1)[0] > 100) runs.pop();
  const width = ([a, b]) => b - a;
  let at = -1;
  let via = "pixel row";
  if (anchor === label) {
    // The scan started right of the readout itself: every run is already a toolbar icon.
    via = "ocr zoom readout (no url)";
  } else if (label && Math.abs(label.top + label.height / 2 - y) < 12) {
    at = runs.findIndex(([a, b]) => a <= label.left + label.width && b >= label.left);
    if (at >= 0) via = "ocr zoom readout";
  }
  if (at < 0 && anchor !== label) {
    const small = runs.map(width).filter((w) => w >= 6 && w <= 30).sort((a, b) => a - b);
    const median = small[Math.floor(small.length / 2)] ?? 11;
    at = runs.findLastIndex((run) => width(run) >= median * 1.6 && width(run) <= median * 4);
  }
  if (at < 0 && anchor !== label) return { icons: [], label: null, via: "zoom readout run not found", runs: runs.length, y };
  // Icons are 14px glyphs; a separator line is 1px, a wider run is not an icon.
  const icons = runs.slice(at + 1).filter((run) => width(run) >= 6 && width(run) <= 30).map(([a, b]) => ({ x: Math.round((a + b) / 2), y }));
  return { icons, label: { x: at >= 0 ? runs[at][0] : label.left, y }, via };
}

/** The toolbar Back button: the third control left of the address box (Back, Forward, Reload). */
function toolbarBack(ctx) {
  const path = ctx.probeShot();
  const url = addressWord(ctx.ocr(path) ?? []);
  if (!url) return null;
  const y = url.top + Math.round(url.height / 2);
  // The box border sits ~10px left of its text; stop before it.
  const icons = rowRuns(ctx.decodePng(path), y, Math.max(0, url.left - 160), url.left - 14).filter(([a, b]) => b - a >= 6 && b - a <= 30);
  if (icons.length < 3) return null;
  const [a, b] = icons.at(-3);
  return { x: Math.round((a + b) / 2), y };
}

/**
 * Ferryx's site-permission Allow button on the prompt line of the OCR word `wants`: the
 * rightmost filled light button (Allow is white with dark text; Block and Remember are not
 * filled). OCR read "Allow" as "atow" or not at all (run 41767162699 linux probes 109-142).
 */
function findAllowButton(ctx, wants) {
  const image = ctx.decodePng(ctx.probeShot());
  const y = wants.top + Math.round(wants.height / 2);
  const y0 = Math.max(0, y - 10), y1 = Math.min(image.height - 1, y + 10);
  const runs = [];
  let current = null;
  for (let x = wants.left + wants.width; x < image.width; x += 1) {
    let bright = 0;
    for (let yy = y0; yy <= y1; yy += 1) if (Math.min(...image.rgb(x, yy)) > 200) bright += 1;
    // A filled button column is mostly bright; a text column has a few bright pixels only.
    if (bright < 12) continue;
    if (current && x - current[1] <= 2) current[1] = x;
    else runs.push((current = [x, x]));
  }
  const button = runs.filter(([a, b]) => b - a >= 25 && b - a <= 140).at(-1);
  return button ? [Math.round((button[0] + button[1]) / 2), y] : null;
}

/**
 * Makes `browserId` the active browser tab. A tab-title click comes first (real input), but OCR
 * often misses inactive tab titles (macOS run 41767162699 read "QA Charlie Page" as "ea lie P",
 * so the click never happened and Mod+[ went to the other tab). The result is checked with
 * `ferryx browser list` (only the active tab's webview is visible), and `ferryx browser tab
 * switch` (which focuses the webview and selects its layout tab) is the fallback.
 */
async function activateTab(ctx, browserId, titleTest) {
  const record = { clicked: null, via: "none" };
  if (!browserId) return { ...record, ok: false };
  const shown = () => {
    const list = ctx.sessions().list;
    return list.find((entry) => entry.browserId === browserId)?.visible === true
      && list.filter((entry) => entry.browserId !== browserId).every((entry) => entry.visible === false);
  };
  if (titleTest) {
    ctx.host.focus();
    record.clicked = await clickWord(ctx, titleTest, { timeout: 5000 });
    if (record.clicked && (await until(shown, 3000, 500))) return { ...record, ok: true, via: "tab title click" };
  }
  const result = ctx.cli("tab", "switch", "--browser-id", browserId);
  record.cli = { ok: result.ok, stderr: result.stderr.slice(0, 300) };
  const visible = await until(shown, 5000, 500);
  await sleep(800);
  if (visible) return { ...record, ok: true, via: "cli tab switch" };
  return { ...record, ok: result.ok, via: result.ok ? "cli tab switch (visibility not confirmed)" : "none" };
}

/** Clicks the toolbar icon at `index` (0 = Zoom in); `verify` decides, else the next run is tried. */
async function clickToolbarIcon(ctx, index, verify, { spread = 1 } = {}) {
  const { icons, label } = toolbarIcons(ctx);
  const tried = [];
  for (const offset of [0, ...Array.from({ length: spread }, (_, i) => [i + 1, -(i + 1)]).flat()]) {
    const icon = icons[index + offset];
    if (!icon) continue;
    ctx.host.click(icon.x, icon.y);
    tried.push([icon.x, icon.y]);
    await sleep(1500);
    if (await verify()) return { ok: true, at: [icon.x, icon.y], tried, icons: icons.length, label };
    ctx.host.key({ key: "Escape" });
    await sleep(600);
  }
  return { ok: false, tried, icons: icons.length, label };
}

/** Clicks the OCR word matching `test` (topmost or as picked); returns its centre or null. */
async function clickWord(ctx, test, { timeout = 8000, pick = "first", dx = 0 } = {}) {
  const hit = await until(() => {
    const words = screenWords(ctx).filter((word) => test(word.text));
    if (!words.length) return null;
    const sorted = pick === "rightmost" ? words.sort((a, b) => b.left - a.left)
      : pick === "bottom" ? words.sort((a, b) => b.top - a.top) : words.sort((a, b) => a.top - b.top || a.left - b.left);
    return sorted[0];
  }, timeout, 800);
  if (!hit) return null;
  const at = [hit.left + Math.round(hit.width / 2) + dx, hit.top + Math.round(hit.height / 2)];
  ctx.host.click(...at);
  return at;
}

/** Runs one real-input check body inside an audit step and records it. */
async function run(ctx, name, description, body) {
  let outcome = null;
  await ctx.step(name, description, [[name, async () => {
    try {
      outcome = (await body()) ?? { ok: true };
    } catch (error) {
      outcome = { ok: false, stderr: String(error?.stack ?? error).slice(0, 1500) };
    }
    return outcome;
  }]]);
  return outcome;
}

/** The OS file manager / dialog helpers need a file with known bytes. */
function fixtureFile(ctx) {
  const path = join(ctx.out, "upload-fixture.txt");
  if (!existsSync(path)) writeFileSync(path, "ferryx qa upload fixture\n");
  return { path, name: "upload-fixture.txt", size: statSync(path).size };
}

/** Types `path` into the platform's native open-file dialog (which has just opened) and confirms. */
async function answerOpenDialog(ctx, path) {
  await sleep(2500);
  if (ctx.platform === "linux") {
    ctx.host.key({ key: "l", mods: ["ctrl"] });
    await sleep(500);
    ctx.host.type(path);
    await sleep(400);
    ctx.host.key({ key: "Enter" });
  } else if (ctx.platform === "win32") {
    // The common file dialog focuses its File name box; SendInput types into the foreground.
    ctx.win.ps("typeraw", ["-Text", path]);
    await sleep(400);
    ctx.win.ps("keysraw", ["-Codes", "13"]);
  } else {
    // The Chromium panel's "Go to folder" sheet kept only "/" of the typed path (macOS run
    // 41801115761 probe 234): it was still opening when typing started. Wait longer, and select
    // the field's text so the whole path replaces whatever the sheet prefilled.
    ctx.host.key({ key: "g", mods: ["mod", "shift"] });
    await sleep(2500);
    ctx.host.key({ key: "a", mods: ["mod"] });
    await sleep(300);
    ctx.host.type(path);
    await sleep(600);
    ctx.host.key({ key: "Enter" });
    await sleep(1200);
    ctx.host.key({ key: "Enter" });
  }
}

/** Real drag of a file from an OS file source onto screen point `to`. */
async function dragFileTo(ctx, file, to) {
  const record = { from: null, helper: null };
  let helper = null;
  try {
    if (ctx.platform === "linux") {
      const script = join(ctx.out, "drag-source.py");
      writeFileSync(script, `import sys, gi
gi.require_version('Gtk', '3.0')
from gi.repository import Gtk, Gdk
uri = 'file://' + sys.argv[1]
w = Gtk.Window(title='QA DRAG SOURCE'); w.set_default_size(260, 140); w.move(40, 40)
b = Gtk.EventBox(); l = Gtk.Label(); l.set_markup('<span size="xx-large" weight="bold">DRAGME</span>'); b.add(l); w.add(b)
b.drag_source_set(Gdk.ModifierType.BUTTON1_MASK, [], Gdk.DragAction.COPY); b.drag_source_add_uri_targets()
b.connect('drag-data-get', lambda wd, ctx, data, info, t: data.set_uris([uri]))
w.connect('destroy', Gtk.main_quit); w.show_all(); Gtk.main()
`);
      helper = spawn("python3", [script, file.path], { stdio: "ignore", env: process.env });
      const window = await until(() => ctx.run("xdotool", ["search", "--name", "QA DRAG SOURCE"]).stdout.split(/\s+/).filter(Boolean)[0], 15000, 500);
      record.helper = window ?? null;
      if (!window) return { ...record, ok: false, stderr: "drag source window did not appear" };
      ctx.run("xdotool", ["windowactivate", "--sync", window]);
      await sleep(800);
      const from = await until(() => {
        const hit = screenWords(ctx).find((word) => /DRAGME/i.test(word.text));
        return hit ? [hit.left + Math.round(hit.width / 2), hit.top + Math.round(hit.height / 2)] : null;
      }, 8000, 700);
      // OCR missed the label (run 41767162699); the label fills the helper window, so its
      // centre from xdotool is a press point on the drag source.
      const geometry = from ? null : ctx.run("xdotool", ["getwindowgeometry", "--shell", window]).stdout;
      const value = (name) => Number(new RegExp(`^${name}=(\\d+)`, "m").exec(geometry ?? "")?.[1] ?? NaN);
      record.from = from ?? (Number.isFinite(value("X")) ? [value("X") + Math.round(value("WIDTH") / 2), value("Y") + Math.round(value("HEIGHT") / 2)] : null);
      record.from_via = from ? "ocr" : record.from ? "window geometry" : null;
      if (!record.from) return { ...record, ok: false, stderr: "DRAGME not read on screen" };
    } else if (ctx.platform === "win32") {
      ctx.run("explorer.exe", [`/select,${file.path}`]);
      await sleep(4000);
      const from = await until(() => {
        const hit = screenWords(ctx).find((word) => /upload-fixture|upload-?fix|fixture[.,]txt/i.test(word.text));
        return hit ? [hit.left + Math.round(hit.width / 2), hit.top + Math.round(hit.height / 2)] : null;
      }, 10000, 800);
      record.from = from;
      if (!from) return { ...record, ok: false, stderr: "the file name was not read in Explorer" };
    } else {
      ctx.run("open", ["-R", file.path]);
      await sleep(3500);
      const from = await until(() => {
        // Finder wraps the icon label over two lines ("upload-f" / "ixture.txt"; OCR read
        // "uploa" and "ixture,txt" in macOS run 41767162699 probe 298): either half is the file.
        const hit = screenWords(ctx).find((word) => /upload-fixture|^uploa|ixture[.,]txt/i.test(word.text));
        return hit ? [hit.left + Math.round(hit.width / 2), hit.top + Math.round(hit.height / 2)] : null;
      }, 10000, 800);
      // `open -R` selects the file, and OCR cannot read its white-on-blue label (macOS run
      // 41801115761 probe 231): the selection highlight is a blue band 12-24px tall under the
      // icon, so the press point is the icon centre above it.
      record.from = from ?? finderSelection(ctx);
      record.from_via = from ? "ocr" : record.from ? "selection highlight" : null;
      if (!record.from) return { ...record, ok: false, stderr: "the file name was not read in Finder" };
    }
    const [x0, y0] = record.from;
    ctx.host.mouseDown(x0, y0);
    await sleep(300);
    // A short first move past the OS drag threshold, held, so the source starts the drag before
    // the long move.
    ctx.host.mouseMove(x0 + 8, y0 + 8);
    await sleep(400);
    const steps = 20;
    for (let index = 1; index <= steps; index += 1) {
      ctx.host.mouseMove(Math.round(x0 + ((to[0] - x0) * index) / steps), Math.round(y0 + ((to[1] - y0) * index) / steps));
      await sleep(80);
    }
    await sleep(500);
    ctx.host.mouseUp(to[0], to[1]);
    return { ...record, ok: true };
  } finally {
    helper?.kill();
    await sleep(1500);
    record.file_manager_closed = closeFileManager(ctx);
  }
}

/** The Finder icon above the selected item's blue label highlight, or null. */
function finderSelection(ctx) {
  const image = ctx.decodePng(ctx.probeShot());
  const blue = ([r, g, b]) => r < 40 && g >= 85 && g <= 125 && b >= 205;
  const bands = [];
  let band = null;
  // The Dock (bottom 80px) has blue icons too.
  for (let y = 0; y < image.height - 80; y += 1) {
    let count = 0, a = Infinity, b = -1;
    for (let x = 0; x < image.width; x += 1) {
      if (!blue(image.rgb(x, y))) continue;
      count += 1;
      a = Math.min(a, x);
      b = Math.max(b, x);
    }
    if (count < 20) continue;
    if (band && y - band.y1 <= 2) Object.assign(band, { y1: y, x0: Math.min(band.x0, a), x1: Math.max(band.x1, b) });
    else bands.push((band = { y0: y, y1: y, x0: a, x1: b }));
  }
  const label = bands.find((entry) => entry.y1 - entry.y0 >= 10 && entry.y1 - entry.y0 <= 26 && entry.x1 - entry.x0 >= 40 && entry.x1 - entry.x0 <= 240);
  return label ? [Math.round((label.x0 + label.x1) / 2), Math.max(0, label.y0 - 38)] : null;
}

/** The OS-native print UI words read on screen. */
const PRINT_WORDS = {
  linux: ["printer", "copies", "pagesetup", "printtofile", "range"],
  win32: ["destination", "copies", "layout", "microsoftprinttopdf", "saveaspdf", "pages"],
  darwin: ["printer", "copies", "pdf"],
};
function printUiSeen(ctx) {
  const words = screenWords(ctx).map((word) => ctx_norm(word.text));
  const wanted = PRINT_WORDS[ctx.platform];
  return wanted.filter((needle) => words.some((word) => word.includes(needle)));
}

// ---------------------------------------------------------------- before the restart (webview)
export async function dailyWebviewBefore(ctx) {
  const { fixtureBase, siteB } = ctx;
  const g1 = gap(ctx, "g1", ["tabs_restored", "back_1", "back_2", "session_file_isolated", "no_session_outside_isolation"]);
  const g2 = gap(ctx, "g2", ["bookmark_star", "bookmark_bar_shows", "omnibox_mix", "omnibox_accept", "history_search", "history_delete", "history_clear_range", "folder_created", "folder_move", "folder_survives_restart", "folder_on_bar_after_restart"]);
  const g3 = gap(ctx, "g3", ["save_prompt_shown", "saved", "keystore_item", "fill_offer_after_restart", "autofilled_after_restart", "cross_origin_iframe_empty"]);
  const g13 = gap(ctx, "g13", ["zoom_set", "zoom_other_site_default", "zoom_site_restored", "zoom_persisted_file", "zoom_survives_restart", "pdf_webview_text", "pdf_webview_pages", "pdf_chromium_text", "pdf_chromium_pages"]);
  const g8 = gap(ctx, "g8", ["upload_picker", "drag_in", "upload_picker_chromium", "drag_in_chromium"]);
  ctx.state.daily = {};
  const daily = ctx.state.daily;

  // G1: tab A walks a -> b -> c; tab B is a second tab.
  await run(ctx, "daily-g1-tabs", "Tab A: /qa/a, /qa/b, /qa/c through the address bar; Mod+Shift+B opens tab B on /qa/second", async () => {
    ctx.host.focus();
    let list = ctx.sessions().list;
    if (!list.length) {
      // No browser tab is open (Windows run 41767162699 showed "No open tabs"): open tab A first
      // instead of typing URLs into nothing.
      await shortcut(ctx, { key: "b", mods: ["mod", "shift"] });
      list = (await until(() => (ctx.sessions().list.length ? ctx.sessions().list : null), 20000)) ?? [];
      await sleep(3000);
    }
    daily.tabA = (list.find((entry) => entry.engine === "system") ?? list.at(-1))?.browserId ?? null;
    const seen = [];
    for (const [path, word] of [["/qa/a.html", "ALPHAPAGE"], ["/qa/b.html", "BRAVOPAGE"], ["/qa/c.html", "CHARLIEPAGE"]]) {
      seen.push(await visit(ctx, `${fixtureBase}${path}`, word));
    }
    await shortcut(ctx, { key: "b", mods: ["mod", "shift"] });
    const tabB = await until(() => ctx.sessions().list.find((entry) => entry.browserId !== daily.tabA)?.browserId, 20000);
    daily.tabB = tabB ?? null;
    await sleep(3000);
    seen.push(await visit(ctx, `${fixtureBase}/qa/second.html`, "SECONDTAB"));
    return { ok: seen.every(Boolean) && Boolean(tabB), seen, tabA: daily.tabA, tabB };
  });

  // G2: bookmark (Mod+D), bar item, omnibox mix, history page, folders.
  await run(ctx, "daily-g2-bookmark", "Tab B: load /qa/b, Mod+D bookmarks it; the bookmark bar shows 'QA Bravo Page'", async () => {
    await visit(ctx, `${fixtureBase}/qa/b.html`, "BRAVOPAGE");
    await shortcut(ctx, { key: "d", mods: ["mod"] });
    const saved = await until(() => bookmarksFile(ctx).bookmarks.find((entry) => entry.url.endsWith("/qa/b.html")), 10000);
    g2.checks.bookmark_star = Boolean(saved);
    g2.bookmark = saved ?? null;
    const bar = await waitWords(ctx, ["bravo"], 10000);
    g2.checks.bookmark_bar_shows = Boolean(bar);
    return { ok: g2.checks.bookmark_star && g2.checks.bookmark_bar_shows };
  });
  await run(ctx, "daily-g2-omnibox", "Address bar: type 'QA'; suggestions hold an open-tab row, a bookmark and a history entry; Down+Enter opens one", async () => {
    // Tab B leaves /qa/b first: the first suggestion is the Bravo bookmark, and accepting it on
    // /qa/b itself changes nothing (run 41767162699: omnibox_after null on linux and macOS).
    await visit(ctx, `${fixtureBase}/qa/second.html`, "SECONDTAB");
    const before = tab(ctx, daily.tabB)?.url ?? null;
    await ctx.clickAddressBar();
    ctx.host.key({ key: "a", mods: ["mod"] });
    ctx.host.type("QA");
    await sleep(1500);
    // The rows are small text: read a few frames and keep what any of them saw. The open-tab row
    // reads "Switch to tab · url"; OCR has read it as "switch to tat" (macOS probe 149) or
    // missed "switch". A bare "tab" is not enough (the tab bar shows "QA Second Tab"), so the
    // fallback needs "to" directly followed by "tab"/"tat" on the same line.
    const switchRow = (words) => has(words, "switch") || words.some((to) => ctx_norm(to.text) === "to"
      && words.some((next) => /^ta[bt]/.test(ctx_norm(next.text)) && Math.abs(next.top - to.top) < 8 && next.left > to.left && next.left - (to.left + to.width) < 40));
    const rows = { tab: false, bookmark: false, history: false };
    for (let frame = 0; frame < 4 && !(rows.tab && rows.bookmark && rows.history); frame += 1) {
      const words = screenWords(ctx);
      rows.tab ||= switchRow(words);
      rows.bookmark ||= has(words, "bravo");
      rows.history ||= has(words, "alpha");
      if (frame < 3) await sleep(700);
    }
    g2.omnibox_rows = rows;
    g2.checks.omnibox_mix = rows.tab && rows.bookmark && rows.history;
    ctx.host.key({ key: "Down" });
    await sleep(300);
    ctx.host.key({ key: "Enter" });
    const after = await until(() => {
      const url = tab(ctx, daily.tabB)?.url ?? null;
      return url && url !== before && /\/qa\//.test(url) ? url : null;
    }, 10000);
    // An open-tab row switches tabs instead of navigating: either outcome is an accepted row.
    const switched = !after && ctx.sessions().list.some((entry) => entry.browserId === daily.tabA);
    g2.omnibox_after = after;
    g2.checks.omnibox_accept = Boolean(after) || (switched && rows.tab && (await waitWords(ctx, ["charlie"], 5000)) !== null);
    return { ok: g2.checks.omnibox_mix && g2.checks.omnibox_accept, rows, after };
  });
  await run(ctx, "daily-g2-history", "History menu > Show all history: search 'Alpha', delete that row, clear the last hour", async () => {
    // Make sure tab B is active again (the omnibox may have switched to tab A).
    g2.activate_b = await activateTab(ctx, daily.tabB, null);
    ctx.host.focus();
    await ctx.chromeFocusClick();
    const opened = await clickToolbarIcon(ctx, 1, async () => (await waitWords(ctx, ["showall", "recent"], 2500)) !== null);
    g2.history_menu = opened;
    if (!opened.ok) return { ok: false, stderr: "History menu did not open", opened };
    // "Show all history": OCR reads it as separate words.
    const all = await clickWord(ctx, (text) => /^all$/i.test(text) || /showall/i.test(text), { timeout: 5000 });
    const page = await waitWords(ctx, ["clearbrowsing", "browsing"], 8000);
    if (!all || !page) return { ok: false, stderr: "history page did not open" };
    const search = await clickWord(ctx, (text) => /^search$/i.test(text), { timeout: 5000, pick: "first" });
    ctx.host.type("Alpha");
    await sleep(2000);
    let words = screenWords(ctx);
    // Only the list below the search box counts: the bookmark bar above it always shows
    // "QA Bravo Page", so a whole-screen "no bravo" test could never pass.
    const listTop = (search?.[1] ?? 0) + 12;
    const listed = words.filter((word) => word.top > listTop);
    g2.history_search_rows = listed.filter((word) => /alpha|bravo|charlie|second/i.test(word.text)).map((word) => word.text);
    g2.checks.history_search = Boolean(search) && has(listed, "alpha") && !has(listed, "bravo");
    // The row's trash is the last icon-sized control on the row line, right of its url text.
    const row = listed.filter((word) => /alpha/i.test(word.text)).sort((a, b) => b.top - a.top)[0];
    let deleted = false;
    if (row) {
      const image = ctx.decodePng(ctx.probeShot());
      const y = row.top + Math.round(row.height / 2);
      const runs = rowRuns(image, y, row.left + row.width + 4, image.width);
      // Past the window's right edge the screen differs from the row in one wide run.
      while (runs.length && runs.at(-1)[1] - runs.at(-1)[0] > 60) runs.pop();
      const trash = runs.filter(([a, b]) => b - a >= 6 && b - a <= 30).at(-1);
      g2.trash_at = trash ? [Math.round((trash[0] + trash[1]) / 2), y] : null;
      if (trash) {
        ctx.host.click(Math.round((trash[0] + trash[1]) / 2), y);
        deleted = Boolean(await until(async () => !(await historyRows(ctx)).some((entry) => entry.url.endsWith("/qa/a.html")), 8000));
      }
    }
    g2.checks.history_delete = deleted;
    // Clear the search, then "Clear browsing data" (range: Last hour).
    ctx.host.click(...(search ?? [0, 0]));
    ctx.host.key({ key: "a", mods: ["mod"] });
    ctx.host.key({ key: "BackSpace" });
    await sleep(1000);
    const clear = await clickWord(ctx, (text) => /^clear$/i.test(text), { timeout: 5000, pick: "rightmost" });
    const empty = await until(async () => (await historyRows(ctx)).length === 0, 8000);
    words = screenWords(ctx);
    g2.history_empty_text = has(words, "nobrowsing") || has(words, "history.");
    g2.checks.history_clear_range = Boolean(clear) && Boolean(empty);
    g2.rows_after_clear = (await historyRows(ctx)).length;
    return { ok: g2.checks.history_search && g2.checks.history_delete && g2.checks.history_clear_range };
  });
  await run(ctx, "daily-g2-folder", "Library > bookmarks: New folder 'QAFolder', move 'QA Bravo Page' into it", async () => {
    let tabBtn = await clickWord(ctx, (text) => /^bookmarks$/i.test(text), { timeout: 5000 });
    if (!tabBtn) {
      // The history step did not leave the library open: History menu > Bookmarks manager.
      const opened = await clickToolbarIcon(ctx, 1, async () => (await waitWords(ctx, ["showall", "recent"], 2500)) !== null);
      g2.folder_menu = opened;
      if (opened.ok) tabBtn = await clickWord(ctx, (text) => /^manager$/i.test(text) || /bookmarksmanager/i.test(text), { timeout: 5000 });
    }
    await sleep(1000);
    const newFolder = await clickWord(ctx, (text) => /^folder$/i.test(text), { timeout: 5000, pick: "first" });
    await sleep(800);
    ctx.host.type("QAFolder");
    ctx.host.key({ key: "Enter" });
    const folder = await until(() => bookmarksFile(ctx).folders.find((entry) => entry.name === "QAFolder"), 8000);
    g2.checks.folder_created = Boolean(folder);
    if (!folder) return { ok: false, stderr: "folder not created", tabBtn, newFolder };
    await sleep(1500);
    // The bravo row's "Move to…" select: focus it with a click, then pick the folder by typing.
    // "Bravo" is also on the tab bar and the bookmark bar above the library (Windows run
    // 41801115761 probe 057: the first hit was the bar, so no "Move" matched and move_at was
    // null); the row is the "Bravo" with a "Move" on its own line.
    const words = screenWords(ctx);
    const sameLine = (a, b) => Math.abs(a.top + a.height / 2 - (b.top + b.height / 2)) < 10;
    const row = words.filter((word) => /bravo/i.test(word.text)).find((word) => words.some((other) => /^move/i.test(other.text) && sameLine(other, word))) ?? null;
    const move = row ? words.filter((word) => /^move/i.test(word.text) && sameLine(word, row))[0] : null;
    if (move) {
      ctx.host.click(move.left + Math.round(move.width / 2), move.top + Math.round(move.height / 2));
      await sleep(1200);
      const option = await clickWord(ctx, (text) => /qafolder/i.test(text), { timeout: 4000, pick: "bottom" });
      if (!option) {
        ctx.host.type("  QAF");
        ctx.host.key({ key: "Enter" });
      }
    }
    const moved = await until(() => {
      const tree = bookmarksFile(ctx);
      const target = tree.folders.find((entry) => entry.name === "QAFolder");
      return target && tree.bookmarks.find((entry) => entry.url.endsWith("/qa/b.html") && entry.parentId === target.id);
    }, 8000);
    g2.checks.folder_move = Boolean(moved);
    g2.move_at = move ? [move.left, move.top] : null;
    ctx.host.key({ key: "Escape" });
    return { ok: g2.checks.folder_created && g2.checks.folder_move };
  });
  // Close the library panel (its close X sits right of the tab list; Escape from the page works too).
  await run(ctx, "daily-library-close", "Close the library panel", async () => {
    // The library sits below the toolbar, so the toolbar stays visible while it is open; its tab
    // row (History, Bookmarks, Downloads) is the signal, and its close X is the last control there.
    // The step only passes once the panel is gone: the old version passed on any page word
    // (Windows run 41801115761 screenshot 15 shows the panel still open after an "ok").
    const closed = await closeLibrary(ctx);
    const back = closed.ok ? await waitWords(ctx, ["bravopage", "secondtab", "charliepage"], 6000) : null;
    return { ok: closed.ok && Boolean(back), ...closed };
  });

  // G3: login save prompt, vault file, OS keystore.
  await run(ctx, "daily-g3-save", "Tab B /login.html: click the page, Tab into USERNAME, type user, Tab, type password, Enter submits; click Save in the password bar", async () => {
    g3.library_closed = await closeLibrary(ctx);
    await visit(ctx, `${fixtureBase}/login.html`, "LOGINPAGE");
    // The tab that shows the login page, confirmed through the CLI (a remembered id can point
    // at a tab the visit did not land in).
    const loginTab = tabAt(ctx, "/login.html")?.browserId ?? daily.tabB;
    g3.login_tab = loginTab;
    // Focus the page through its heading (big black text OCR reads reliably), not the grey
    // placeholders; the heading sits before the form, so Tab lands in USERNAME. Otherwise the
    // address bar keeps focus and the credentials end up in the URL.
    const page = await clickWord(ctx, (text) => /^LOGINPAGE$/i.test(text), { timeout: 8000, pick: "bottom" });
    await sleep(500);
    ctx.host.key({ key: "Tab" });
    await sleep(300);
    ctx.host.type("qauser");
    ctx.host.key({ key: "Tab" });
    await sleep(300);
    ctx.host.type("qa-pass-123");
    const typedOk = (fields) => Array.isArray(fields) && fields[0] === "qauser" && fields[1] === "qa-pass-123";
    let fields = await loginFields(ctx, loginTab);
    g3.typed_via = "heading click + Tab";
    if (!typedOk(fields)) {
      // The keys went elsewhere (Windows run 41801115761: typed ["",0], the LOGINPAGE click hit
      // nothing). Give the webview keyboard focus through the CLI, then click each field itself.
      g3.focus_cli = (await ctx.cliAsync("focus", "--browser-id", loginTab)).ok;
      for (const [placeholder, value] of [["USERNAME", "qauser"], ["PASSWORD", "qa-pass-123"]]) {
        await clickWord(ctx, (text) => new RegExp(`^${placeholder}$`, "i").test(text), { timeout: 5000, pick: "bottom" });
        await sleep(400);
        ctx.host.key({ key: "a", mods: ["mod"] });
        ctx.host.type(value);
        await sleep(300);
      }
      fields = await loginFields(ctx, loginTab);
      g3.typed_via = "cli focus + field clicks";
    }
    g3.typed = fields ? [fields[0], String(fields[1] ?? "").length] : null;
    // Enter in the password field: a trusted keydown for the capture script and an implicit submit.
    ctx.host.key({ key: "Enter" });
    const prompt = await until(() => {
      const words = screenWords(ctx);
      const save = words.filter((word) => word.text === "Save");
      const not = words.find((word) => /^now$/i.test(word.text) || /^not$/i.test(word.text));
      return has(words, "password") && save.length ? { save, not } : null;
    }, 15000);
    g3.checks.save_prompt_shown = Boolean(prompt);
    if (!prompt) return { ok: false, stderr: "no save prompt", page, typed: g3.typed };
    // The Save button: the rightmost "Save" left of "Not now" (the sentence starts with "Save" too).
    const buttons = prompt.save.filter((word) => !prompt.not || word.left < prompt.not.left).sort((a, b) => b.left - a.left);
    const button = buttons[0];
    ctx.host.click(button.left + Math.round(button.width / 2), button.top + Math.round(button.height / 2));
    const vault = await until(() => (readJson(join(ctx.dirs.data, "browser-vault", "vault.json"))?.items?.length ?? 0) > 0, 10000);
    g3.checks.saved = Boolean(vault);
    // The key's OS keystore item.
    let keystore = null;
    if (ctx.platform === "linux") {
      const result = ctx.run("secret-tool", ["search", "service", "com.ferryx.app.browser-vault"]);
      keystore = { status: result.status, out: result.stdout.split("\n").filter((line) => !/^secret\s*=/.test(line)).join("\n").slice(0, 600), err: result.stderr.slice(0, 300) };
      // secret-tool prints the attributes (attribute.service = ...) on stderr, the label on stdout.
      g3.checks.keystore_item = result.ok && /com\.ferryx\.app\.browser-vault/.test(`${result.stdout}\n${result.stderr}`);
    } else if (ctx.platform === "darwin") {
      const result = ctx.run("security", ["find-generic-password", "-s", "com.ferryx.app.browser-vault"]);
      keystore = { status: result.status, out: result.stdout.slice(0, 600), err: result.stderr.slice(0, 300) };
      g3.checks.keystore_item = result.ok;
    } else {
      const dir = join(ctx.dirs.data, "browser-vault");
      const blobs = existsSync(dir) ? readdirSync(dir).filter((name) => /^key-.*\.dpapi$/.test(name)) : [];
      keystore = { dpapi: blobs };
      g3.checks.keystore_item = blobs.length > 0;
    }
    g3.keystore = keystore;
    return { ok: g3.checks.save_prompt_shown && g3.checks.saved && g3.checks.keystore_item, keystore };
  });

  // G13: per-site zoom on the webview.
  await run(ctx, "daily-g13-zoom", "Site A zoomed in twice (125%); site B stays 100%; back on A it is 125% again", async () => {
    await visit(ctx, `${fixtureBase}/example.html`, ["domain", "illustrative"]);
    daily.tabB = tabAt(ctx, "/example.html")?.browserId ?? daily.tabB;
    // From 100%, one press at a time, each confirmed through the CLI before the next: two
    // presses 500ms apart reached only 1.1 on Windows run 41801115761 (one press was lost).
    await shortcut(ctx, { key: "0", mods: ["mod"] });
    g13.zoom_reset = Boolean(await until(() => tab(ctx, daily.tabB)?.zoomFactor === 1, 6000, 500));
    g13.zoom_steps = [];
    for (let press = 0; press < 4 && tab(ctx, daily.tabB)?.zoomFactor !== 1.25; press += 1) {
      const before = tab(ctx, daily.tabB)?.zoomFactor ?? null;
      await shortcut(ctx, { key: "=", mods: ["mod"] });
      const after = await until(() => {
        const now = tab(ctx, daily.tabB)?.zoomFactor ?? null;
        return now !== null && now !== before ? now : null;
      }, 5000, 400);
      g13.zoom_steps.push(after);
    }
    const set = await until(() => tab(ctx, daily.tabB)?.zoomFactor === 1.25, 4000);
    g13.checks.zoom_set = Boolean(set);
    await visit(ctx, `${siteB}/example.html`, ["domain", "illustrative"]);
    const other = await until(() => tab(ctx, daily.tabB)?.zoomFactor === 1, 8000);
    g13.checks.zoom_other_site_default = Boolean(other) && /:\d+\/example/.test(tab(ctx, daily.tabB)?.url ?? "");
    await visit(ctx, `${fixtureBase}/example.html`, ["domain", "illustrative"]);
    const back = await until(() => tab(ctx, daily.tabB)?.zoomFactor === 1.25, 8000);
    g13.checks.zoom_site_restored = Boolean(back);
    const file = readJson(join(ctx.dirs.data, "browser-site-zoom.json"));
    const key = new URL(fixtureBase).host;
    g13.zoom_file = file;
    g13.checks.zoom_persisted_file = JSON.stringify(file ?? {}).includes(`"${key}":1.25`);
    g13.zoom_label = screenWords(ctx).map((word) => word.text).find((text) => /^\d{2,3}%$/.test(text)) ?? null;
    return { ok: g13.checks.zoom_set && g13.checks.zoom_other_site_default && g13.checks.zoom_site_restored && g13.checks.zoom_persisted_file };
  });
  await run(ctx, "daily-g13-pdf-webview", "Webview tab: load /doc.pdf; page 1 text renders, page 2 after wheel", async () => {
    await visit(ctx, `${fixtureBase}/doc.pdf`, null);
    const one = await waitWords(ctx, ["ferryxpdf"], 30000);
    g13.checks.pdf_webview_text = Boolean(one);
    let two = await waitWords(ctx, ["two", "/2", "of2"], 2000);
    for (let index = 0; !two && index < 8; index += 1) {
      const words = screenWords(ctx);
      const anchor = words.find((word) => /ferryxpdf/i.test(word.text)) ?? { left: 700, top: 500, width: 0, height: 0 };
      ctx.host.wheel(anchor.left + 40, anchor.top + 40, 6);
      await sleep(1200);
      two = await waitWords(ctx, ["two", "/2", "of2"], 1500);
    }
    g13.checks.pdf_webview_pages = Boolean(two);
    const pdfTab = tabAt(ctx, "/doc.pdf");
    g13.pdf_webview = { one, two, url: pdfTab?.url ?? tab(ctx, daily.tabB)?.url, title: pdfTab?.title ?? tab(ctx, daily.tabB)?.title };
    return { ok: g13.checks.pdf_webview_text && g13.checks.pdf_webview_pages };
  });

  // G8 on the webview: native file chooser, then a real file drag from the OS.
  const file = fixtureFile(ctx);
  await run(ctx, "daily-g8-upload", "Webview /upload.html: click UPLOADPICK, type the path in the native dialog; then drag the file from the OS onto DROPZONE", async () => {
    await visit(ctx, `${fixtureBase}/upload.html`, "UPLOADPICK");
    await clickWord(ctx, (text) => /UPLOADPICK/i.test(text), { timeout: 8000 });
    await answerOpenDialog(ctx, file.path);
    const picked = await until(() => tab(ctx, daily.tabB)?.title === `picked:${file.name}:${file.size}`, 12000);
    g8.checks.upload_picker = Boolean(picked);
    g8.picker_title = tab(ctx, daily.tabB)?.title ?? null;
    if (!picked) ctx.closeDialogs();
    g8.panel_closed = await closeFilePanel(ctx);
    ctx.host.focus();
    const zone = await until(() => {
      const hit = screenWords(ctx).find((word) => /DROPZONE/i.test(word.text));
      return hit ? [hit.left + Math.round(hit.width / 2), hit.top + Math.round(hit.height / 2)] : null;
    }, 8000);
    g8.drop_zone = zone;
    if (!zone) return { ok: false, stderr: "DROPZONE not visible" };
    g8.drag = await dragFileTo(ctx, file, zone);
    const dropped = await until(() => tab(ctx, daily.tabB)?.title === `dropped:${file.name}:${file.size}`, 8000);
    g8.checks.drag_in = Boolean(dropped);
    g8.drop_title = tab(ctx, daily.tabB)?.title ?? null;
    ctx.host.focus();
    return { ok: g8.checks.upload_picker && g8.checks.drag_in };
  });

  if (ctx.platform !== "darwin") {
    const g6 = gap(ctx, "g6", ["prompt_allowed", "os_notification"]);
    await run(ctx, "daily-g6-notification", "Webview /notify.html: click NOTIFYME, Allow in Ferryx's prompt; the OS shows the 'ferryx-qa' notification", async () => {
      let monitor = null;
      const log = join(ctx.out, "dbus-notify.log");
      if (ctx.platform === "linux") {
        writeFileSync(log, "");
        monitor = spawn("sh", ["-c", `exec dbus-monitor --session "interface='org.freedesktop.Notifications',member='Notify'" > '${log}' 2>&1`], { stdio: "ignore", env: process.env });
        await sleep(1000);
      }
      try {
        await visit(ctx, `${fixtureBase}/notify.html`, "NOTIFYME");
        await clickWord(ctx, (text) => /NOTIFYME/i.test(text), { timeout: 8000 });
        await waitWords(ctx, ["wants"], 10000);
        // OCR reads the small "Allow" as "atow" or misses it (linux run 41767162699), so fuzzy
        // spellings count, and the filled button right of "wants" on the prompt line is the fallback.
        let allow = await clickWord(ctx, (text) => /^(allow|atow|alow|aliow|allo|aflow)$/i.test(text), { timeout: 4000, pick: "rightmost" });
        g6.allow_via = allow ? "ocr" : null;
        if (!allow) {
          const wants = screenWords(ctx).find((word) => /^wants$/i.test(word.text));
          const at = wants ? findAllowButton(ctx, wants) : null;
          if (at) {
            ctx.host.click(...at);
            allow = at;
            g6.allow_via = "filled button right of 'wants'";
          }
        }
        g6.allow_at = allow;
        const granted = await until(() => /^perm:granted/.test(tab(ctx, daily.tabB)?.title ?? ""), 10000);
        g6.checks.prompt_allowed = Boolean(allow) && Boolean(granted);
        g6.title = tab(ctx, daily.tabB)?.title ?? null;
        if (ctx.platform === "linux") {
          const seen = await until(() => readFileSync(log, "utf8").includes("ferryx-qa"), 10000);
          g6.dbus = readFileSync(log, "utf8").split("\n").filter((line) => /string "/.test(line)).slice(0, 12);
          g6.checks.os_notification = Boolean(seen);
        } else {
          const history = ctx.run("powershell.exe", ["-NoProfile", "-Command",
            "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType=WindowsRuntime] > $null; " +
            "foreach ($id in @('com.ferryx.app', (Get-StartApps | Where-Object { $_.Name -like '*erryx*' } | Select-Object -ExpandProperty AppID))) { if ($id) { try { [Windows.UI.Notifications.ToastNotificationManager]::History.GetHistory($id) | ForEach-Object { $_.Content.GetXml() } } catch {} } }"], { timeout: 30000 });
          const onScreen = await waitWords(ctx, ["ferryx-qa", "ferryxqa"], 10000);
          g6.toast_history = history.stdout.slice(0, 600);
          g6.toast_on_screen = onScreen;
          g6.checks.os_notification = /ferryx-qa/.test(history.stdout) || Boolean(onScreen);
        }
      } finally {
        monitor?.kill();
      }
      return { ok: g6.checks.prompt_allowed && g6.checks.os_notification };
    });

    const g9 = gap(ctx, "g9", ["print_dialog", "print_cancelled", "pdf_saved"]);
    await run(ctx, "daily-g9-print", "Webview /print.html: Mod+P opens the OS print UI; cancel; Save as PDF writes a %PDF file", async () => {
      await visit(ctx, `${fixtureBase}/print.html`, "PRINTPAGE");
      await shortcut(ctx, { key: "p", mods: ["mod"] });
      let seen = await until(() => (printUiSeen(ctx).length >= 2 ? printUiSeen(ctx) : null), 20000);
      g9.print_words = seen;
      g9.checks.print_dialog = Boolean(seen);
      if (seen) {
        const cancel = await clickWord(ctx, (text) => /^Cancel$/.test(text), { timeout: 4000, pick: "bottom" });
        if (!cancel) ctx.host.key({ key: "Escape" });
        const gone = await until(() => printUiSeen(ctx).length < 2, 10000);
        g9.checks.print_cancelled = Boolean(gone);
      }
      // Save as PDF: the toolbar's PDF button opens the same OS print UI on a webview tab.
      const pdfPath = join(ctx.out, "qa-print.pdf");
      ctx.host.focus();
      const opened = await clickToolbarIcon(ctx, 7, async () => printUiSeen(ctx).length >= 2, { spread: 1 });
      g9.pdf_button = opened;
      if (!opened.ok) {
        await shortcut(ctx, { key: "p", mods: ["mod"] });
        await until(() => printUiSeen(ctx).length >= 2, 15000);
      }
      if (ctx.platform === "linux") {
        // GTK print dialog: select the "Print to File" printer, then Print. GTK writes to its own
        // default output (the "File:" button showed "...-isolation/tmp/output.pdf" in linux run
        // 41801115761 probe 109, and there was no "Name:" field), so that path counts too.
        await clickWord(ctx, (text) => /^File$/i.test(text), { timeout: 5000 });
        await sleep(1000);
        const nameLabel = await until(() => screenWords(ctx).find((word) => /^Name:?$/.test(word.text)), 3000);
        if (nameLabel) {
          ctx.host.click(nameLabel.left + nameLabel.width + 120, nameLabel.top + Math.round(nameLabel.height / 2));
          ctx.host.key({ key: "a", mods: ["ctrl"] });
          ctx.host.type(pdfPath);
        }
        g9.print_click = await clickWord(ctx, (text) => /^Print$/.test(text), { timeout: 5000, pick: "bottom" });
      } else {
        // WebView2 print preview: the printer is preset to "Microsoft Print to PDF" (Windows run
        // 41801115761 probe 120). Its blue Print button is white-on-blue text OCR misses; it sits
        // about 128px left of the bottom "Cancel". Then the "Save Print Output As" dialog.
        const words = screenWords(ctx);
        if (!has(words, "saveaspdf") && !has(words, "microsoftprinttopdf") && !has(words, "printtopdf")) {
          const dest = words.find((word) => /^(Destination|Printer)$/.test(word.text));
          if (dest) {
            ctx.host.click(dest.left + 120, dest.top + Math.round(dest.height / 2) + 36);
            await sleep(1500);
            await clickWord(ctx, (text) => /^PDF$/i.test(text), { timeout: 5000 });
          }
        }
        await sleep(1500);
        const cancel = await until(() => screenWords(ctx).filter((word) => /^Cancel$/.test(word.text)).sort((a, b) => b.top - a.top)[0], 6000);
        if (cancel) {
          const at = [cancel.left + Math.round(cancel.width / 2) - 128, cancel.top + Math.round(cancel.height / 2)];
          ctx.host.click(...at);
          g9.save_click = at;
        } else {
          g9.save_click = await clickWord(ctx, (text) => /^(Save|Print)$/.test(text), { timeout: 4000, pick: "bottom" });
        }
        g9.save_dialog = await waitWords(ctx, ["saveprint", "outputas", "filename"], 15000);
        await sleep(1000);
        ctx.win.ps("typeraw", ["-Text", pdfPath]);
        await sleep(500);
        ctx.win.ps("keysraw", ["-Codes", "13"]);
      }
      const candidates = [pdfPath, join(ctx.dirs.tmp, "output.pdf"), join(ctx.dirs.home, "output.pdf"), join(process.cwd(), "output.pdf")];
      const isPdf = (path) => existsSync(path) && readFileSync(path).subarray(0, 4).toString() === "%PDF";
      const written = await until(() => candidates.find(isPdf) ?? null, 25000);
      g9.checks.pdf_saved = Boolean(written);
      g9.pdf = { path: written ?? pdfPath, exists: Boolean(written), size: written ? statSync(written).size : 0, looked_in: candidates };
      if (!written) {
        ctx.host.key({ key: "Escape" });
        ctx.closeDialogs();
      }
      ctx.host.focus();
      return { ok: g9.checks.print_dialog && g9.checks.print_cancelled && g9.checks.pdf_saved };
    });
  }

  // Back to a plain page so the restart restores a simple tab B.
  await run(ctx, "daily-tab-b-settle", "Tab B back on /qa/second before the restart", async () => {
    // A failed print or notification step can leave another tab or a dialog in front (linux run
    // 41767162699: this step failed and tab B stayed on /notify.html across the restart). Linux
    // run 41801115761 still showed the notification prompt here, and Windows had its Settings app
    // in front of Ferryx (screenshot 22), so the typed URL went nowhere.
    ctx.closeDialogs();
    ctx.host.key({ key: "Escape" });
    if (ctx.platform === "win32") ctx.run("powershell.exe", ["-NoProfile", "-Command", "Get-Process SystemSettings -ErrorAction SilentlyContinue | Stop-Process -Force"], { timeout: 20000 });
    ctx.host.focus();
    const wants = screenWords(ctx).find((word) => /^wants$/i.test(word.text));
    const allowAt = wants ? findAllowButton(ctx, wants) : null;
    if (allowAt) ctx.host.click(...allowAt);
    const activate = await activateTab(ctx, daily.tabB, null);
    const shown = await visit(ctx, `${fixtureBase}/qa/second.html`, "SECONDTAB");
    daily.tabB = tabAt(ctx, "/qa/second.html")?.browserId ?? daily.tabB;
    return { ok: Boolean(shown), activate, prompt_answered: allowAt };
  });
  daily.beforeRestart = ctx.sessions().list.map((entry) => ({ browserId: entry.browserId, url: entry.url }));
  g1.before_restart = daily.beforeRestart;
  return { g1, g2, g3, g13, g8 };
}

/** Session files outside the isolation dirs that a run must leave untouched. */
export function outsideSessionFiles(platform) {
  const home = homedir();
  const roots = platform === "darwin"
    ? [join(home, "Library", "Application Support", "com.ferryx.app")]
    : platform === "win32"
      ? [join(process.env.APPDATA ?? join(home, "AppData", "Roaming"), "com.ferryx.app"), join(process.env.LOCALAPPDATA ?? join(home, "AppData", "Local"), "com.ferryx.app")]
      : [join(process.env.XDG_DATA_HOME ?? join(home, ".local", "share"), "com.ferryx.app")];
  return roots.flatMap((root) => [join(root, "session_state.json"), join(root, "dev", "session_state.json")]);
}
export const statOrNull = (path) => (existsSync(path) ? statSync(path).mtimeMs : null);

// ---------------------------------------------------------------- restart
export async function dailyRestart(ctx) {
  const g1 = ctx.summary.daily.g1;
  return run(ctx, "daily-restart", "Quit like a user (macOS Mod+Q, else Alt+F4), relaunch on the same data dirs", async () => {
    const quit = await ctx.quitAndRelaunch("app-daily-restart.log");
    g1.quit = quit;
    const sessionFile = join(ctx.dirs.session, "session_state.json");
    g1.session_file = { path: sessionFile, exists: existsSync(sessionFile), mtime: statOrNull(sessionFile) };
    g1.checks.session_file_isolated = existsSync(sessionFile) && statSync(sessionFile).mtimeMs >= ctx.startedAtMs;
    const outside = outsideSessionFiles(ctx.platform).map((path) => ({ path, before: ctx.outsideBefore[path] ?? null, after: statOrNull(path) }));
    g1.outside_session_files = outside;
    g1.checks.no_session_outside_isolation = outside.every((entry) => entry.before === entry.after);
    return { ok: quit.ok, ...quit };
  });
}

// ---------------------------------------------------------------- after the restart (webview)
export async function dailyWebviewAfter(ctx) {
  const { fixtureBase } = ctx;
  const { g1, g2, g3, g13 } = ctx.summary.daily;
  const daily = ctx.state.daily;
  const g11 = gap(ctx, "g11", ["private_tab_opened", "private_cookie_not_shared", "private_not_in_history", "work_tab_opened", "work_cookie_not_in_default", "work_cookie_kept", "work_profile_dir", "no_private_profile_dir"]);

  await run(ctx, "daily-g1-restored", "After the restart: both tabs are listed with their URLs; tab A's Back goes c -> b -> a", async () => {
    // Every tab listed before the quit must come back with the same id and URL. A /qa/ filter
    // here dropped tab B whenever an earlier scenario left it elsewhere (linux run 41767162699:
    // tab B on /notify.html, so `want` had one URL and tabs_restored failed with both restored).
    const want = (daily.beforeRestart ?? []).filter((entry) => entry.url && entry.url !== "about:blank");
    const tabsKnown = Boolean(daily.tabA) && want.some((entry) => entry.browserId === daily.tabA && entry.url.endsWith("/qa/c.html"));
    const restored = await until(() => {
      const list = ctx.sessions().list;
      return want.length >= 2 && tabsKnown && want.every((entry) => list.some((now) => now.browserId === entry.browserId && now.url === entry.url)) ? list : null;
    }, 45000);
    g1.after_restart = ctx.sessions().list.map((entry) => ({ browserId: entry.browserId, url: entry.url, engine: entry.engine, visible: entry.visible }));
    g1.checks.tabs_restored = Boolean(restored);
    // Activate tab A (on /qa/c, tab bar title "QA Charlie Page") and confirm it through the CLI.
    daily.tabA = ctx.sessions().list.find((entry) => entry.browserId === daily.tabA)?.browserId
      ?? ctx.sessions().list.find((entry) => entry.url.endsWith("/qa/c.html"))?.browserId ?? daily.tabA;
    g1.activate_a = await activateTab(ctx, daily.tabA, (text) => /charlie/i.test(text));
    await sleep(1000);
    // Mod+[ is the browser.back app shortcut: it needs the app chrome focused, not the page.
    const goBack = async (expectPath, word) => {
      await shortcut(ctx, { key: "[", mods: ["mod"] });
      let landed = await until(() => tab(ctx, daily.tabA)?.url?.endsWith(expectPath), 10000);
      let via = "Mod+[";
      if (!landed) {
        // Shortcut lost (focus): the toolbar Back button is the same action through real input.
        const back = toolbarBack(ctx);
        if (back) {
          ctx.host.click(back.x, back.y);
          landed = await until(() => tab(ctx, daily.tabA)?.url?.endsWith(expectPath), 10000);
          via = "toolbar Back";
        }
      }
      return { ok: Boolean(landed) && Boolean(await waitWords(ctx, [word], 8000)), via, url: tab(ctx, daily.tabA)?.url ?? null };
    };
    g1.back_1 = await goBack("/qa/b.html", "bravopage");
    g1.checks.back_1 = g1.back_1.ok;
    g1.back_2 = await goBack("/qa/a.html", "alphapage");
    g1.checks.back_2 = g1.back_2.ok;
    g1.tab_a_url = tab(ctx, daily.tabA)?.url ?? null;
    return { ok: g1.checks.tabs_restored && g1.checks.back_1 && g1.checks.back_2, activate: g1.activate_a };
  });

  await run(ctx, "daily-g2-after", "After the restart: QAFolder still holds the bookmark and shows on the bookmark bar", async () => {
    const tree = bookmarksFile(ctx);
    const folder = tree.folders.find((entry) => entry.name === "QAFolder");
    g2.checks.folder_survives_restart = Boolean(folder) && tree.bookmarks.some((entry) => entry.parentId === folder.id && entry.url.endsWith("/qa/b.html"));
    g2.checks.folder_on_bar_after_restart = Boolean(await waitWords(ctx, ["qafolder"], 10000));
    return { ok: g2.checks.folder_survives_restart && g2.checks.folder_on_bar_after_restart };
  });

  await run(ctx, "daily-g3-after", "After the restart: /login.html offers 'Fill saved login'; clicking qauser fills both fields; the cross-origin iframe stays empty", async () => {
    g3.activate_b = await activateTab(ctx, daily.tabB, (text) => /second/i.test(text));
    await visit(ctx, `${fixtureBase}/login.html`, "LOGINPAGE");
    // Read the tab that really shows the page. Linux run 41801115761 restored tab B on
    // /notify.html, the visit landed in tab A, and tab B's "perm:idle" was read as the frame result.
    const loginTab = tabAt(ctx, "/login.html")?.browserId ?? daily.tabB;
    g3.after_tab = loginTab;
    const offer = await waitWords(ctx, ["fillsaved", "saved"], 15000);
    g3.checks.fill_offer_after_restart = Boolean(offer);
    // The fill button is the "qauser" on the offer bar's line, right of "saved"/"login".
    const button = await clickWord(ctx, (text) => /^qauser$/i.test(text), { timeout: 6000, pick: "first" });
    const filled = await until(async () => {
      const fields = await loginFields(ctx, loginTab);
      return fields && fields[0] === "qauser" && fields[1] === "qa-pass-123" ? fields : null;
    }, 8000);
    g3.filled_fields = filled ? [filled[0], filled[1].length] : await loginFields(ctx, loginTab);
    g3.checks.autofilled_after_restart = Boolean(button) && Boolean(filled);
    await visit(ctx, `${fixtureBase}/login-frame.html`, "FRAMEHOST");
    const frameTab = tabAt(ctx, "/login-frame.html")?.browserId ?? loginTab;
    g3.frame_tab = frameTab;
    await sleep(2500);
    // Even if a fill is offered here, it must not reach the cross-origin frame.
    await clickWord(ctx, (text) => /^qauser$/i.test(text), { timeout: 2500 });
    await sleep(3000);
    const title = tab(ctx, frameTab)?.title ?? null;
    g3.frame_title = title;
    g3.checks.cross_origin_iframe_empty = title === "frame:|";
    daily.tabB = frameTab;
    return { ok: g3.checks.fill_offer_after_restart && g3.checks.autofilled_after_restart && g3.checks.cross_origin_iframe_empty };
  });

  await run(ctx, "daily-g13-after", "After the restart: site A opens at 125%; Mod+0 resets", async () => {
    await visit(ctx, `${fixtureBase}/example.html`, ["domain", "illustrative"]);
    daily.tabB = tabAt(ctx, "/example.html")?.browserId ?? daily.tabB;
    const kept = await until(() => tab(ctx, daily.tabB)?.zoomFactor === 1.25, 10000);
    g13.checks.zoom_survives_restart = Boolean(kept);
    await shortcut(ctx, { key: "0", mods: ["mod"] });
    return { ok: Boolean(kept) };
  });

  await run(ctx, "daily-g11-private", "Tab context menu > Duplicate browser tab > Private; /secret sets a cookie there; the default tab does not get it and history has no /secret", async () => {
    await visit(ctx, `${fixtureBase}/qa/second.html`, "SECONDTAB");
    const before = new Set(ctx.sessions().list.map((entry) => entry.browserId));
    const title = await until(() => screenWords(ctx).filter((word) => /second/i.test(word.text)).sort((a, b) => a.top - b.top)[0], 8000);
    if (!title) return { ok: false, stderr: "tab title not read" };
    ctx.host.rightClick(title.left + Math.round(title.width / 2), title.top + Math.round(title.height / 2));
    await sleep(1200);
    await clickWord(ctx, (text) => /^Duplicate$/.test(text), { timeout: 5000 });
    await sleep(1000);
    await clickWord(ctx, (text) => /^Private$/.test(text), { timeout: 5000, pick: "rightmost" });
    const privateTab = await until(() => ctx.sessions().list.find((entry) => !before.has(entry.browserId) && entry.profileId === "private"), 15000);
    g11.checks.private_tab_opened = Boolean(privateTab);
    g11.private_tab = privateTab ?? null;
    if (!privateTab) return { ok: false, stderr: "no private tab listed" };
    await sleep(2000);
    await visit(ctx, `${fixtureBase}/secret.html`, "SECRETPAGE");
    const atSecret = await until(() => tab(ctx, privateTab.browserId)?.url?.endsWith("/secret.html"), 8000);
    // Back to the default tab B and read the cookies the server receives there. The private
    // duplicate has the same title ("QA Second Tab"), so a title click picked the private tab and
    // /cookies loaded there (linux run 41767162699 probe 193: "cookies:qa_secret=1"); select the
    // default tab by id instead.
    const defaultTab = ctx.sessions().list.find((entry) => entry.profileId !== "private" && entry.url.endsWith("/qa/second.html"))
      ?? ctx.sessions().list.find((entry) => entry.browserId === daily.tabB);
    g11.activate_default = await activateTab(ctx, defaultTab?.browserId ?? daily.tabB, null);
    await sleep(1200);
    await visit(ctx, `${fixtureBase}/cookies.html`, "COOKIES");
    const cookieTitle = await until(() => {
      const value = tab(ctx, defaultTab?.browserId ?? daily.tabB)?.title ?? "";
      return value.startsWith("cookies:") ? value : null;
    }, 8000);
    g11.default_cookie_title = cookieTitle;
    g11.checks.private_cookie_not_shared = Boolean(atSecret) && Boolean(cookieTitle) && !cookieTitle.includes("qa_secret");
    const rows = await historyRows(ctx);
    g11.checks.private_not_in_history = Boolean(atSecret) && !rows.some((entry) => entry.url.includes("/secret.html")) && rows.some((entry) => entry.url.includes("/cookies.html"));
    ctx.cli("close", "--browser-id", privateTab.browserId);
    return { ok: g11.checks.private_tab_opened && g11.checks.private_cookie_not_shared && g11.checks.private_not_in_history };
  });
}

// ---------------------------------------------------------------- Chromium engine tabs
export async function dailyChromium(ctx) {
  const { fixtureBase } = ctx;
  const { g8, g13, g11 } = ctx.summary.daily;
  const file = fixtureFile(ctx);
  const chromiumNavigate = async (browserId, url) => {
    ctx.host.focus();
    ctx.host.key({ key: "l", mods: ["mod"] });
    await sleep(800);
    ctx.host.type(url);
    ctx.host.key({ key: "Enter" });
    const arrived = await until(() => tab(ctx, browserId)?.url?.startsWith(url.split("#")[0]), 10000);
    if (!arrived) {
      // The typed URL did not land (focus): the CLI navigates, the page interaction stays real.
      await ctx.cliAsync("navigate", "--browser-id", browserId, "--url", url);
      return { typed: false };
    }
    return { typed: true };
  };
  const newChromiumTab = async () => {
    const before = new Set(ctx.sessions().list.map((entry) => entry.browserId));
    await shortcut(ctx, { key: "b", mods: ["mod", "shift"] });
    const created = await until(() => ctx.sessions().list.find((entry) => !before.has(entry.browserId) && entry.engine === "chromium"), 30000);
    await sleep(3000);
    return created ?? null;
  };

  await run(ctx, "daily-g8-chromium", "Chromium tab /upload.html: native file chooser and an OS file drag onto DROPZONE", async () => {
    const created = await newChromiumTab();
    if (!created) return { ok: false, stderr: "no chromium tab" };
    g8.chromium_tab = created.browserId;
    g8.chromium_nav = await chromiumNavigate(created.browserId, `${fixtureBase}/upload.html`);
    await waitWords(ctx, ["uploadpick"], 15000);
    await clickWord(ctx, (text) => /UPLOADPICK/i.test(text), { timeout: 8000 });
    await answerOpenDialog(ctx, file.path);
    const picked = await until(() => tab(ctx, created.browserId)?.title === `picked:${file.name}:${file.size}`, 12000);
    g8.checks.upload_picker_chromium = Boolean(picked);
    g8.chromium_picker_title = tab(ctx, created.browserId)?.title ?? null;
    if (!picked) ctx.closeDialogs();
    g8.chromium_panel_closed = await closeFilePanel(ctx);
    const zone = await until(() => {
      const hit = screenWords(ctx).find((word) => /DROPZONE/i.test(word.text));
      return hit ? [hit.left + Math.round(hit.width / 2), hit.top + Math.round(hit.height / 2)] : null;
    }, 8000);
    if (zone) g8.chromium_drag = await dragFileTo(ctx, file, zone);
    const dropped = await until(() => tab(ctx, created.browserId)?.title === `dropped:${file.name}:${file.size}`, 8000);
    g8.checks.drag_in_chromium = Boolean(dropped);
    g8.chromium_drop_title = tab(ctx, created.browserId)?.title ?? null;
    ctx.host.focus();
    return { ok: g8.checks.upload_picker_chromium && g8.checks.drag_in_chromium };
  });

  await run(ctx, "daily-g13-pdf-chromium", "Chromium tab: /doc.pdf renders in the PDF viewer, with 2 pages", async () => {
    const browserId = g8.chromium_tab;
    if (!browserId) return { ok: false, stderr: "no chromium tab" };
    await chromiumNavigate(browserId, `${fixtureBase}/doc.pdf`);
    const one = await waitWords(ctx, ["ferryxpdf"], 30000);
    g13.checks.pdf_chromium_text = Boolean(one);
    let two = await waitWords(ctx, ["/2", "of2", "two"], 3000);
    for (let index = 0; !two && index < 8; index += 1) {
      const anchor = screenWords(ctx).find((word) => /ferryxpdf/i.test(word.text)) ?? { left: 700, top: 500, width: 0, height: 0 };
      ctx.host.wheel(anchor.left + 40, anchor.top + 40, 6);
      await sleep(1200);
      two = await waitWords(ctx, ["/2", "of2", "two"], 1500);
    }
    g13.checks.pdf_chromium_pages = Boolean(two);
    g13.pdf_chromium = { one, two, title: tab(ctx, browserId)?.title ?? null };
    return { ok: g13.checks.pdf_chromium_text && g13.checks.pdf_chromium_pages };
  });

  await run(ctx, "daily-g11-profiles", "Settings > Session & Cookies: add profile 'work'; a work Chromium tab keeps its cookie; the default profile never sees it", async () => {
    // A file panel left by the upload step covers Settings (macOS run 41801115761).
    g11.panel_closed = await closeFilePanel(ctx);
    ctx.host.focus();
    const opened = await ctx.openSettings();
    await ctx.clickNav("Browser");
    await sleep(1500);
    let scrolled = await ctx.scrollSettingsTo("Session & Cookies");
    // OCR can split or drop the "&"; the next group's heading is right below the profile input.
    if (!scrolled.ok) scrolled = await ctx.scrollSettingsTo("Active Browser Tabs");
    const input = await clickWord(ctx, (text) => /^Profile$/.test(text), { timeout: 6000, pick: "rightmost" });
    ctx.host.type("work");
    await sleep(300);
    const add = await clickWord(ctx, (text) => /^Add$/.test(text), { timeout: 5000, pick: "rightmost" });
    await sleep(1500);
    g11.settings = { opened: opened.ok, scrolled: scrolled.ok, input, add };
    ctx.host.key({ key: "Escape" });
    await sleep(1500);
    // Duplicate the Chromium tab into the "work" profile from its tab context menu.
    const before = new Set(ctx.sessions().list.map((entry) => entry.browserId));
    const title = await until(() => screenWords(ctx).filter((word) => /ferryxpdf|doc\.pdf|upload/i.test(word.text)).sort((a, b) => a.top - b.top)[0], 8000);
    if (title) {
      ctx.host.rightClick(title.left + Math.round(title.width / 2), title.top + Math.round(title.height / 2));
      await sleep(1200);
      await clickWord(ctx, (text) => /^Duplicate$/.test(text), { timeout: 5000 });
      await sleep(1000);
      await clickWord(ctx, (text) => /^work$/i.test(text), { timeout: 5000, pick: "rightmost" });
    }
    const workTab = await until(() => ctx.sessions().list.find((entry) => !before.has(entry.browserId) && entry.profileId === "work"), 30000);
    g11.work_tab = workTab ?? null;
    g11.checks.work_tab_opened = Boolean(workTab) && workTab.engine === "chromium";
    if (!workTab) return { ok: false, stderr: "no work tab", ...g11.settings };
    await sleep(3000);
    await chromiumNavigate(workTab.browserId, `${fixtureBase}/set-work.html`);
    await waitWords(ctx, ["workset"], 15000);
    await chromiumNavigate(workTab.browserId, `${fixtureBase}/cookies.html`);
    const workCookies = await until(() => {
      const value = tab(ctx, workTab.browserId)?.title ?? "";
      return value.startsWith("cookies:") ? value : null;
    }, 10000);
    g11.work_cookie_title = workCookies;
    g11.checks.work_cookie_kept = Boolean(workCookies) && workCookies.includes("qa_work");
    const defaultTab = await newChromiumTab();
    if (defaultTab) {
      await chromiumNavigate(defaultTab.browserId, `${fixtureBase}/cookies.html`);
      const defaultCookies = await until(() => {
        const value = tab(ctx, defaultTab.browserId)?.title ?? "";
        return value.startsWith("cookies:") ? value : null;
      }, 10000);
      g11.default_chromium_cookie_title = defaultCookies;
      g11.checks.work_cookie_not_in_default = defaultTab.profileId !== "work" && Boolean(defaultCookies) && !defaultCookies.includes("qa_work");
      ctx.cli("close", "--browser-id", defaultTab.browserId);
    }
    const cef = join(ctx.dirs.data, "browser-cef");
    g11.cef_dirs = existsSync(cef) ? readdirSync(cef) : [];
    g11.checks.work_profile_dir = existsSync(join(cef, "work"));
    g11.checks.no_private_profile_dir = !existsSync(join(cef, "private"));
    ctx.cli("close", "--browser-id", workTab.browserId);
    if (g8.chromium_tab) ctx.cli("close", "--browser-id", g8.chromium_tab);
    return { ok: Object.values(g11.checks).every(Boolean) };
  });
}

/** Ensures `dir` exists (output subfolders). */
export const ensureDir = (dir) => (mkdirSync(dir, { recursive: true }), dir);
