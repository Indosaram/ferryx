import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { mkdtemp, mkdir, readFile, readdir, rm } from "node:fs/promises";
import { watch } from "node:fs";
import net from "node:net";
import path from "node:path";
import { createInterface } from "node:readline";

const binary = path.resolve(process.argv[2] ?? "src-tauri/target/debug/ferryx");
const initialBinary = path.resolve(process.argv[3] ?? binary);
const noIntermediateSession = process.argv.includes("--no-intermediate-session");
const verifyAgentState = process.argv.includes("--agent-state");
const root = await mkdtemp("/tmp/fx-handover-");
const runtime = path.join(root, "runtime");
const canonical = path.join(runtime, "daemon.sock");
const repository = path.join(root, "repository");
const connections = new Set();
const ownedPids = new Set();
const sessions = [];
let stderr = "";
let initial;

function bounded(promise, label) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`Timed out: ${label}\n${stderr}`)), 10000);
    }),
  ]).finally(() => clearTimeout(timer));
}

async function connect(socketPath) {
  assert.equal(path.dirname(socketPath), runtime, "only isolated QA sockets are allowed");
  const socket = net.createConnection(socketPath);
  connections.add(socket);
  const reader = createInterface({ input: socket });
  const queue = [];
  const pending = [];
  const agentStates = [];
  const agentWaiters = [];
  let closed = false;
  let output = "";
  const outputWaiters = new Set();
  reader.on("line", (line) => {
    const message = JSON.parse(line);
    if (message.type === "agentState") {
      const waiter = agentWaiters.shift();
      if (waiter) waiter.resolve(message);
      else agentStates.push(message);
      return;
    }
    if (message.type === "output") {
      output += Buffer.from(message.data, "base64").toString();
      for (const waiter of outputWaiters) {
        if (output.includes(waiter.marker)) {
          outputWaiters.delete(waiter);
          waiter.resolve(output);
        }
      }
      return;
    }
    const next = pending.shift();
    if (next) next.resolve(message);
    else queue.push(message);
  });
  const fail = (error) => {
    closed = true;
    for (const waiter of pending.splice(0)) waiter.reject(error);
    for (const waiter of agentWaiters.splice(0)) waiter.reject(error);
    for (const waiter of outputWaiters) waiter.reject(error);
    outputWaiters.clear();
  };
  socket.on("error", fail);
  reader.on("error", fail);
  socket.on("close", () => {
    connections.delete(socket);
    fail(new Error(`Connection closed: ${socketPath}`));
  });
  const next = () => bounded(new Promise((resolve, reject) => {
    if (queue.length) resolve(queue.shift());
    else if (closed) reject(new Error(`Closed: ${socketPath}`));
    else pending.push({ resolve, reject });
  }), "daemon response");
  const call = async (request) => {
    const response = next();
    socket.write(`${JSON.stringify(request)}\n`);
    return response;
  };
  const handshake = await call({ type: "handshake", version: 3 });
  assert.equal(handshake.type, "handshakeOk", JSON.stringify(handshake));
  ownedPids.add(handshake.pid);
  return {
    call,
    handshake,
    close: () => socket.destroy(),
    nextAgentState() {
      return bounded(new Promise((resolve, reject) => {
        if (agentStates.length) resolve(agentStates.shift());
        else if (closed) reject(new Error(`Closed: ${socketPath}`));
        else agentWaiters.push({ resolve, reject });
      }), "agent state through canonical attach");
    },
    waitForOutput(marker) {
      if (output.includes(marker)) return Promise.resolve(output);
      return bounded(new Promise((resolve, reject) => {
        outputWaiters.add({ marker, resolve, reject });
      }), `PTY output ${marker}`);
    },
  };
}

async function reportAgentStates(sessionId, states) {
  const socket = net.createConnection(path.join(runtime, "agent-state.sock"));
  connections.add(socket);
  try {
    await bounded(new Promise((resolve, reject) => {
      socket.once("error", reject);
      socket.once("connect", () => {
        const payload = states.map((state) => JSON.stringify({
          type: "agentState", sessionId, state, agent: "omo",
        })).join("\n") + "\n";
        socket.end(payload, resolve);
      });
    }), "isolated agent state report");
  } finally {
    connections.delete(socket);
    socket.destroy();
  }
}

async function verifyAgentDelivery(attached, sessionId) {
  const working = attached.nextAgentState();
  await reportAgentStates(sessionId, ["working"]);
  let started = await working;
  if (started.isSnapshot) started = await attached.nextAgentState();
  assert.equal(started.sessionId, sessionId);
  assert.equal(started.state, "working");
  assert.notEqual(started.isSnapshot, true);

  const transitions = Promise.all([attached.nextAgentState(), attached.nextAgentState()]);
  await reportAgentStates(sessionId, ["blocked", "idle"]);
  const [blocked, idle] = await transitions;
  assert.equal(blocked.state, "blocked", "burst must preserve intermediate attention");
  assert.equal(idle.state, "idle", "burst must preserve completion order");

  const reattached = await connect(canonical);
  assert.equal((await reattached.call({
    type: "attach", sessionId, afterSequence: 0,
  })).type, "attachOk");
  const restored = await reattached.nextAgentState();
  assert.equal(restored.sessionId, sessionId);
  assert.equal(restored.state, "idle");
  assert.equal(restored.isSnapshot, true, "reattach restores quietly instead of fabricating a live edge");
  reattached.close();
}

function nextCanonical(previousEpoch) {
  let watcher;
  let settled = false;
  const ready = new Promise((resolve, reject) => {
    watcher = watch(runtime, async (_event, name) => {
      if (name !== "daemon.sock" || settled) return;
      try {
        const client = await connect(canonical);
        if (!settled && client.handshake.epoch !== previousEpoch) {
          settled = true;
          resolve(client);
        } else client.close();
      } catch (error) {
        if (!["ENOENT", "ECONNREFUSED"].includes(error.code)) reject(error);
      }
    });
    watcher.on("error", reject);
  });
  return bounded(ready, "replacement canonical daemon").finally(() => watcher.close());
}

try {
  await mkdir(runtime, { mode: 0o700 });
  await mkdir(repository);
  await mkdir(path.join(root, "home"));
  execFileSync("git", ["init", "-q", repository]);
  initial = spawn(initialBinary, ["--daemon"], {
    env: {
      ...process.env,
      HOME: path.join(root, "home"),
      FERRYX_RUNTIME_DIR: runtime,
      FERRYX_DATA_DIR: path.join(root, "data"),
      FERRYX_SESSION_DIR: path.join(root, "sessions"),
      XDG_CONFIG_HOME: path.join(root, "config"),
      XDG_DATA_HOME: path.join(root, "data"),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  ownedPids.add(initial.pid);
  console.log(`QA initial PID ${initial.pid}; runtime ${runtime}`);
  initial.stderr.on("data", (chunk) => { stderr += chunk; });
  await bounded(new Promise((resolve, reject) => {
    const reader = createInterface({ input: initial.stdout });
    initial.once("error", reject);
    initial.once("exit", (code) => reject(new Error(`Initial daemon exited ${code}\n${stderr}`)));
    reader.on("line", (line) => {
      if (line === "FERRYX_DAEMON_READY") {
        reader.close();
        initial.stdout.destroy();
        resolve();
      }
    });
  }), "initial readiness");
  let control = await connect(canonical);
  console.log(`QA initial version ${control.handshake.daemonVersion}`);

  for (let generation = 0; generation < 2; generation++) {
    assert.equal((await control.call({
      type: "registerWorkspace", workspaceId: "handover-qa", repoRoot: repository,
    })).type, "registerWorkspaceOk");
    if (generation === 0 || !noIntermediateSession) {
      const created = await control.call({
        type: "spawn",
        clientRequestId: `generation-${generation}`,
        workspaceId: "handover-qa",
        cwd: repository,
        cols: 80,
        rows: 24,
        shell: "/bin/sh",
      });
      assert.equal(created.type, "spawnOk", JSON.stringify(created));
      sessions.push(created.sessionId);
      const before = await connect(canonical);
      assert.equal((await before.call({
        type: "attach", sessionId: created.sessionId, afterSequence: 0,
      })).type, "attachOk");
      const seeded = before.waitForOutput(`BEFORE_${created.sessionId}`);
      assert.equal((await control.call({
        type: "write",
        sessionId: created.sessionId,
        data: Buffer.from(`printf '%s%s\\n' 'BEFORE_' '${created.sessionId}'\n`).toString("base64"),
      })).type, "writeOk");
      await seeded;
      if (verifyAgentState && initialBinary === binary) {
        await verifyAgentDelivery(before, created.sessionId);
      }
      before.close();
    }

    const replacement = nextCanonical(control.handshake.epoch);
    const upgrade = await control.call({ type: "upgradeBinary", newBinaryPath: binary });
    assert.equal(upgrade.type, "upgradeScheduled", JSON.stringify(upgrade));
    control.close();
    control = await replacement;
    const manifest = JSON.parse(await readFile(path.join(runtime, "handover_routes.json"), "utf8"));
    const listed = await control.call({ type: "listSessions" });
    assert.equal(listed.type, "listSessionsOk", JSON.stringify(listed));
    for (const sessionId of sessions) {
      assert.ok(listed.sessions.includes(sessionId), `missing original PTY ${sessionId}`);
      assert.ok(manifest.routes.some((route) => route.sessions.includes(sessionId)),
        `original PTY ${sessionId} has no durable legacy route`);
      const attached = await connect(canonical);
      const snapshot = await attached.call({ type: "attach", sessionId, afterSequence: 0 });
      assert.equal(snapshot.type, "attachOk", JSON.stringify(snapshot));
      assert.ok(
        Buffer.from(snapshot.history, "base64").toString().includes(`BEFORE_${sessionId}`),
        `pre-handover output missing for ${sessionId}`,
      );
      const marker = `HANDOVER_OK_${generation}_${sessionId}`;
      const output = attached.waitForOutput(marker);
      const written = await control.call({
        type: "write",
        sessionId,
        data: Buffer.from(`printf '%s%s\\n' 'HANDOVER_' 'OK_${generation}_${sessionId}'\n`).toString("base64"),
      });
      assert.equal(written.type, "writeOk", JSON.stringify(written));
      const received = await output;
      assert.equal(received.split(marker).length - 1, 1, "one executed printf result");
      assert.equal((await control.call({
        type: "resize", sessionId, cols: 100, rows: 30,
      })).type, "resizeOk");
      const described = await control.call({ type: "describeSession", sessionId });
      assert.equal(described.type, "describeSessionOk");
      assert.equal(described.session.cols, 100);
      assert.equal(described.session.rows, 30);
      if (verifyAgentState) await verifyAgentDelivery(attached, sessionId);
      attached.close();
    }
    console.log(`PASS generation ${generation + 1}: ${sessions.length} original PTYs; history, executed input, resize preserved`);
    if (verifyAgentState) console.log(`PASS agent states generation ${generation + 1}: live ordered edges and quiet attach snapshots`);
  }
  control.close();
} finally {
  for (const socket of connections) socket.destroy();
  for (const name of await readdir(runtime).catch(() => [])) {
    if (name !== "daemon.sock" && !(name.startsWith("legacy-") && name.endsWith(".sock"))) continue;
    try {
      const client = await connect(path.join(runtime, name));
      await client.call({ type: "shutdown" }).catch(() => {});
      client.close();
    } catch {}
  }
  for (const pid of ownedPids) {
    try { process.kill(pid, "SIGTERM"); } catch (error) {
      if (error.code !== "ESRCH") throw error;
    }
  }
  for (const socket of connections) socket.destroy();
  initial?.stderr.destroy();
  await rm(root, { recursive: true, force: true });
  console.log(`CLEANUP isolated runtime removed: ${root}`);
}
