// Headless Windows daemon E2E: proves the daemon -> ConPTY input path on a real
// Windows host without any GUI. Run with: bun script/qa/win-daemon-e2e.mjs
//
// PASS = handshakeOk && spawnOk && writeOk && describeSession.endSequence advances
//        (the echoed command bytes came back through ConPTY).
//
// Requires a running daemon: ferryx.exe --daemon  (prints FERRYX_DAEMON_READY)
//
// Windows launcher tips (maho-win verified 2026-08-29):
// - Linking can leave target\debug\ferryx.exe intermittently removed by AV
//   heuristics; if launch fails with "file not found", copy the exe to a new
//   name (e.g. ferryx-run.exe) and run that.
// - Delete stale %LOCALAPPDATA%\Ferryx\runtime\daemon.lock before starting;
//   a stale lock makes the new daemon exit instantly.

import { readFileSync } from "node:fs";
import { join } from "node:path";

const REPO_ROOT = process.argv[2] ?? "C:\\Users\\sook\\ferryx-winbuild\\orca-lite";

const runtimeDir = join(
  process.env.LOCALAPPDATA ?? "C:\\ProgramData",
  "Ferryx",
  "runtime",
);
const portFile = join(runtimeDir, "daemon.port");
const port = Number(readFileSync(portFile, "utf8").trim());
if (!Number.isInteger(port) || port <= 0) {
  console.error(`FAIL: bad port file ${portFile}: ${port}`);
  process.exit(1);
}
console.log(`INFO: daemon port ${port} (from ${portFile})`);

const sock = await Bun.connect({
  hostname: "127.0.0.1",
  port,
  socket: {
    data(_socket, chunk) {
      buffer += chunk.toString("utf8");
      drain();
    },
    error(_socket, err) {
      console.error("SOCKET ERROR:", err);
      process.exit(1);
    },
  },
});

let buffer = "";
let pending = [];
function drain() {
  let idx;
  while ((idx = buffer.indexOf("\n")) !== -1) {
    const line = buffer.slice(0, idx);
    buffer = buffer.slice(idx + 1);
    if (!line.trim()) continue;
    const resolve = pending.shift();
    if (resolve) resolve(JSON.parse(line));
  }
}
function request(obj) {
  sock.write(JSON.stringify(obj) + "\n");
  return new Promise((resolve, reject) => {
    pending.push(resolve);
    setTimeout(() => reject(new Error(`timeout waiting response for ${obj.type}`)), 15000);
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  const hs = await request({ type: "handshake", version: 2 });
  console.log("handshake:", JSON.stringify(hs));
  if (hs.type !== "handshakeOk") throw new Error("handshake failed");

  const rw = await request({
    type: "registerWorkspace",
    workspaceId: "e2e-ws",
    repoRoot: REPO_ROOT,
  });
  console.log("registerWorkspace:", JSON.stringify(rw));

  const sp = await request({
    type: "spawn",
    clientRequestId: "e2e-1",
    workspaceId: "e2e-ws",
    cwd: REPO_ROOT,
    cols: 80,
    rows: 24,
  });
  console.log("spawn:", JSON.stringify(sp));
  const sessionId = sp.sessionId ?? sp.session_id;
  if (!sessionId) throw new Error("spawn returned no session id");

  const before = await request({ type: "describeSession", sessionId });
  console.log("describe before:", JSON.stringify(before).slice(0, 300));
  const sessBefore = before.session ?? before;

  const payload = Buffer.from("echo ferryx-e2e\r").toString("base64");
  const wr = await request({ type: "write", sessionId, data: payload });
  console.log("write:", JSON.stringify(wr));
  if (wr.type === "error" || wr.writeOk === false) throw new Error("write rejected");

  await sleep(2500);

  const after = await request({ type: "describeSession", sessionId });
  console.log("describe after:", JSON.stringify(after).slice(0, 300));
  const sessAfter = after.session ?? after;
  const endBefore = Number(sessBefore.endSequence ?? sessBefore.end_sequence ?? 0);
  const endAfter = Number(sessAfter.endSequence ?? sessAfter.end_sequence ?? 0);
  console.log(`endSequence: ${endBefore} -> ${endAfter}`);
  if (!(endAfter > endBefore)) throw new Error("endSequence did not advance (no ConPTY echo)");

  console.log(`E2E PASS: typed bytes reached ConPTY and echo advanced the ring (${endBefore} -> ${endAfter})`);
}

try {
  await main();
  process.exit(0);
} catch (err) {
  console.error("E2E FAIL:", err.message);
  process.exit(1);
}
