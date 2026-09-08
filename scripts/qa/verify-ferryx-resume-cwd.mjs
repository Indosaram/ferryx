import assert from "node:assert/strict";
import { once } from "node:events";
import { chmod, mkdir, mkdtemp, realpath, rm, writeFile } from "node:fs/promises";
import net from "node:net";
import { delimiter, join, resolve } from "node:path";
import { createInterface } from "node:readline";

if (process.platform === "win32") throw new Error("This QA driver uses the Unix daemon socket");
const binary = resolve(process.argv[2] ?? "src-tauri/target/debug/ferryx");
const root = await realpath(await mkdtemp("/tmp/fx-cwd-"));
const receiver = net.createServer();
let daemon;
let socket;
let lines;
let readyTimer;
let call;
const ownedSessions = new Set();

try {
  const repo = join(root, "project");
  const nested = join(repo, "nested");
  const runtime = join(root, "runtime");
  const bin = join(root, "bin");
  await Promise.all([mkdir(nested, { recursive: true }), mkdir(runtime), mkdir(bin)]);
  const listening = once(receiver, "listening");
  receiver.listen(0, "127.0.0.1");
  await listening;
  const address = receiver.address();
  assert.ok(address && typeof address !== "string");
  const agent = join(bin, "omo");
  await writeFile(agent, `#!${process.execPath}
import net from "node:net";
const socket = net.createConnection({host:"127.0.0.1",port:${address.port}});
socket.on("connect", () => socket.end(JSON.stringify({cwd:process.cwd(),argv:process.argv.slice(2)})+"\\n"));
socket.on("error", error => { console.error(error); process.exit(1); });
process.stdin.resume();
`);
  await chmod(agent, 0o755);
  const id = crypto.randomUUID();
  const store = join(root, ".omo", "agent", "sessions", "encoded-project");
  await mkdir(store, { recursive: true });
  const profiles = join(root, ".omo", "profiles");
  await mkdir(profiles);
  await writeFile(join(profiles, ".DS_Store"), "unrelated profile-directory metadata");
  const transcript = join(store, `2026-09-08T00-00-00-000Z_${id}.jsonl`);
  await writeFile(transcript, `${JSON.stringify({ type: "session", id, cwd: nested })}\n`);

  let daemonOutput = "";
  let ready;
  let failed;
  const readiness = new Promise((resolveReady, rejectReady) => {
    ready = resolveReady;
    failed = rejectReady;
  });
  daemon = Bun.spawn([binary, "--daemon"], {
    env: {
      PATH: `${bin}${delimiter}${process.env.PATH}`,
      HOME: root,
      USERPROFILE: root,
      FERRYX_RUNTIME_DIR: runtime,
      FERRYX_SESSION_DIR: join(root, "state"),
      PI_OFFLINE: "1",
    },
    stdout: "pipe",
    stderr: "pipe",
  });
  const stderr = new Response(daemon.stderr).text();
  const readReady = (async () => {
    for await (const chunk of daemon.stdout) {
      daemonOutput += new TextDecoder().decode(chunk);
      if (daemonOutput.includes("FERRYX_DAEMON_READY")) ready();
    }
  })();
  daemon.exited.then(async (code) => failed(new Error(`Daemon exited ${code}: ${await stderr}`)));
  readyTimer = setTimeout(() => failed(new Error(`Daemon readiness timed out: ${daemonOutput}`)), 20000);
  await readiness;
  clearTimeout(readyTimer);
  socket = net.createConnection({ path: join(runtime, "daemon.sock") });
  await once(socket, "connect", { signal: AbortSignal.timeout(5000) });
  lines = createInterface({ input: socket });
  call = async (request) => {
    const response = once(lines, "line", { signal: AbortSignal.timeout(10000) });
    socket.write(`${JSON.stringify(request)}\n`);
    return JSON.parse((await response)[0]);
  };
  assert.equal((await call({ type: "handshake", version: 3 })).type, "handshakeOk");
  assert.equal((await call({ type: "registerWorkspace", workspaceId: "qa", repoRoot: repo })).type, "registerWorkspaceOk");
  const results = [];
  for (const scenario of ["with-transcript", "legacy-id-only"]) {
    const receipt = (async () => {
      const [agentSocket] = await once(receiver, "connection", { signal: AbortSignal.timeout(10000) });
      const agentLines = createInterface({ input: agentSocket });
      try {
        return JSON.parse((await once(agentLines, "line", { signal: AbortSignal.timeout(10000) }))[0]);
      } finally {
        agentLines.close();
        agentSocket.destroy();
      }
    })();
    const spawning = call({
      type: "spawn", clientRequestId: scenario, workspaceId: "qa", worktree: null,
      cwd: repo, cols: 80, rows: 24,
      startup: {
        kind: "agentResume", agentType: "omo",
        providerSession: {
          key: "session_id", id,
          ...(scenario === "with-transcript" ? { transcriptPath: transcript } : {}),
        },
      },
    }).then((response) => {
      assert.equal(response.type, "spawnOk", JSON.stringify(response));
      ownedSessions.add(response.sessionId);
      return response;
    });
    const [spawned, observed] = await Promise.all([spawning, receipt]);
    assert.deepEqual(observed, { cwd: nested, argv: ["--session", id] });
    const description = await call({ type: "describeSession", sessionId: spawned.sessionId });
    assert.equal(description.session.cwd, nested);
    assert.equal(description.session.workspaceId, "qa");
    assert.equal((await call({ type: "close", sessionId: spawned.sessionId })).type, "closeOk");
    ownedSessions.delete(spawned.sessionId);
    results.push({ scenario, cwdCorrect: true, exactSessionIdPreserved: true, workspaceUnchanged: true });
  }
  console.log(JSON.stringify({ binary, results }, null, 2));
  socket.end();
  daemon.kill();
  await daemon.exited;
  await readReady;
} finally {
  clearTimeout(readyTimer);
  try {
    if (call && socket && !socket.destroyed) {
      for (const sessionId of ownedSessions) await call({ type: "close", sessionId });
    }
  } finally {
    lines?.close();
    socket?.destroy();
    if (daemon) {
      if (daemon.exitCode === null) daemon.kill();
      await daemon.exited;
    }
    receiver.close();
    await rm(root, { recursive: true, force: true });
  }
}
