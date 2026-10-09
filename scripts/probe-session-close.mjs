// Close a live backend session via the daemon UDS protocol (v4).
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

const socket = net.createConnection({ path: SOCKET_PATH });
const lines = readline.createInterface({ input: socket });
const pending = [];
lines.on("line", (line) => {
  const resolveNext = pending.shift();
  if (resolveNext) resolveNext(JSON.parse(line));
});
socket.on("error", (e) => { console.error("SOCKET_ERROR", e.message); process.exit(2); });
const call = (payload) =>
  new Promise((res) => { pending.push(res); socket.write(`${JSON.stringify(payload)}\n`); });

socket.on("connect", async () => {
  const hs = await call({ type: "handshake", version: 4 });
  console.log("HANDSHAKE", JSON.stringify(hs).slice(0, 120));
  const before = await call({ type: "describeSession", sessionId });
  console.log("BEFORE", JSON.stringify(before).slice(0, 220));
  const res = await call({ type: "close", sessionId });
  console.log("CLOSE", JSON.stringify(res).slice(0, 300));
  const after = await call({ type: "describeSession", sessionId });
  console.log("AFTER", JSON.stringify(after).slice(0, 300));
  process.exit(0);
});
