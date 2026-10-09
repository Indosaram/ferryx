// Attach (read-only) to a session's output ring and print the tail.
import net from "node:net";
import path from "node:path";
import readline from "node:readline";

// Resolves the live daemon endpoint the way src-tauri/src/daemon/server.rs::get_runtime_dir does:
// FERRYX_RUNTIME_DIR wins, then /tmp/rorca-<uid>{-dev} on unix or %LOCALAPPDATA%\Ferryx\runtime{,-dev}
// on Windows. Fails with a clear message instead of a TypeError where neither applies.
function daemonSocketPath(dev = false) {
  const override = process.env.FERRYX_RUNTIME_DIR;
  if (override) return path.join(override, process.platform === "win32" ? "daemon.port" : "daemon.sock");
  if (process.platform === "win32") {
    const base = process.env.LOCALAPPDATA || process.env.TEMP || "C:\\ProgramData";
    return path.join(base, "Ferryx", dev ? "runtime-dev" : "runtime", "daemon.port");
  }
  if (typeof process.getuid !== "function") {
    console.error(`Cannot resolve the Ferryx daemon runtime dir on ${process.platform}: expected /tmp/rorca-<uid> on unix or %LOCALAPPDATA%\\Ferryx\\runtime on Windows; set FERRYX_RUNTIME_DIR to override.`);
    process.exit(2);
  }
  return path.join("/tmp", `rorca-${process.getuid()}${dev ? "-dev" : ""}`, "daemon.sock");
}

const SOCKET_PATH = daemonSocketPath();
const sessionId = process.argv[2];
const after = Number(process.argv[3] ?? 0);

const socket = net.createConnection({ path: SOCKET_PATH });
const lines = readline.createInterface({ input: socket });
const pending = [];
let text = "";
let latestSeq = after;
let closed = false;

const call = (payload) =>
  new Promise((res) => { pending.push(res); socket.write(`${JSON.stringify(payload)}\n`); });

lines.on("line", (line) => {
  let msg;
  try { msg = JSON.parse(line); } catch { return; }
  if (msg.type === "stream" || msg.type === "output" || msg.type === "terminalOutput") {
    const payload = msg.data ?? msg.base64 ?? msg.bytes ?? msg.payload;
    if (payload) text += Buffer.from(payload, "base64").toString("utf8");
    if (msg.sequence) latestSeq = Math.max(latestSeq, msg.sequence);
    return;
  }
  const resolveNext = pending.shift();
  if (resolveNext) resolveNext(msg);
});

socket.on("connect", async () => {
  await call({ type: "handshake", version: 4 });
  const d = await call({ type: "describeSession", sessionId });
  const detail = JSON.parse(JSON.stringify(d));
  const seq = detail.session ?? {};
  const startAt = after > 0 ? after : Math.max(0, (seq.endSequence ?? 0) - Number(process.argv[4] ?? 400));
  console.log("DESCRIBE", JSON.stringify(d).slice(0, 200));
  const attach = await call({ type: "attach", sessionId, afterSequence: startAt });
  if (attach.history) text += Buffer.from(attach.history, "base64").toString("utf8");
  console.log("ATTACH", JSON.stringify(attach).slice(0, 300));
  // drain stream for a moment, then hard-close the socket so the process exits
  setTimeout(() => { closed = true; socket.destroy(); }, 1500);
});

socket.on("close", () => {
  console.log("LATEST_SEQ", latestSeq);
  console.log("TAIL_START");
  console.log(text.slice(-20000));
  process.exit(0);
});
socket.on("error", (e) => { console.error("SOCKET_ERROR", e.message); process.exit(2); });
