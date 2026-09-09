import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { once } from "node:events";
import { mkdtemp, mkdir, rm, chmod, readFile, copyFile } from "node:fs/promises";
import { tmpdir, homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const binary = process.env.FERRYX_QA_HELPER_BINARY ??
  join(repo, "remote-helper", "target", "debug",
    process.platform === "win32" ? "ferryx-remote-helper.exe" : "ferryx-remote-helper");

const fixture = await mkdtemp(join(tmpdir(), "ferryx-bridge-qa-"));
const binDir = join(fixture, "bin");
const stateDir = join(fixture, "state");
const projectDir = join(fixture, "project");
const installedBinary = join(binDir, process.platform === "win32" ? "ferryx-remote-helper.exe" : "ferryx-remote-helper");
const hostId = `qa-bridge-${randomUUID().slice(0, 8)}`;
const nonce = `nonce-${randomUUID().slice(0, 12)}`;

const ownedSshChildren = [];
let helperDaemonPid = null;
let ptyPid = null;

async function bounded(promise, label, ms = 15_000) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(`${label}: timed out after ${ms}ms`)), ms);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

async function sha256File(path) {
  try {
    const bytes = await readFile(path);
    return createHash("sha256").update(bytes).digest("hex");
  } catch {
    return null;
  }
}

async function reapPid(pid, label) {
  if (!pid) return;
  try {
    process.kill(pid, 0);
  } catch (err) {
    if (err.code === "ESRCH") return;
    return;
  }
  try {
    process.kill(pid, "SIGTERM");
  } catch {}
  const deadline = Date.now() + 5000;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 50));
    try {
      process.kill(pid, 0);
    } catch (err) {
      if (err.code === "ESRCH") {
        console.log(JSON.stringify({ event: "pid-reaped", label, pid }));
        return;
      }
    }
  }
  try {
    process.kill(pid, "SIGKILL");
  } catch {}
  const killDeadline = Date.now() + 3000;
  while (Date.now() < killDeadline) {
    await new Promise((r) => setTimeout(r, 50));
    try {
      process.kill(pid, 0);
    } catch (err) {
      if (err.code === "ESRCH") {
        console.log(JSON.stringify({ event: "pid-force-reaped", label, pid }));
        return;
      }
    }
  }
  console.error(`Warning: PID ${pid} (${label}) did not disappear within deadline`);
}

async function reapChild(child, label) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const closed = once(child, "close");
  child.kill("SIGTERM");
  try {
    await bounded(closed, `reap ${label} (${child.pid})`, 3000);
  } catch {
    child.kill("SIGKILL");
    await bounded(closed, `force reap ${label} (${child.pid})`, 3000);
  }
  console.log(JSON.stringify({ event: "child-reaped", label, pid: child.pid }));
}

const knownHostsPath = join(homedir(), ".ssh", "known_hosts");
const sshConfigPath = join(homedir(), ".ssh", "config");

const initialKnownHostsHash = await sha256File(knownHostsPath);
const initialSshConfigHash = await sha256File(sshConfigPath);

function sshArgs(remoteCommand) {
  return [
    "-T",
    "-o", "BatchMode=yes",
    "-o", "StrictHostKeyChecking=yes",
    "-o", "UpdateHostKeys=no",
    "-o", "ConnectTimeout=5",
    "127.0.0.1",
    remoteCommand,
  ];
}

async function runSsh(remoteCommand, label) {
  const child = spawn("ssh", sshArgs(remoteCommand), { stdio: ["pipe", "pipe", "pipe"] });
  ownedSshChildren.push(child);
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (d) => stdout += d.toString());
  child.stderr.on("data", (d) => stderr += d.toString());
  const [exitCode] = await bounded(once(child, "exit"), `ssh: ${label}`);
  return { exitCode, stdout, stderr, sshPid: child.pid };
}

function openSshBridgeTransport(label) {
  const cmd = `exec '${installedBinary}' bridge --stdio --root '${stateDir}'`;
  const child = spawn("ssh", sshArgs(cmd), { stdio: ["pipe", "pipe", "pipe"] });
  ownedSshChildren.push(child);

  let buffered = Buffer.alloc(0);
  const output = child.stdout[Symbol.asyncIterator]();
  let stderrText = "";
  child.stderr.on("data", (d) => stderrText += d.toString());

  return {
    child,
    pid: child.pid,
    async request(op, params = {}) {
      const payload = Buffer.from(JSON.stringify({ protocol: 1, token: "", op, params }));
      assert(payload.length <= 1024 * 1024, "request exceeds 1 MiB");
      const header = Buffer.alloc(4);
      header.writeUInt32BE(payload.length);
      child.stdin.write(Buffer.concat([header, payload]));

      const read = async () => {
        while (buffered.length < 4 || buffered.length < 4 + buffered.readUInt32BE()) {
          const next = await output.next();
          assert.equal(next.done, false, `${op}: SSH bridge closed unexpectedly. Stderr: ${stderrText}`);
          buffered = Buffer.concat([buffered, next.value]);
        }
        const len = buffered.readUInt32BE();
        assert(len <= 1024 * 1024, "response frame exceeds 1 MiB");
        const msg = JSON.parse(buffered.subarray(4, 4 + len).toString("utf8"));
        buffered = buffered.subarray(4 + len);
        return msg;
      };

      const reply = await bounded(read(), `${label} -> ${op}`);
      assert.equal(reply.ok, true, `${op} failed: ${reply.error}`);
      return reply.data;
    },
    async close() {
      child.stdin.end();
      await reapChild(child, label);
    },
  };
}

try {
  await chmod(fixture, 0o700);
  await mkdir(binDir, { mode: 0o700 });
  await mkdir(stateDir, { mode: 0o700 });
  await mkdir(projectDir, { mode: 0o700 });

  console.log(JSON.stringify({
    event: "fixture-created",
    fixture,
    hostId,
    nonce,
    initialKnownHostsHash,
    initialSshConfigHash,
  }));

  // 1. Install helper binary into fixture
  await copyFile(binary, installedBinary);
  await chmod(installedBinary, 0o700);
  console.log(JSON.stringify({ event: "helper-installed", path: installedBinary }));

  // 2. Start detached helper daemon via SSH
  const startScript = `exec '${installedBinary}' start --root '${stateDir}' --host-id '${hostId}'`;
  const startRes = await runSsh(startScript, "ssh-start-helper");
  assert.equal(startRes.exitCode, 0, `Startup failed: ${startRes.stderr}`);
  assert(startRes.stdout.includes('"event":"ready"'), "Daemon must output ready event");
  console.log(JSON.stringify({ event: "daemon-started-over-ssh", stdout: startRes.stdout.trim() }));

  // 3. Capture helper PID while stateDir still exists directly from endpoint.json
  const endpointContent = JSON.parse(await readFile(join(stateDir, "endpoint.json"), "utf8"));
  helperDaemonPid = endpointContent.pid;
  assert.ok(helperDaemonPid, "endpoint must record verified helper daemon PID");
  console.log(JSON.stringify({
    event: "helper-daemon-pid-captured",
    helperDaemonPid,
    endpoint: endpointContent.address,
  }));

  // 4. Open First SSH Bridge Transport (Connection 1)
  const bridge1 = openSshBridgeTransport("ssh-bridge-conn-1");
  console.log(JSON.stringify({ event: "bridge-conn-1-opened", sshPid: bridge1.pid }));

  // Handshake 1
  const handshake1 = await bridge1.request("handshake");
  assert.equal(handshake1.protocol, 1);
  assert.equal(handshake1.hostId, hostId);
  assert(handshake1.capabilities.includes("sshHelperV1"));
  const ownerId = handshake1.ownerId;
  const epoch = handshake1.epoch;
  console.log(JSON.stringify({ event: "handshake-1-success", ownerId, epoch }));

  // Register Project
  await bridge1.request("project.register", { id: "test-proj", path: projectDir });

  // Spawn PTY running a counter loop with the nonce
  const spawnScript = `sh -c 'printf "INIT:%s:1\\n" "$1"; c=2; while IFS= read -r line; do printf "ACK:%s:%d:%s\\n" "$1" "$c" "$line"; c=$((c+1)); done' -- '${nonce}'`;
  const spawnRes = await bridge1.request("pty.spawn", {
    projectId: "test-proj",
    worktree: ".",
    program: "/bin/sh",
    args: ["-c", spawnScript],
    clientRequestId: "req-survival-1",
  });

  const target = spawnRes.target;
  ptyPid = spawnRes.pid;
  assert(ptyPid > 0, "Spawned PTY must have a valid remote PID");
  console.log(JSON.stringify({
    event: "pty-spawned",
    remotePid: ptyPid,
    target,
  }));

  // Initial Read on Connection 1
  let cursor = "0";
  const read1 = await bridge1.request("pty.read", {
    target,
    cursor,
    waitMs: 2000,
  });
  cursor = read1.cursor;
  assert(read1.chunks.length > 0, "Expected output chunks from initial spawn");
  const initOutput = Buffer.concat(read1.chunks.map((c) => Buffer.from(c.data, "base64"))).toString("utf8");
  assert(initOutput.includes(`INIT:${nonce}:1`), `Expected INIT with nonce, got: ${initOutput}`);
  console.log(JSON.stringify({
    event: "step-1-verified-on-conn-1",
    cursor,
    output: initOutput.trim(),
    remotePid: ptyPid,
  }));

  // Send first increment on Connection 1
  await bridge1.request("pty.write", {
    target,
    data: Buffer.from("tick1\n").toString("base64"),
  });

  const readAck1 = await bridge1.request("pty.read", {
    target,
    cursor,
    waitMs: 2000,
  });
  cursor = readAck1.cursor;
  const ack1Output = Buffer.concat(readAck1.chunks.map((c) => Buffer.from(c.data, "base64"))).toString("utf8");
  assert(ack1Output.includes(`ACK:${nonce}:2:tick1`), `Expected ACK counter 2, got: ${ack1Output}`);
  console.log(JSON.stringify({
    event: "step-1-counter-2-verified",
    cursor,
    output: ack1Output.trim(),
  }));

  // 5. CUT THE CONNECTION! (Kill SSH Bridge 1)
  console.log(JSON.stringify({ event: "severing-ssh-connection-1", sshPid: bridge1.pid }));
  await bridge1.close();
  console.log(JSON.stringify({ event: "ssh-connection-1-closed-and-reaped", deadPid: bridge1.pid }));

  // 6. Open Second SSH Bridge Transport (Connection 2 - Reattach)
  const bridge2 = openSshBridgeTransport("ssh-bridge-conn-2");
  console.log(JSON.stringify({ event: "bridge-conn-2-opened", sshPid: bridge2.pid }));

  // Handshake on Connection 2 must match host, owner, epoch exactly
  const handshake2 = await bridge2.request("handshake");
  assert.equal(handshake2.protocol, 1);
  assert.equal(handshake2.hostId, hostId);
  assert.equal(handshake2.ownerId, ownerId, "Owner ID must remain identical across reconnection");
  assert.equal(handshake2.epoch, epoch, "Epoch must remain identical across reconnection");
  console.log(JSON.stringify({
    event: "handshake-2-verified-identical",
    ownerId: handshake2.ownerId,
    epoch: handshake2.epoch,
  }));

  // Describe existing TargetRef on Connection 2
  const describeRes = await bridge2.request("pty.describe", { target });
  assert.equal(describeRes.pid, ptyPid, "Remote PID must be IDENTICAL after connection reconnect");
  assert.equal(describeRes.exited, false, "Remote PTY must still be running");
  console.log(JSON.stringify({
    event: "pty-described-on-conn-2",
    remotePid: describeRes.pid,
    identicalPid: describeRes.pid === ptyPid,
    exited: describeRes.exited,
    cursor: describeRes.cursor,
  }));

  // Send second increment on Connection 2
  await bridge2.request("pty.write", {
    target,
    data: Buffer.from("tick2\n").toString("base64"),
  });

  // Read continuation chunks from Connection 2 using cursor
  const readAck2 = await bridge2.request("pty.read", {
    target,
    cursor,
    waitMs: 2000,
  });
  cursor = readAck2.cursor;
  const ack2Output = Buffer.concat(readAck2.chunks.map((c) => Buffer.from(c.data, "base64"))).toString("utf8");
  assert(ack2Output.includes(`ACK:${nonce}:3:tick2`), `Expected ACK counter 3 with same nonce, got: ${ack2Output}`);
  assert.equal(readAck2.pid, ptyPid, "ReadResult must report identical remote PID");

  console.log(JSON.stringify({
    event: "step-2-counter-3-verified-on-conn-2",
    cursor,
    output: ack2Output.trim(),
    remotePid: readAck2.pid,
    identicalNonce: true,
    identicalPid: readAck2.pid === ptyPid,
  }));

  // 7. Clean PTY Stop via RPC
  await bridge2.request("pty.stop", { target });
  console.log(JSON.stringify({ event: "pty-stopped-cleanly" }));

  // Close Connection 2
  await bridge2.close();
  console.log(JSON.stringify({ event: "bridge-conn-2-closed" }));

  // 8. Trust Hash Verification
  const finalKnownHostsHash = await sha256File(knownHostsPath);
  const finalSshConfigHash = await sha256File(sshConfigPath);

  assert.equal(
    finalKnownHostsHash,
    initialKnownHostsHash,
    "known_hosts SHA-256 hash modified during test!"
  );
  assert.equal(
    finalSshConfigHash,
    initialSshConfigHash,
    "ssh/config SHA-256 hash modified during test!"
  );

  console.log(JSON.stringify({
    event: "trust-hashes-preserved",
    knownHostsHash: finalKnownHostsHash,
    sshConfigHash: finalSshConfigHash,
  }));

  console.log("PASS: SSH bridge survival verified with identical remote PID, nonce, and continuous counter");
} finally {
  // Orderly and bounded teardown:
  // 1. Reap explicit QA PTY process while root still exists
  if (ptyPid) {
    try {
      await reapPid(ptyPid, "qa-pty-process");
    } catch {}
  }

  // 2. Reap verified helper daemon while root still exists and await disappearance
  if (helperDaemonPid) {
    try {
      await reapPid(helperDaemonPid, "helper-daemon");
    } catch {}
  }

  // 3. Reap any owned SSH child processes
  for (const child of ownedSshChildren.toReversed()) {
    try {
      await reapChild(child, "ssh-child");
    } catch {}
  }

  // 4. Delete fixture only after all processes have disappeared
  try {
    await rm(fixture, { recursive: true, force: true });
  } catch {}

  console.log(JSON.stringify({
    event: "cleanup-receipt",
    fixtureRemoved: fixture,
    reapedHelperPid: helperDaemonPid,
    reapedPtyPid: ptyPid,
  }));
}
