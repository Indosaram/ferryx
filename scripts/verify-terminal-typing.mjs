import net from "node:net";
import fs from "node:fs";
import readline from "node:readline";

const SOCKET_PATH = `/tmp/rorca-${process.getuid()}/daemon.sock`;
const TRACE_PATH = process.env.FERRYX_TRACE_PATH ?? "/tmp/ferryx-switch-debug.jsonl";
const BASELINE_PATH = process.env.FERRYX_BASELINE_PATH ?? "/tmp/ulw-c4-baseline.json";

function readDaemonSessions() {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection({ path: SOCKET_PATH });
    const lines = readline.createInterface({ input: socket });
    const pending = [];
    lines.on("line", (line) => {
      const resolveNext = pending.shift();
      if (resolveNext) resolveNext(JSON.parse(line));
    });
    socket.on("error", reject);
    const call = (payload) =>
      new Promise((res) => {
        pending.push(res);
        socket.write(`${JSON.stringify(payload)}\n`);
      });
    socket.on("connect", async () => {
      try {
        await call({ type: "handshake", version: 2 });
        const listed = await call({ type: "listSessions" });
        const sessions = {};
        for (const sessionId of listed.sessions ?? []) {
          const described = await call({ type: "describeSession", sessionId });
          const session = described.session ?? described ?? {};
          sessions[sessionId] = {
            endSequence: session.endSequence ?? null,
            cwd: session.cwd ?? null,
          };
        }
        socket.end();
        resolve(sessions);
      } catch (error) {
        socket.end();
        reject(error);
      }
    });
  });
}

function readLatestRun() {
  const raw = fs.readFileSync(TRACE_PATH, "utf8");
  const events = [];
  for (const line of raw.split("\n")) {
    if (!line.startsWith("{")) continue;
    try {
      events.push(JSON.parse(line));
    } catch {
      continue;
    }
  }
  const latestRunId = events.length ? events[events.length - 1].runId : null;
  const scoped = events.filter((event) => event.runId === latestRunId);
  const counts = {};
  const activeTabIds = new Set();
  const inputSessions = new Set();
  for (const event of scoped) {
    const name = event.event ?? "unknown";
    counts[name] = (counts[name] ?? 0) + 1;
    const activeTabId = event.details?.activeTabId;
    if (activeTabId) activeTabIds.add(activeTabId);
    if (name.startsWith("terminal.surface.input")) {
      const sessionId = event.details?.sessionId ?? event.details?.backendSessionId;
      if (sessionId) inputSessions.add(sessionId);
    }
  }
  return { latestRunId, counts, activeTabIds: [...activeTabIds], inputSessions: [...inputSessions] };
}

function countOf(counts, name) {
  return counts[name] ?? 0;
}

function diagnose(counts, advanced) {
  const capture = countOf(counts, "terminal.surface.input.capture");
  const sent = countOf(counts, "terminal.surface.input.sent");
  const dropped = countOf(counts, "terminal.surface.input.dropped");
  const recovering = countOf(counts, "terminal.surface.input.error.recovering");
  const attachError = countOf(counts, "terminal.surface.attach.error");
  if (sent > 0 && advanced.length > 0) return ["PASS", "input reached the PTY and the ring advanced"];
  if (sent > 0 && advanced.length === 0)
    return ["FAIL", "send_input succeeded but no ring growth: daemon or PTY side"];
  if (dropped > 0)
    return ["FAIL", `input dropped ${dropped}x: pane had no attached backend session (attach errors: ${attachError})`];
  if (recovering > 0)
    return ["FAIL", "IPC send path failing: check input.recover.failed / input.retry.failed"];
  if (capture > 0)
    return ["FAIL", "capture handler ran but nothing was sent: inspect input.dropped reason field"];
  return ["PENDING", "no terminal.surface.input.* events in the latest run: no keystroke reached the webview yet"];
}

const sessions = await readDaemonSessions();

if (process.argv.includes("--save-baseline")) {
  fs.writeFileSync(BASELINE_PATH, JSON.stringify(sessions));
  console.log(`baseline saved to ${BASELINE_PATH}`);
  process.exit(0);
}

const baseline = fs.existsSync(BASELINE_PATH)
  ? JSON.parse(fs.readFileSync(BASELINE_PATH, "utf8"))
  : {};
const advanced = [];
console.log("=== daemon ring vs baseline ===");
for (const [sessionId, info] of Object.entries(sessions)) {
  const before = baseline[sessionId]?.endSequence ?? null;
  const delta = before === null ? null : info.endSequence - before;
  if (delta !== null && delta > 0) advanced.push({ sessionId, delta });
  const label = before === null ? "NEW-SESSION" : delta > 0 ? `ADVANCED +${delta}` : "same";
  console.log(`  ${sessionId.slice(0, 8)} end=${info.endSequence} base=${before} ${label} ${info.cwd ?? ""}`);
}

const { latestRunId, counts, activeTabIds, inputSessions } = readLatestRun();
console.log(`\n=== latest trace run ${String(latestRunId).slice(0, 8)} ===`);
for (const name of [
  "terminal.surface.input.capture",
  "terminal.surface.input.sent",
  "terminal.surface.input.dropped",
  "terminal.surface.input.error.recovering",
  "terminal.surface.attach.error",
  "worktree.ensure.skipped",
  "worktree.ensure.activated",
]) {
  console.log(`  ${countOf(counts, name)}  ${name}`);
}
console.log(`  active tab ids seen: ${activeTabIds.length ? activeTabIds.join(", ") : "none"}`);
console.log(`  sessions with input events: ${inputSessions.length ? inputSessions.join(", ") : "none"}`);

const [verdict, reason] = diagnose(counts, advanced);
console.log(`\nVERDICT: ${verdict} - ${reason}`);
if (activeTabIds.length > 1) {
  console.log(`WARNING: more than one active tab id observed in this run: ${activeTabIds.join(", ")}`);
}
process.exit(verdict === "PASS" ? 0 : verdict === "PENDING" ? 2 : 1);
