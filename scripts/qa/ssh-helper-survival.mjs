import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { once } from "node:events";
import { mkdtemp, mkdir, rm, chmod, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const binary = process.env.FERRYX_QA_HELPER_BINARY ??
  join(repo, "remote-helper", "target", "debug",
    process.platform === "win32" ? "ferryx-remote-helper.exe" : "ferryx-remote-helper");
const fixture = await mkdtemp(join(tmpdir(), "ferryx-helper-qa-"));
const stateDir = join(fixture, "state");
const projectDir = join(fixture, "project");
const nonce = randomUUID();
const children = [];
let target;
let bridge;
let remoteStopped = false;
let spawnCount = 0;
let remoteShellPid;

async function bounded(promise, label) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`${label}: timed out`)), 15_000);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

function child(args) {
  const process = spawn(binary, args, { stdio: ["pipe", "pipe", "pipe"] });
  process.stderr.on("data", (chunk) => console.error(`helper[${process.pid}]: ${chunk}`));
  children.push(process);
  return process;
}

async function stop(process) {
  if (process.exitCode !== null || process.signalCode !== null) return;
  const closed = once(process, "close");
  process.kill("SIGKILL");
  await bounded(closed, `reap ${process.pid}`);
  console.log(`cleanup: reaped child ${process.pid}`);
}

function connect() {
  const process = child(["bridge", "--stdio", "--root", stateDir]);
  const output = process.stdout[Symbol.asyncIterator]();
  let buffered = Buffer.alloc(0);
  return {
    process,
    async requestRaw(op, params = {}) {
      if (op === "pty.spawn") spawnCount++;
      const payload = Buffer.from(JSON.stringify({ protocol: 1, token: "", op, params }));
      const header = Buffer.alloc(4);
      header.writeUInt32BE(payload.length);
      process.stdin.write(Buffer.concat([header, payload]));
      const read = async () => {
        while (buffered.length < 4 || buffered.length < 4 + buffered.readUInt32BE()) {
          const next = await output.next();
          assert.equal(next.done, false, `${op}: bridge closed before response`);
          buffered = Buffer.concat([buffered, next.value]);
          if (buffered.length >= 4) {
            assert.ok(buffered.readUInt32BE() <= 1024 * 1024, "response exceeds frame limit");
          }
        }
        const length = buffered.readUInt32BE();
        const reply = JSON.parse(buffered.subarray(4, 4 + length).toString("utf8"));
        buffered = buffered.subarray(4 + length);
        return reply;
      };
      return bounded(read(), op);
    },
    async request(op, params = {}) {
      const reply = await this.requestRaw(op, params);
      assert.equal(reply.ok, true, `${op}: ${reply.error}`);
      return reply.data;
    },
  };
}

let cursor = "0";
async function status(connection, expectedCounter) {
  let text = "";
  let cursorReports = 0;
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    const reply = await connection.request("pty.read", { target, cursor, waitMs: 2000 });
    assert.equal(reply.gap, false, "small fixture must not overflow replay buffer");
    for (const chunk of reply.chunks) {
      const bytes = chunk.data !== undefined
        ? Buffer.from(chunk.data, "base64")
        : Buffer.from(chunk.bytes);
      text += bytes.toString("utf8");
      assert.equal(typeof chunk.cursor, "string", "remote output cursor must be lossless");
      cursor = chunk.cursor;
    }
    const queries = text.split("\x1b[6n").length - 1;
    while (cursorReports < queries) {
      await connection.request("pty.write", { target, text: "\x1b[1;1R" });
      cursorReports++;
    }
    const match = text.match(new RegExp(`SURVIVAL:(\\d+):${nonce}:${expectedCounter}(?:\\r?\\n)`));
    if (match) return { pid: Number(match[1]), counter: expectedCounter, nonce };
    assert.equal(reply.exited, false, `remote process exited: ${text}`);
  }
  throw new Error(`No status for counter ${expectedCounter}: ${text}`);
}

try {
  await mkdir(stateDir, { mode: 0o700 });
  await mkdir(projectDir);
  const daemon = child(["daemon", "--root", stateDir, "--host-id", "qa-host"]);
  const ready = (async () => {
    let text = "";
    for await (const chunk of daemon.stdout) {
      text += chunk.toString();
      if (text.includes('"event":"ready"')) return;
    }
    throw new Error("helper exited before ready");
  })();
  await bounded(ready, "helper ready");
  bridge = connect();
  const handshake = await bridge.request("handshake");
  assert.equal(handshake.protocol, 1);
  await bridge.request("project.register", { id: "qa-project", path: projectDir });
  const script = process.platform === "win32"
    ? `$n=0; while (($line=[Console]::ReadLine()) -ne $null) { $n++; [Console]::WriteLine(('SURVIVAL:{0}:${nonce}:{1}' -f $PID,$n)) }`
    : `n=0; while IFS= read -r line; do n=$((n+1)); printf 'SURVIVAL:%s:${nonce}:%s\\n' "$$" "$n"; done`;
  const args = process.platform === "win32"
    ? ["-NoLogo", "-NoProfile", "-Command", script]
    : ["-c", script];
  const spawned = await bridge.request("pty.spawn", {
    projectId: "qa-project", worktree: ".",
    program: process.platform === "win32" ? "powershell.exe" : "/bin/sh",
    args, cols: 100, rows: 30, clientRequestId: `qa-${nonce}`,
  });
  target = spawned.target;
  if (process.platform === "win32") {
    let startup = "";
    const deadline = Date.now() + 15_000;
    while (!startup.includes("\x1b[6n") && Date.now() < deadline) {
      const reply = await bridge.request("pty.read", { target, cursor, waitMs: 2000 });
      assert.equal(reply.exited, false, "ConPTY must remain alive during terminal initialization");
      for (const chunk of reply.chunks) {
        startup += Buffer.from(chunk.data, "base64").toString("utf8");
        cursor = chunk.cursor;
      }
    }
    assert.ok(startup.includes("\x1b[6n"), "ConPTY must request its initial cursor position");
    await bridge.request("pty.write", { target, text: "\x1b[1;1R" });
  }
  const enter = process.platform === "win32" ? "\r" : "\n";
  await bridge.request("pty.write", { target, text: `first${enter}` });
  const before = await status(bridge, 1);
  console.log(JSON.stringify({ event: "before-disconnect", target, ...before }));
  await stop(bridge.process);
  bridge = connect();
  const reconnected = await bridge.request("handshake");
  assert.equal(reconnected.epoch, handshake.epoch);
  await bridge.request("pty.write", { target, text: `second${enter}` });
  const after = await status(bridge, 2);
  remoteShellPid = after.pid;
  assert.equal(after.pid, before.pid);
  assert.equal(after.nonce, before.nonce);
  assert.equal(spawnCount, 1, "reattachment must not spawn a replacement process");
  console.log(JSON.stringify({ event: "after-reconnect", target, ...after, spawnCount }));

  // 1. Verify pty.describe metadata across bridge
  const describe = await bridge.request("pty.describe", { target });
  assert.equal(describe.pid, before.pid, "describe must report identical remote PID");
  assert.equal(describe.cols, 100, "describe must report configured cols");
  assert.equal(describe.rows, 30, "describe must report configured rows");
  assert.equal(describe.exited, false, "describe must report process running");
  console.log(JSON.stringify({ event: "verified-describe", describe }));

  // 2. Test duplicate clientRequestId deduplication (matching parameters)
  const spawnParams = {
    projectId: "qa-project",
    worktree: ".",
    program: process.platform === "win32" ? "powershell.exe" : "/bin/sh",
    args,
    cols: 100,
    rows: 30,
    clientRequestId: `qa-${nonce}`,
  };
  const dedupeSpawn = await bridge.requestRaw("pty.spawn", spawnParams);
  assert.equal(dedupeSpawn.ok, true, "duplicate spawn with matching params must succeed");
  assert.equal(dedupeSpawn.data.target.backendSessionId, target.backendSessionId, "deduped spawn must return identical backendSessionId");
  assert.equal(dedupeSpawn.data.pid, before.pid, "deduped spawn must return identical PID");
  assert.equal(spawnCount, 2, "dedupe probe is a second request, not a second allocation");
  const sessions = await bridge.request("pty.list");
  assert.equal(sessions.length, 1, "dedupe must leave exactly one remote process");
  assert.equal(sessions[0].pid, before.pid);
  console.log(JSON.stringify({ event: "verified-spawn-dedupe", dedupeSpawn: dedupeSpawn.data, spawnRequests: spawnCount }));

  // 3. Test duplicate clientRequestId conflict (different parameters)
  const conflictSpawn = await bridge.requestRaw("pty.spawn", { ...spawnParams, cols: 120 });
  assert.equal(conflictSpawn.ok, false, "duplicate clientRequestId with different params must fail");
  assert.ok(
    conflictSpawn.error.startsWith("REQUEST_CONFLICT"),
    `expected REQUEST_CONFLICT, got ${conflictSpawn.error}`
  );
  console.log(JSON.stringify({ event: "verified-spawn-conflict", error: conflictSpawn.error }));

  // 4. Test expired TargetRef (different epoch)
  const expiredTarget = { ...target, epoch: "9999999999999999999" };
  const expiredRead = await bridge.requestRaw("pty.describe", { target: expiredTarget });
  assert.equal(expiredRead.ok, false, "expired epoch TargetRef must fail");
  assert.equal(expiredRead.error, "TARGET_EXPIRED", `expected TARGET_EXPIRED, got ${expiredRead.error}`);
  console.log(JSON.stringify({ event: "verified-expired-target", error: expiredRead.error }));

  // 5. Test unknown backendSessionId
  const unknownTarget = { ...target, backendSessionId: randomUUID() };
  const unknownRead = await bridge.requestRaw("pty.describe", { target: unknownTarget });
  assert.equal(unknownRead.ok, false, "unknown backendSessionId must fail");
  assert.equal(unknownRead.error, "NOT_FOUND", `expected NOT_FOUND, got ${unknownRead.error}`);
  console.log(JSON.stringify({ event: "verified-unknown-target", error: unknownRead.error }));

  // 6. Test bridge unallowlisted operation
  const unallowlisted = await bridge.requestRaw("shutdown", {});
  assert.equal(unallowlisted.ok, false, "unallowlisted operation must fail");
  assert.equal(unallowlisted.error, "UNSUPPORTED: operation not allowlisted");
  console.log(JSON.stringify({ event: "verified-allowlist-rejection", error: unallowlisted.error }));

  // POSIX mode/symlink probes; Windows ACL rejection has a native Rust fixture.
  if (process.platform !== "win32") {
  const insecureFixture = await mkdtemp(join(tmpdir(), "ferryx-insecure-qa-"));
  try {
    await chmod(insecureFixture, 0o755);
    const insecureProc = child(["bridge", "--stdio", "--root", insecureFixture]);
    let insecureStderr = "";
    insecureProc.stderr.on("data", (c) => insecureStderr += c.toString());
    const [insecureExit] = await bounded(once(insecureProc, "exit"), "insecure root bridge exit");
    assert.notEqual(insecureExit, 0, "bridge on insecure root must exit with non-zero code");
    assert.ok(
      insecureStderr.includes("FORBIDDEN: helper IPC must be owned by the current user and private"),
      `expected private error, got: ${insecureStderr}`
    );
    console.log(JSON.stringify({ event: "verified-insecure-root-rejection", exitCode: insecureExit, stderr: insecureStderr.trim() }));
  } finally {
    await rm(insecureFixture, { recursive: true, force: true });
  }

  // 7b. Symlinked endpoint.json rejection
  const symlinkFixture = await mkdtemp(join(tmpdir(), "ferryx-symlink-qa-"));
  try {
    await chmod(symlinkFixture, 0o700);
    const dummy = join(symlinkFixture, "dummy.json");
    await writeFile(dummy, "{}");
    await chmod(dummy, 0o600);
    await symlink(dummy, join(symlinkFixture, "endpoint.json"));
    const symlinkProc = child(["bridge", "--stdio", "--root", symlinkFixture]);
    let symlinkStderr = "";
    symlinkProc.stderr.on("data", (c) => symlinkStderr += c.toString());
    const [symlinkExit] = await bounded(once(symlinkProc, "exit"), "symlink endpoint bridge exit");
    assert.notEqual(symlinkExit, 0, "bridge on symlinked endpoint must exit with non-zero code");
    assert.ok(
      symlinkStderr.includes("FORBIDDEN: helper IPC cannot be a symlink"),
      `expected symlink error, got: ${symlinkStderr}`
    );
    console.log(JSON.stringify({ event: "verified-symlink-endpoint-rejection", exitCode: symlinkExit, stderr: symlinkStderr.trim() }));
  } finally {
    await rm(symlinkFixture, { recursive: true, force: true });
  }
  }

  // 8. Explicit pty.stop terminates child process
  const stopReply = await bridge.request("pty.stop", { target });
  assert.equal(stopReply.stopped, true, "pty.stop must report stopped: true");
  remoteStopped = true;

  // Verify remote shell process is reaped
  let shellAlive = true;
  try {
    process.kill(remoteShellPid, 0);
  } catch (err) {
    if (err.code === "ESRCH") shellAlive = false;
  }
  assert.equal(shellAlive, false, "remote shell child process must be terminated after pty.stop");
  console.log(JSON.stringify({ event: "verified-remote-shell-stopped", shellPid: remoteShellPid, shellAlive }));

  console.log("PASS: same remote PID, nonce and mutable counter after actual bridge SIGKILL");
} finally {
  const trackedPids = children.map((c) => c.pid);
  if (remoteShellPid) trackedPids.push(remoteShellPid);
  if (target && !remoteStopped) {
    try {
      if (!bridge || bridge.process.exitCode !== null || bridge.process.signalCode !== null) {
        bridge = connect();
      }
      await bridge.request("pty.stop", { target });
      remoteStopped = true;
    } catch (error) {
      console.error(`cleanup: remote stop failed: ${error}`);
      process.exitCode = 1;
    }
  }
  for (const owned of children.toReversed()) {
    try {
      await stop(owned);
    } catch (error) {
      console.error(`cleanup: child ${owned.pid} failed: ${error}`);
      process.exitCode = 1;
    }
  }
  await rm(fixture, { recursive: true, force: true });
  console.log(`cleanup: removed ${fixture}; remoteStopped=${remoteStopped}`);

  const remainingLivePids = [];
  for (const pid of trackedPids) {
    try {
      process.kill(pid, 0);
      remainingLivePids.push(pid);
    } catch (error) {
      if (error.code !== "ESRCH") throw error;
    }
  }
  assert.equal(remainingLivePids.length, 0, `QA processes still alive: ${remainingLivePids}`);
  console.log(JSON.stringify({
    event: "cleanup-receipt",
    fixtureRemoved: fixture,
    trackedPids,
    remainingLivePids,
    remoteStopped,
  }));
}
