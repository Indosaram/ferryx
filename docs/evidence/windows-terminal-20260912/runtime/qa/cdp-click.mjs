// cdp-click.mjs - QA-window-only UI interaction for st_01a0958a.
// Attaches to the QA WebView2 CDP endpoint (port 9223) and clicks the page element
// whose visible text matches the argument (default "New Terminal"). This drives the
// QA app's own webview DOM - no global mouse/keyboard injection, the user's installed
// app is never touched. Node >=22 built-in WHATWG WebSocket.
// Usage: node cdp-click.mjs ["New Terminal"]
import { appendFileSync } from "node:fs";
import { join } from "node:path";

const qaRoot = "C:\\Users\\sook\\ferryx-qa-rt-st01a0958a";
const out = join(qaRoot, "evidence", "cdp-click.log");
const port = 9223;
const needle = process.argv[2] ?? "New Terminal";

function log(line) {
  const l = new Date().toISOString() + " " + line;
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

await send("Runtime.enable");
// Find the leaf element whose exact visible text matches the needle and click it with
// a full pointer/mouse sequence (React delegated listeners need a bubbling click).
const expr = `(() => {
  const needle = ${JSON.stringify(needle)};
  const els = Array.from(document.querySelectorAll("button, [role=button], a, div, span"));
  const area = (e) => { const r = e.getBoundingClientRect(); return r.width * r.height; };
  const exact = els.filter((e) => (e.textContent || "").trim() === needle && area(e) > 0);
  const el = exact.sort((a, b) => area(a) - area(b))[0];
  if (!el) return JSON.stringify({ found: false, candidates: els.filter((e) => (e.textContent || "").includes(needle)).slice(0, 5).map((e) => (e.textContent || "").trim().slice(0, 40)) });
  const r = el.getBoundingClientRect();
  const tag = el.tagName.toLowerCase();
  const text = (el.textContent || "").trim().slice(0, 60);
  const cx = r.x + r.width / 2, cy = r.y + r.height / 2;
  const mk = (t) => t.startsWith("pointer") ? new PointerEvent(t, { bubbles: true, cancelable: true, view: window, clientX: cx, clientY: cy, pointerId: 1, pointerType: "mouse", isPrimary: true }) : new MouseEvent(t, { bubbles: true, cancelable: true, view: window, clientX: cx, clientY: cy, button: 0 });
  ["pointerdown", "mousedown", "pointerup", "mouseup", "click"].forEach((t) => el.dispatchEvent(mk(t)));
  return JSON.stringify({ found: true, tag, text, x: r.x, y: r.y, w: r.width, h: r.height });
})()`;
const result = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
log("EVAL_RESULT " + JSON.stringify(result.result?.value ?? result));
ws.close();
process.exit(0);
