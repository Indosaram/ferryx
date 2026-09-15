// cdp-echo.mjs - QA-window-only keyboard input for st_01a0958a.
// The native terminal child HWND is HTTRANSPARENT (platform/windows_focus.rs): keyboard
// focus lives in the QA WebView2 child window and the webview forwards keys to the
// native surface. This script therefore injects the echo line through the QA app's OWN
// webview via CDP Input.dispatchKeyEvent (trusted browser input events) - no OS-level
// global keyboard/mouse injection, no dependence on desktop foreground, the user's
// installed app is never touched.
// Usage: node cdp-echo.mjs [line]   (default: echo FERRYX_WIN_START_OK)
import { appendFileSync } from "node:fs";
import { join } from "node:path";

const qaRoot = "C:\\Users\\sook\\ferryx-qa-rt-st01a0958a";
const out = join(qaRoot, "evidence", "cdp-echo.log");
const port = 9223;
const line = process.argv[2] ?? "echo FERRYX_WIN_START_OK";

function log(s) {
  const l = new Date().toISOString() + " " + s;
  console.log(l);
  try { appendFileSync(out, l + "\n"); } catch {}
}

const endpoints = await fetch(`http://127.0.0.1:${port}/json/list`).then((r) => r.json());
const page = endpoints.find((t) => t.type === "page") ?? endpoints[0];
if (!page) { log("NO_CDP_PAGE"); process.exit(2); }
log("CDP_TARGET " + page.url);

const ws = new WebSocket(page.webSocketDebuggerUrl);
let id = 0;
const pending = new Map();
const send = (method, params = {}) =>
  new Promise((resolve, reject) => {
    const mid = ++id;
    pending.set(mid, { resolve, reject });
    ws.send(JSON.stringify({ id: mid, method, params }));
    setTimeout(() => { if (pending.has(mid)) { pending.delete(mid); reject(new Error("timeout " + method)); } }, 10000);
  });

ws.addEventListener("message", (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result);
  }
});
ws.addEventListener("error", (e) => { log("WS_ERROR " + (e.message ?? e)); });
await new Promise((res, rej) => { ws.addEventListener("open", res); ws.addEventListener("error", rej); });
log("CDP_ATTACHED");

await send("Runtime.enable");
// Make the QA page consider itself focused even though the OS foreground is elsewhere
// (the Start menu holds it). This only affects the QA webview's own focus state.
try { await send("Emulation.setFocusEmulationEnabled", { enabled: true }); log("FOCUS_EMULATION_ON"); } catch (e) { log("FOCUS_EMULATION_ERR " + e); }
try { await send("Page.bringToFront"); log("BROUGHT_TO_FRONT"); } catch (e) { log("BRING_TO_FRONT_ERR " + e); }

// instrument keydown receipt inside the QA page only
await send("Runtime.evaluate", { expression: "window.__qaKeys = []; window.addEventListener('keydown', (e) => window.__qaKeys.push(e.key + ':' + (e.isTrusted ? 'T' : 'S') + ':' + e.code), true); 'installed'", returnByValue: true });

// 1. Focus the terminal area inside the page: report the element stack at the viewport
//    center-right (where the terminal pane renders) and click it via the CDP Input domain.
const probe = `(() => {
  const x = Math.round(window.innerWidth * 0.6), y = Math.round(window.innerHeight * 0.5);
  const el = document.elementFromPoint(x, y);
  const chain = [];
  let n = el;
  while (n && chain.length < 6) { chain.push(n.tagName + (n.className && typeof n.className === 'string' ? '.' + n.className.split(' ').slice(0,2).join('.') : '')); n = n.parentElement; }
  return JSON.stringify({ vw: innerWidth, vh: innerHeight, at: [x, y], chain });
})()`;
const probeRes = await send("Runtime.evaluate", { expression: probe, returnByValue: true });
log("PROBE " + (probeRes.result?.value ?? JSON.stringify(probeRes)));

// view metrics via CDP layout metrics (CSS px)
const metrics = await send("Page.getLayoutMetrics");
const cw = metrics.cssVisualViewport?.clientWidth, ch = metrics.cssVisualViewport?.clientHeight;
log("VIEWPORT " + cw + "x" + ch);
const tx = Math.round((metrics.cssVisualViewport?.clientWidth ?? cw) * 0.6);
const ty = Math.round((metrics.cssVisualViewport?.clientHeight ?? ch) * 0.5);

const click = (type, x, y) => send("Input.dispatchMouseEvent", {
  type, x, y, button: type === "mousePressed" || type === "mouseReleased" ? "left" : "none",
  buttons: type === "mousePressed" || type === "mouseReleased" ? 1 : 0, clickCount: 1,
});
await click("mouseMoved", tx, ty);
await click("mousePressed", tx, ty);
await click("mouseReleased", tx, ty);
log("CLICKED terminal area at " + tx + "," + ty);
await new Promise((r) => setTimeout(r, 400));

// 2. Type the echo line + Enter as trusted CDP key events.
const vkFor = (c) => {
  if (c >= "a" && c <= "z") return 65 + c.charCodeAt(0) - 97;
  if (c >= "A" && c <= "Z") return 65 + c.charCodeAt(0) - 65;
  if (c >= "0" && c <= "9") return 48 + c.charCodeAt(0) - 48;
  if (c === "_") return 189; if (c === " ") return 32;
  return 0;
};
const codeFor = (c) => {
  if (c >= "a" && c <= "z") return "Key" + c.toUpperCase();
  if (c >= "A" && c <= "Z") return "Key" + c.toUpperCase();
  if (c >= "0" && c <= "9") return "Digit" + c;
  if (c === "_") return "Minus"; if (c === " ") return "Space";
  return "";
};
for (const c of line) {
  const base = { key: c, code: codeFor(c), windowsVirtualKeyCode: vkFor(c), text: c, unmodifiedText: c };
  await send("Input.dispatchKeyEvent", { type: "keyDown", ...base });
  await send("Input.dispatchKeyEvent", { type: "keyUp", key: c, code: codeFor(c), windowsVirtualKeyCode: vkFor(c) });
}
await send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
log("KEYS_SENT " + JSON.stringify(line) + " +Enter");

// report what the QA page actually received
const recv = await send("Runtime.evaluate", { expression: "JSON.stringify({ hasFocus: document.hasFocus(), active: document.activeElement ? document.activeElement.tagName + '.' + (document.activeElement.className || '').toString().slice(0, 40) : 'none', keys: (window.__qaKeys || []).join('|') })", returnByValue: true });
log("PAGE_RECV " + (recv.result?.value ?? JSON.stringify(recv)));
ws.close();
process.exit(0);
