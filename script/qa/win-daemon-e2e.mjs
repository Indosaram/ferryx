// Headless Windows daemon E2E: proves daemon -> ConPTY input/output path
// and verified PTY shell working directory on a real Windows host.
//
// PASS = handshakeOk && errorProbeOk && spawnOk && writeOk && outputMarkerOk && cwdOk
// Run: bun script/qa/win-daemon-e2e.mjs [repoRoot]
// Self-test: bun script/qa/win-daemon-e2e.mjs --self-test

import { readFileSync } from "node:fs";
import { createConnection, createServer } from "node:net";
import assert from "node:assert/strict";
import { join } from "node:path";

export function normalizePath(p) {
  if (typeof p !== "string") return "";
  let s = p.trim().replace(/^([/\\]{2}[?.][/\\])/, "").replace(/\\/g, "/");
  s = s.replace(/^([a-zA-Z]):/, (_, d) => `${d.toLowerCase()}:`);
  while (s.length > 3 && s.endsWith("/")) s = s.slice(0, -1);
  return s.toLowerCase();
}

export function pathsMatch(a, b) {
  return normalizePath(a) === normalizePath(b);
}

export function parseRepoRoot(args, portIndex, portFileIndex) {
  return args.find((arg, index) =>
    !arg.startsWith("-") &&
    !(portIndex >= 0 && index === portIndex + 1) &&
    !(portFileIndex >= 0 && index === portFileIndex + 1)
  ) ?? "C:\\Users\\sook\\ferryx-winbuild\\orca-lite";
}

export function parsePtyOutput(text) {
  const lines = text
    .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "")
    .split(/\r?\n/)
    .map((line) => line.trim());
  const marker = lines
    .map((line) => line.match(/^FERRYX_MARKER=([a-zA-Z0-9_-]+)$/)?.[1])
    .findLast(Boolean) ?? null;
  const cwd = lines
    .map((line) => line.match(/^FERRYX_CWD=((?:[a-zA-Z]:[\\/]|\\\\)[^\r\n]+)$/)?.[1])
    .findLast(Boolean) ?? null;
  return { marker, cwd };
}

const deadlines = { setTimeout, clearTimeout };
const PROTOCOL_VERSION = 3;
// DaemonStreamMessage plus the separate DaemonRemoteEvent envelope.
const streamTypes = new Set(["output", "gap", "replayGap", "agentState", "remoteStatus", "exit"]);

function connectClient(port, clock = deadlines, dial = createConnection) {
  return new Promise((resolve, reject) => {
    const socket = dial({ port, host: "127.0.0.1" });
    let buffer = "";
    const pending = [], listeners = new Set(), closeListeners = new Set();
    let terminalError = null;
    const deliver = (subscribers, value) => {
      for (const listener of subscribers) {
        try { listener(value); }
        catch (error) { console.error("Daemon QA listener failed:", error.message); }
      }
    };
    const terminate = (error) => {
      if (terminalError) return;
      terminalError = error;
      while (pending.length) pending.shift().reject(error);
      deliver(closeListeners, error);
      listeners.clear();
      closeListeners.clear();
      socket.destroy();
      reject(error);
    };

    socket.setEncoding("utf8");
    socket.on("data", (chunk) => {
      buffer += chunk;
      let idx;
      while ((idx = buffer.indexOf("\n")) !== -1) {
        const line = buffer.slice(0, idx).trim();
        buffer = buffer.slice(idx + 1);
        if (!line) continue;
        if (terminalError) return;
        let parsed;
        try { parsed = JSON.parse(line); }
        catch (error) { terminate(error); return; }
        if (!parsed || typeof parsed !== "object") {
          terminate(new Error("Invalid daemon frame"));
          return;
        }
        // Stream frames never consume the uncorrelated response FIFO.
        if (!streamTypes.has(parsed.type) && typeof parsed.event !== "string") {
          pending.shift()?.resolve(parsed);
        }
        deliver(listeners, parsed);
      }
    });
    socket.on("error", terminate);
    socket.on("end", () => terminate(new Error("Daemon connection ended")));
    socket.on("close", () => terminate(new Error("Daemon connection closed")));
    socket.on("connect", () => resolve({
      request(obj, timeoutMs = 15000) {
        if (terminalError) return Promise.reject(terminalError);
        return new Promise((res, rej) => {
          const timer = clock.setTimeout(() => {
            // Without response IDs a late reply makes every later FIFO entry ambiguous.
            terminate(new Error(`Timeout waiting response for ${obj.type} (${timeoutMs}ms)`));
          }, timeoutMs);
          pending.push({
            resolve: (v) => { clock.clearTimeout(timer); res(v); },
            reject: (e) => { clock.clearTimeout(timer); rej(e); },
          });
          socket.write(`${JSON.stringify(obj)}\n`);
        });
      },
      onMessage: (fn) => (listeners.add(fn), () => listeners.delete(fn)),
      onClose(fn) {
        if (terminalError) fn(terminalError);
        else closeListeners.add(fn);
        return () => closeListeners.delete(fn);
      },
      close: () => terminate(new Error("Daemon client explicitly closed")),
    }));
  });
}

async function attachStream(port, sessionId, timeoutMs = 15000, clock = deadlines, dial = createConnection) {
  const client = await connectClient(port, clock, dial);
  try {
      let decoded = "";
      let terminalError = null;
      const waiters = [];
      client.onClose((error) => {
        terminalError = error;
        for (const waiter of waiters.splice(0)) {
          clock.clearTimeout(waiter.timer);
          waiter.reject(error);
        }
      });

      const notify = () => {
        const parsed = parsePtyOutput(decoded);
        for (let i = waiters.length - 1; i >= 0; i--) {
          if (parsed.marker === waiters[i].marker && parsed.cwd) {
            clock.clearTimeout(waiters[i].timer);
            waiters.splice(i, 1)[0].resolve({ decoded, parsed });
          }
        }
      };

      client.onMessage((msg) => {
        if (msg.type === "attachOk" && msg.history) {
          decoded += Buffer.from(msg.history, "base64").toString("utf8");
          notify();
        } else if (msg.type === "output" && msg.data) {
          decoded += Buffer.from(msg.data, "base64").toString("utf8");
          notify();
        }
      });

      const hs = await client.request({ type: "handshake", version: PROTOCOL_VERSION }, timeoutMs);
      if (hs.type !== "handshakeOk") throw new Error(`Stream handshake failed: ${JSON.stringify(hs)}`);
      const att = await client.request({ type: "attach", sessionId, afterSequence: null }, timeoutMs);
      if (att.type !== "attachOk") throw new Error(`Stream attach failed: ${JSON.stringify(att)}`);

      return {
        waitForPtySignal(marker, waitTimeoutMs = timeoutMs) {
          if (terminalError) return Promise.reject(terminalError);
          const parsed = parsePtyOutput(decoded);
          if (parsed.marker === marker && parsed.cwd) return Promise.resolve({ decoded, parsed });
          return new Promise((res, rej) => {
            const timer = clock.setTimeout(() => {
              const idx = waiters.findIndex((w) => w.resolve === res);
              if (idx !== -1) waiters.splice(idx, 1);
              rej(new Error(`Timeout waiting marker "${marker}". Decoded: ${JSON.stringify(decoded.slice(-200))}`));
            }, waitTimeoutMs);
            waiters.push({ marker, resolve: res, reject: rej, timer });
          });
        },
        close: () => client.close(),
      };
  } catch (error) {
    client.close();
    throw error;
  }
}

export function runPureSelfTest() {
  const n1 = normalizePath("\\\\?\\C:\\Users\\sook\\ferryx-winbuild\\orca-lite");
  const n2 = normalizePath("c:/users/sook/ferryx-winbuild/orca-lite/");
  if (n1 !== "c:/users/sook/ferryx-winbuild/orca-lite" || n1 !== n2) throw new Error("normalizePath mismatch");
  if (!pathsMatch("\\\\?\\C:\\foo\\bar", "c:/foo/bar")) throw new Error("pathsMatch mismatch");
  if (parseRepoRoot(["C:\\Users\\sook\\ferryx-ulw-01a04fcf"], -1, -1) !== "C:\\Users\\sook\\ferryx-ulw-01a04fcf") {
    throw new Error("parseRepoRoot positional argument mismatch");
  }
  const parsed = parsePtyOutput(
    'PS > Write-Output "FERRYX_MARKER=sig-42"; Write-Output "FERRYX_CWD=$((Get-Location).Path)"\r\n' +
    "FERRYX_MARKER=sig-42\r\nFERRYX_CWD=C:\\Users\\sook\r\nPS > ",
  );
  if (parsed.marker !== "sig-42" || parsed.cwd !== "C:\\Users\\sook") throw new Error("parsePtyOutput mismatch");
  console.log("P22 PARSER: pure normalization and PTY signal parser verified (transport not yet qualified).");
}

function controlHandshake(client) {
  return client.request({ type: "handshake", version: PROTOCOL_VERSION });
}

async function closeOwnedSession(controlClient, streamClient, sessionId) {
  try {
    if (sessionId && controlClient) {
      const response = await controlClient.request({ type: "close", sessionId }, 3000);
      if (response.type !== "closeOk") {
        throw new Error(`Session cleanup failed: ${JSON.stringify(response)}`);
      }
    }
  } finally {
    if (streamClient) streamClient.close();
    if (controlClient) controlClient.close();
  }
}

// Only this branch owns listeners on ephemeral loopback ports. Deadlines are
// injected, but every byte still traverses a real TCP socket and JSON parser.
async function runTransportSelfTest() {
  runPureSelfTest();
  const rust = readFileSync(new URL("../../src-tauri/src/daemon/protocol.rs", import.meta.url), "utf8");
  const version = Number(rust.match(/pub const DAEMON_PROTOCOL_VERSION: u32 = (\d+);/)?.[1]);
  assert.ok(version > 0, "Rust protocol constant found");
  const failures = [];
  let cases = 0;
  const signal = () => {
    let resolve;
    const promise = new Promise((r) => { resolve = r; });
    return { promise, resolve };
  };
  const bounded = async (promise, label) => {
    let timer;
    try {
      return await Promise.race([promise, new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`Barrier missing: ${label}`)), 1000);
      })]);
    } finally { clearTimeout(timer); }
  };
  const track = (promise) => {
    const state = { resolved: 0, rejected: 0, value: null };
    state.done = promise.then((value) => { state.resolved++; state.value = value; },
      (error) => { state.rejected++; state.value = error; });
    return state;
  };
  async function scenario(name, body) {
    const timers = new Map();
    let nextTimer = 0;
    const clock = {
      setTimeout(fn) { timers.set(++nextTimer, fn); return nextTimer; },
      clearTimeout(id) { timers.delete(id); },
      fire() { const [id, fn] = timers.entries().next().value; timers.delete(id); fn(); },
    };
    const sockets = new Set(), closes = [], received = [], waiting = [];
    let peer, local, receipts = 0;
    const server = createServer((socket) => {
      peer = socket;
      own(socket);
      let buffer = "";
      socket.setEncoding("utf8");
      socket.on("data", (chunk) => {
        buffer += chunk;
        let index;
        while ((index = buffer.indexOf("\n")) >= 0) {
          const message = JSON.parse(buffer.slice(0, index));
          buffer = buffer.slice(index + 1);
          receipts++;
          if (waiting.length) waiting.shift()(message); else received.push(message);
        }
      });
    });
    function own(socket) {
      sockets.add(socket);
      const closed = signal();
      closes.push(closed.promise);
      socket.on("error", (error) => console.log(`P22 SOCKET ${name}: ${error.message}`));
      socket.once("close", () => { sockets.delete(socket); closed.resolve(); });
    }
    const dial = (options) => { local = createConnection(options); own(local); return local; };
    const ready = signal();
    server.listen(0, "127.0.0.1", ready.resolve);
    await bounded(ready.promise, "listen");
    const port = server.address().port;
    const receive = () => bounded(received.length ? Promise.resolve(received.shift()) :
      new Promise((resolve) => waiting.push(resolve)), "request receipt");
    const send = (...messages) => peer.write(messages.map((m) => JSON.stringify(m) + "\n").join(""));
    const handshakeOk = { type: "handshakeOk", version, pid: 123, epoch: 1 };
    const attachOk = { type: "attachOk", epoch: 1, sessionId: "owned", startSequence: 1, endSequence: 1, gap: null, history: "" };
    const stream = async () => {
      const opening = attachStream(port, "owned", 15000, clock, dial);
      await receive(); send(handshakeOk);
      await receive(); send(attachOk);
      return bounded(opening, "stream ready");
    };
    try {
      await body({ clock, timers, port, dial, receive, send, stream, handshakeOk, attachOk,
        local: () => local, peer: () => peer, closes });
      assert.equal(timers.size, 0, "no residual deadlines");
      assert.ok(receipts > 0, "nonzero real peer receipts");
      cases++;
      console.log(`P22 GREEN ${name}: cases=1 receipts=${receipts} timers=0`);
    } catch (error) {
      failures.push(name);
      console.error(`P22 RED ${name}: ${error.message}`);
    } finally {
      for (const socket of sockets) socket.destroy();
      await bounded(Promise.all(closes), "owned socket cleanup");
      await bounded(new Promise((resolve, reject) => server.close((e) => e ? reject(e) : resolve())), "listener cleanup");
      // A failing-first helper may leak virtual deadlines. Report before fixture disposal.
      const leakedTimers = timers.size;
      timers.clear();
      assert.equal(sockets.size, 0);
      console.log(`P22 CLEAN ${name}: sockets=0 listenerClosed=true residualTimers=${leakedTimers} disposedTimers=${leakedTimers}`);
    }
  }
  for (const throwing of [false, true]) await scenario(throwing ? "listener-isolation" : "event-response-fifo", async (p) => {
    const client = await connectClient(p.port, p.clock, p.dial);
    const seen = [], delivered = signal();
    if (throwing) client.onMessage(() => { throw new Error("owned-listener-sentinel"); });
    client.onMessage((m) => { seen.push(m.type); if (m.type === "writeOk") delivered.resolve(); });
    const a = track(client.request({ type: "write", sessionId: "owned", data: "QQ==" }));
    const b = track(client.request({ type: "ping" }));
    await p.receive(); await p.receive();
    p.send({ type: "output", sessionId: "owned", sequence: 1, data: "QQ==" }, { type: "writeOk" }, { type: "pong" });
    await bounded(Promise.all([a.done, b.done]), "FIFO responses");
    assert.equal(a.value?.type, "writeOk", "A resolves only its response despite output/listener throw");
    assert.equal(b.value?.type, "pong", "B resolves only its response");
    await bounded(delivered.promise, "later listener delivery");
    assert.deepEqual(seen, ["output", "writeOk", "pong"]);
    client.close();
  });
  await scenario("timeout-poisons-late-reply", async (p) => {
    const client = await connectClient(p.port, p.clock, p.dial);
    const a = track(client.request({ type: "ping" }));
    const queued = track(client.request({ type: "listSessions" }));
    await p.receive(); await p.receive();
    p.clock.fire();
    // Inject late A before the peer sees close; it must not become B's result.
    p.send({ type: "pong" });
    const b = track(client.request({ type: "describeSession", sessionId: "owned" }));
    await a.done;
    await Promise.resolve();
    assert.equal(queued.rejected, 1, "timeout settles queued request before late reply");
    assert.equal(b.rejected, 1, "poisoned connection rejects new request");
    await bounded(Promise.all(p.closes), "timeout closes both endpoints");
    assert.deepEqual([a.rejected, queued.rejected, b.rejected, a.resolved, queued.resolved, b.resolved], [1, 1, 1, 0, 0, 0]);
  });
  for (const mode of ["close", "eof", "error"]) {
    await scenario(`requests-${mode}`, async (p) => {
      const client = await connectClient(p.port, p.clock, p.dial);
      const a = track(client.request({ type: "ping" })), b = track(client.request({ type: "listSessions" }));
      await p.receive(); await p.receive();
      if (mode === "close") client.close();
      if (mode === "eof") p.peer().end();
      if (mode === "error") p.local().destroy(new Error("owned-socket-error"));
      await bounded(Promise.all(p.closes), "request endpoints closed");
      assert.deepEqual([a.rejected, b.rejected, a.resolved, b.resolved], [1, 1, 0, 0], "all requests settle once on terminal event");
      client.close();
    });
    await scenario(`pty-waiters-${mode}`, async (p) => {
      const client = await p.stream();
      const a = track(client.waitForPtySignal("never-a")), b = track(client.waitForPtySignal("never-b"));
      if (mode === "close") client.close();
      if (mode === "eof") p.peer().end();
      if (mode === "error") p.local().destroy(new Error("owned-stream-error"));
      await bounded(Promise.all(p.closes), "stream endpoints closed");
      assert.deepEqual([a.rejected, b.rejected, a.resolved, b.resolved], [1, 1, 0, 0], "all PTY waiters settle once on terminal event");
      const late = track(client.waitForPtySignal("after-close"));
      await Promise.resolve();
      assert.equal(late.rejected, 1, "post-close waiter rejected without deadline");
      client.close();
    });
  }
  for (const stage of ["handshake", "attach"]) await scenario(`failed-${stage}-cleanup`, async (p) => {
    const opening = track(attachStream(p.port, "owned", 15000, p.clock, p.dial));
    await p.receive();
    if (stage === "attach") { p.send(p.handshakeOk); await p.receive(); }
    p.send({ type: "error", message: `owned-${stage}-failure` });
    await bounded(opening.done, "opening rejected");
    assert.equal(opening.rejected, 1);
    assert.equal(p.local().destroyed, true, "failed opening closes acquired socket before rejection");
    await bounded(Promise.all(p.closes), "failed opening peer close");
  });
  for (const route of ["control", "stream"]) await scenario(`protocol-${route}`, async (p) => {
    const client = route === "control" ? await connectClient(p.port, p.clock, p.dial) : null;
    const result = track(client ? controlHandshake(client) : attachStream(p.port, "owned", 15000, p.clock, p.dial));
    const request = await p.receive();
    p.send(request.version === version ? p.handshakeOk : { type: "protocolMismatch", expectedVersion: version, receivedVersion: request.version });
    assert.equal(request.version, version, "actual handshake matches Rust protocol constant");
    if (!client) { assert.equal((await p.receive()).type, "attach"); p.send(p.attachOk); }
    await bounded(result.done, "strict protocol result");
    assert.equal(result.resolved, 1);
    (client ?? result.value).close();
  });
  for (const outcome of ["closeOk", "error", "eof"]) await scenario(`session-cleanup-${outcome}`, async (p) => {
    const streamClient = await p.stream();
    const controlClient = await connectClient(p.port, p.clock, p.dial);
    let controlCloses = 0, streamCloses = 0;
    const cleanup = track(closeOwnedSession(
      { request: (...args) => controlClient.request(...args), close: () => { controlCloses++; controlClient.close(); } },
      { close: () => { streamCloses++; streamClient.close(); } },
      "owned",
    ));
    assert.deepEqual(await p.receive(), { type: "close", sessionId: "owned" });
    if (outcome === "eof") p.peer().end();
    else p.send({ type: outcome, message: outcome === "error" ? "owned-close-denied" : undefined });
    await bounded(cleanup.done, "session cleanup settled");
    await bounded(Promise.all(p.closes), "session cleanup closes all endpoints");
    assert.equal(controlCloses, 1);
    assert.equal(streamCloses, 1);
    assert.equal(cleanup.rejected, outcome === "closeOk" ? 0 : 1, "session cleanup failure must reject");
    assert.equal(cleanup.resolved, outcome === "closeOk" ? 1 : 0);
  });
  assert.equal(failures.length, 0, `transport scenarios failed: ${failures.join(", ")}`);
  assert.equal(cases, 16, "all transport scenarios executed");
  console.log(`SELF-TEST PASS: transport scenarios=${cases}; pure parser retained; owned resources closed.`);
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes("--help") || args.includes("-h")) {
    console.log("Usage: bun script/qa/win-daemon-e2e.mjs [--self-test] [--port <num>] [--port-file <path>] [repoRoot]");
    return;
  }
  if (args.includes("--self-test")) return runTransportSelfTest();

  const pIdx = args.indexOf("--port"), pfIdx = args.indexOf("--port-file");
  const customPort = pIdx !== -1 ? Number(args[pIdx + 1]) : null;
  const customPortFile = pfIdx !== -1 ? args[pfIdx + 1] : null;
  const repoRoot = parseRepoRoot(args, pIdx, pfIdx);

  const portFile = customPortFile ?? join(process.env.LOCALAPPDATA ?? "C:\\ProgramData", "Ferryx", "runtime", "daemon.port");
  const port = customPort ?? Number(readFileSync(portFile, "utf8").trim());
  if (!Number.isInteger(port) || port <= 0) throw new Error(`Invalid daemon port: ${port}`);
  console.log(`INFO: target daemon port ${port}, repoRoot=${repoRoot}`);

  let controlClient = null, streamClient = null, sessionId = null;
  try {
    controlClient = await connectClient(port);
    const hs = await controlHandshake(controlClient);
    if (hs.type !== "handshakeOk") throw new Error("handshake failed");

    // Inline structured error probe
    const errProbe = await controlClient.request({ type: "describeSession", sessionId: `probe-bad-${Date.now()}` });
    const errorProbeOk = errProbe?.type === "error" && typeof errProbe?.message === "string";
    if (!errorProbeOk) throw new Error(`Expected structured error response, got ${JSON.stringify(errProbe)}`);

    await controlClient.request({ type: "registerWorkspace", workspaceId: "e2e-ws", repoRoot });
    const sp = await controlClient.request({
      type: "spawn", clientRequestId: `e2e-${Date.now()}`, workspaceId: "e2e-ws", cwd: repoRoot, cols: 80, rows: 24,
      shell: "powershell.exe",
    });
    sessionId = sp.sessionId ?? sp.session_id;
    if (sp.type !== "spawnOk" || !sessionId) throw new Error("spawn failed");

    // Subscribe/attach to output stream BEFORE writing
    streamClient = await attachStream(port, sessionId);
    const before = await controlClient.request({ type: "describeSession", sessionId });
    const endBefore = Number(before.session?.endSequence ?? before.end_sequence ?? 0);

    const uniqueMarker = `ferryx-win-e2e-${Date.now()}`;
    const ptySignalPromise = streamClient.waitForPtySignal(uniqueMarker, 15000);

    // Platform command emitting both unique marker and actual PTY CWD
    const cmd = `Write-Output "FERRYX_MARKER=${uniqueMarker}"; Write-Output "FERRYX_CWD=$((Get-Location).Path)"\r`;
    const wr = await controlClient.request({ type: "write", sessionId, data: Buffer.from(cmd).toString("base64") });
    if (wr.type !== "writeOk" && wr.writeOk !== true) throw new Error("write rejected");

    // Await streamed PTY output signal (event-driven, bounded timeout, no fixed sleep)
    const { parsed } = await ptySignalPromise;
    const after = await controlClient.request({ type: "describeSession", sessionId });
    const endAfter = Number(after.session?.endSequence ?? after.end_sequence ?? 0);

    const outputMarkerOk = parsed.marker === uniqueMarker;
    const cwdOk = Boolean(parsed.cwd && pathsMatch(parsed.cwd, repoRoot));

    const summary = {
      verdict: outputMarkerOk && cwdOk && endAfter > endBefore ? "PASS" : "FAIL",
      handshakeOk: true,
      errorProbeOk: true,
      spawnOk: true,
      writeOk: true,
      outputMarkerOk,
      cwdOk,
      sessionId,
      requestedRepoRoot: repoRoot,
      ptyExtractedCwd: parsed.cwd,
      normalizedRequestedRepoRoot: normalizePath(repoRoot),
      normalizedPtyCwd: normalizePath(parsed.cwd),
      uniqueMarker,
      endSequenceBefore: endBefore,
      endSequenceAfter: endAfter,
    };

    console.log("MACHINE_READABLE_SUMMARY:", JSON.stringify(summary, null, 2));
    if (summary.verdict !== "PASS") throw new Error(`Summary check failed: ${JSON.stringify(summary)}`);
    console.log(`E2E PASS: marker and actual PTY CWD verified (${summary.ptyExtractedCwd})`);
  } finally {
    await closeOwnedSession(controlClient, streamClient, sessionId);
  }
}

if (import.meta.main || process.argv[1]?.endsWith("win-daemon-e2e.mjs")) {
  try {
    await main();
    process.exitCode = 0;
  } catch (err) {
    console.error("E2E FAIL:", err.message);
    process.exitCode = 1;
  }
}
