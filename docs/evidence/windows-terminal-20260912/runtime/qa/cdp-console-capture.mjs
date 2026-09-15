// CDP console recorder for st_01a0958a Windows QA.
// Attaches to the WebView2 remote debugging endpoint and appends every console /
// exception / log entry to evidence\cdp-console.jsonl. Node >=22 built-in WebSocket
// (WHATWG API): addEventListener only, MessageEvent.data. Stop via fs.watch on
// cdp-stop.txt or a single bounded timeout. No interval polling.
// Usage: node cdp-console-capture.mjs [timeoutSeconds]
import { appendFileSync, writeFileSync, watch, unlinkSync } from "node:fs";
import { join } from "node:path";

const qaRoot = "C:\\Users\\sook\\ferryx-qa-rt-st01a0958a";
const out = join(qaRoot, "evidence", "cdp-console.jsonl");
const stopFile = join(qaRoot, "evidence", "cdp-stop.txt");
const port = 9223;
const capMs = (Number(process.argv[2]) || 1800) * 1000;
let done = false;

function finish(reason) {
  if (done) return;
  done = true;
  console.log("CDP_DONE " + reason);
  clearTimeout(capTimer);
  stopWatcher.close();
  try { ws.close(); } catch {}
  try { unlinkSync(stopFile); } catch {}
  process.exit(0);
}

// Single bounded timeout; stop-file watched with fs.watch (event-driven, no interval).
// Initial CDP connect uses a bounded retry (500ms steps, 120s cap): the recorder may be
// started right after cargo prints `Finished`, before the WebView2 endpoint exists.
const connectCapMs = 120000;
const connectStepMs = 500;
const connectDeadline = Date.now() + connectCapMs;
let endpoints = null;
let connectErr = null;
while (Date.now() < connectDeadline) {
  try {
    const list = await fetch(`http://127.0.0.1:${port}/json/list`).then((r) => r.json());
    if (Array.isArray(list) && list.length) { endpoints = list; break; }
    connectErr = "empty endpoint list";
  } catch (e) { connectErr = e; }
  await new Promise((res) => setTimeout(res, connectStepMs));
}
if (!endpoints) {
  writeFileSync(out + ".error", JSON.stringify({
    error: "CDP endpoint not reachable: " + String(connectErr),
    waitedMs: connectCapMs,
  }) + "\n");
  console.error("CDP_CONNECT_FAIL " + connectErr);
  process.exit(2);
}
const page = endpoints.find((t) => t.type === "page") ?? endpoints[0];
if (!page) {
  writeFileSync(out + ".error", JSON.stringify({ error: "no CDP page target", endpoints }) + "\n");
  console.error("NO_CDP_PAGE");
  process.exit(2);
}
console.log("CDP_TARGET " + page.url);
const ws = new WebSocket(page.webSocketDebuggerUrl);
let id = 0;
const send = (method, params = {}) =>
  ws.send(JSON.stringify({ id: ++id, method, params }));
const write = (obj) => appendFileSync(out, JSON.stringify(obj) + "\n");
writeFileSync(out, "");

ws.addEventListener("open", () => {
  send("Runtime.enable");
  send("Log.enable");
  send("Page.enable");
  console.log("CDP_ATTACHED");
});
ws.addEventListener("message", (ev) => {
  let msg;
  try { msg = JSON.parse(ev.data.toString()); } catch { return; }
  if (msg.method === "Runtime.consoleAPICalled") {
    const text = (msg.params.args ?? []).map((a) => a.value ?? a.description ?? a.type).join(" ");
    write({ t: Date.now(), kind: "console", type: msg.params.type, text, args: msg.params.args });
    if (text.includes("[ferryx:switch]")) console.log("SWITCH_EVENT " + text.slice(0, 400));
  } else if (msg.method === "Runtime.exceptionThrown") {
    write({ t: Date.now(), kind: "exception", detail: msg.params.exceptionDetails });
    console.log("EXCEPTION " + JSON.stringify(msg.params.exceptionDetails).slice(0, 300));
  } else if (msg.method === "Log.entryAdded") {
    write({ t: Date.now(), kind: "log", entry: msg.params.entry });
    console.log("LOG " + msg.params.entry.level + " " + String(msg.params.entry.text).slice(0, 200));
  }
});
ws.addEventListener("error", (e) => console.error("WS_ERROR " + (e.message ?? String(e))));
ws.addEventListener("close", () => { if (!done) { done = true; console.log("WS_CLOSED_UNEXPECTED"); process.exit(3); } });

// Single bounded timeout; stop-file watched with fs.watch (event-driven, no interval).
const capTimer = setTimeout(() => finish("CAP_TIMEOUT"), capMs);
const stopWatcher = watch(join(qaRoot, "evidence"), (_event, filename) => {
  if (filename === "cdp-stop.txt") finish("STOP_FILE");
});
