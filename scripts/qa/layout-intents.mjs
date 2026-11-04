import net from "node:net";
import path from "node:path";
import assert from "node:assert/strict";

const runtime = process.argv[process.argv.indexOf("--runtime") + 1];
if (!runtime || !process.argv.includes("--runtime")) throw new Error("--runtime is required");
const socket = net.createConnection(path.join(runtime, "daemon.sock"));
let buffered = "";
let pending;
socket.setEncoding("utf8");
socket.on("data", (data) => {
  buffered += data;
  let end;
  while ((end = buffered.indexOf("\n")) >= 0) {
    const message = JSON.parse(buffered.slice(0, end));
    buffered = buffered.slice(end + 1);
    if (!pending) throw new Error("Unexpected daemon response");
    const { resolve, timer } = pending;
    pending = undefined;
    clearTimeout(timer);
    resolve(message);
  }
});
socket.on("error", (error) => pending?.reject(error));
function request(value) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("Daemon response deadline")), 10000);
    pending = { resolve, reject, timer };
    socket.write(JSON.stringify(value) + "\n");
  });
}
try {
  assert.equal((await request({ type: "handshake", version: 5 })).type, "handshakeOk");
  let result = await request({ type: "layoutGet", window: "ws-layout-qa" });
  assert.equal(result.type, "layoutOk");
  if (result.view.layout.tabs.length) throw new Error("QA requires a fresh isolated data directory");
  result = await request({ type: "layoutApply", window: "ws-layout-qa", baseRevision: result.view.revision,
    intent: { type: "ADD_TAB_WITH_SESSION", tabId: "qa-one", label: "one", content: { kind: "terminal", backendSessionId: "qa-pty" }, activate: true } });
  const start = result.view.revision;
  const first = result.view.layout.tabs[0].activeNodeId;
  for (const intent of [
    { type: "SPLIT_PANE", tabId: "qa-one", targetNodeId: first, dir: "horizontal", first: false, content: { kind: "browser", url: "https://example.org" } },
    { type: "DETACH_PANE_TO_TAB", sourceTabId: "qa-one", nodeId: first + 1, tabId: "qa-two", label: "two", targetGroupId: "main" },
    { type: "CLOSE_TAB", tabId: "qa-two" },
  ]) {
    result = await request({ type: "layoutApply", window: "ws-layout-qa", baseRevision: result.view.revision, intent });
    assert.equal(result.type, "layoutOk");
  }
  const current = await request({ type: "layoutGet", window: "ws-layout-qa" });
  assert.equal(current.view.revision, start + 3);
  assert.equal(current.view.layout.tabs.length, 1);
  assert.deepEqual(current.view.layout.tabs[0].paneTree, { type: "leaf", nodeId: first, content: { kind: "terminal", backendSessionId: "qa-pty" } });
  const conflict = await request({ type: "layoutApply", window: "ws-layout-qa", baseRevision: start,
    intent: { type: "CLOSE_TAB", tabId: "qa-one" } });
  assert.equal(conflict.code, "LAYOUT_CONFLICT");
  assert.equal(conflict.revision, current.view.revision);
  console.log(`LAYOUT_QA_OK revision=${current.view.revision}`);
} finally {
  socket.destroy();
}
