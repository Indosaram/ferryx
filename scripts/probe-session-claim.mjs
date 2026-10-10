// Probe the live daemon for the session holding the agent claim.
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
const TARGET = process.argv[2] ?? "4937f6c02-194c-4b8f-ac35-237e0b5e469b";

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
  console.log("HANDSHAKE", JSON.stringify(hs).slice(0, 200));
  const listed = await call({ type: "listSessions" });
  const sessions = listed.sessions ?? listed.ids ?? listed;
  console.log("LIST_KEYS", Object.keys(listed).join(","));
  const arr = Array.isArray(sessions) ? sessions : [];
  console.log("SESSION_COUNT", arr.length);
  for (const s of arr) {
    const id = typeof s === "string" ? s : s.sessionId ?? s.id;
    if (!id) continue;
    const d = await call({ type: "describeSession", sessionId: id });
    const detail = d.detail ?? d;
    const json = JSON.stringify(detail);
    const hit = id === TARGET || json.includes(TARGET) ? " <<< TARGET/CLAIM-HIT" : "";
    console.log(`SESSION ${id} ${json.slice(0, 400)}${hit}`);
  }
  process.exit(0);
});
