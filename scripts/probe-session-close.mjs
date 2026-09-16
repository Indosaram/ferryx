// Close a live backend session via the daemon UDS protocol (v4).
import net from "node:net";
import readline from "node:readline";

const SOCKET_PATH = `/tmp/rorca-${process.getuid()}/daemon.sock`;
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
