// Attach (read-only) to a session's output ring and print the tail.
import net from "node:net";
import readline from "node:readline";

const SOCKET_PATH = `/tmp/rorca-${process.getuid()}/daemon.sock`;
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
