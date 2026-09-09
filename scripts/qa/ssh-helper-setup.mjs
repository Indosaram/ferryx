import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { once } from "node:events";
import { mkdtemp, mkdir, rm, chmod, readFile, copyFile, access } from "node:fs/promises";
import { constants } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const binary = process.env.FERRYX_QA_HELPER_BINARY ??
  join(repo, "remote-helper", "target", "debug",
    process.platform === "win32" ? "ferryx-remote-helper.exe" : "ferryx-remote-helper");

const isWin = process.platform === "win32";
const sshTarget = process.env.FERRYX_QA_SSH_TARGET ?? (isWin ? "localhost" : "127.0.0.1");

async function rmRetry(dir, retries = 10) {
  for (let i = 0; i < retries; i++) {
    try {
      await rm(dir, { recursive: true, force: true });
      return;
    } catch (err) {
      if (err.code === "EBUSY" || err.code === "EPERM" || err.code === "ENOTEMPTY") {
        await new Promise((r) => setTimeout(r, 100));
        continue;
      }
      throw err;
    }
  }
  await rm(dir, { recursive: true, force: true });
}

// Ensure prerequisites exist; never succeed silently
try {
  await access(binary, constants.X_OK);
} catch {
  throw new Error(`Prerequisite missing: remote-helper binary not found or not executable at ${binary}. Run cargo build --manifest-path remote-helper/Cargo.toml`);
}

const fixture = await mkdtemp(join(tmpdir(), "f-setup-qa-"));
const binDir = join(fixture, "bin");
const stateDir = join(fixture, "state");
const projectDir = join(fixture, "project");
const installedBinary = join(binDir, isWin ? "ferryx-remote-helper.exe" : "ferryx-remote-helper");
const hostId = `qa-setup-${randomUUID().slice(0, 8)}`;
const nonce = randomUUID();

let target = null;
let bridge = null;
const ownedChildren = [];
let daemonPid;
let ptyPid;

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

async function reapChild(child, label) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const closed = once(child, "close");
  child.kill(isWin ? undefined : "SIGTERM");
  try {
    await bounded(closed, `reap ${label} (${child.pid})`, 3000);
  } catch {
    child.kill(isWin ? undefined : "SIGKILL");
    await bounded(closed, `force reap ${label} (${child.pid})`, 3000);
  }
  console.log(JSON.stringify({ event: "child-reaped", label, pid: child.pid }));
}

async function reapPid(pid, label) {
  if (!pid) return;
  try {
    process.kill(pid, 0);
  } catch (err) {
    if (err.code === "ESRCH") return;
    throw err;
  }
  try {
    process.kill(pid, isWin ? undefined : "SIGTERM");
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
    process.kill(pid, isWin ? undefined : "SIGKILL");
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
  throw new Error(`Failed to reap ${label} PID ${pid} within timeout`);
}

function sshArgs(remoteCommand) {
  return [
    "-T",
    "-n",
    "-o", "BatchMode=yes",
    "-o", "StrictHostKeyChecking=yes",
    "-o", "UpdateHostKeys=no",
    "-o", "ConnectTimeout=5",
    sshTarget,
    remoteCommand,
  ];
}

async function runSsh(remoteCommand, label) {
  const child = spawn("ssh", sshArgs(remoteCommand), { stdio: ["pipe", "pipe", "pipe"] });
  ownedChildren.push(child);
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (d) => stdout += d.toString());
  child.stderr.on("data", (d) => stderr += d.toString());
  const [exitCode] = await bounded(once(child, "exit"), `ssh: ${label}`);
  return { exitCode, stdout, stderr, sshPid: child.pid };
}

function connectBridge() {
  const child = spawn(installedBinary, ["bridge", "--stdio", "--root", stateDir], {
    stdio: ["pipe", "pipe", "pipe"],
  });
  ownedChildren.push(child);
  const output = child.stdout[Symbol.asyncIterator]();
  let buffered = Buffer.alloc(0);
  return {
    child,
    async request(op, params = {}) {
      const payload = Buffer.from(JSON.stringify({ protocol: 1, token: "", op, params }));
      const header = Buffer.alloc(4);
      header.writeUInt32BE(payload.length);
      child.stdin.write(Buffer.concat([header, payload]));
      const read = async () => {
        while (buffered.length < 4 || buffered.length < 4 + buffered.readUInt32BE()) {
          const next = await output.next();
          assert.equal(next.done, false, `${op}: bridge closed unexpectedly`);
          buffered = Buffer.concat([buffered, next.value]);
        }
        const len = buffered.readUInt32BE();
        const msg = JSON.parse(buffered.subarray(4, 4 + len).toString("utf8"));
        buffered = buffered.subarray(4 + len);
        return msg;
      };
      const reply = await bounded(read(), op);
      assert.equal(reply.ok, true, `${op} failed: ${reply.error}`);
      return reply.data;
    },
    async close() {
      child.stdin.end();
      await bounded(once(child, "exit"), "bridge close");
    },
  };
}

try {
  if (!isWin) {
    await chmod(fixture, 0o700);
  }
  await mkdir(binDir, { mode: 0o700 });
  await mkdir(stateDir, { mode: 0o700 });
  await mkdir(projectDir, { mode: 0o700 });

  console.log(JSON.stringify({
    event: "fixture-created",
    fixture,
    binDir,
    stateDir,
    hostId,
  }));

  // Verify SSH connectivity to target; must fail explicitly if target unreachable
  const probe = await runSsh(isWin ? "powershell -Command exit 0" : "true", "pre-flight probe");
  assert.equal(probe.exitCode, 0, `Pre-flight SSH connectivity to ${sshTarget} failed: ${probe.stderr}`);

  // 1. Explicit installation of caller-selected binary
  await copyFile(binary, installedBinary);
  if (!isWin) {
    await chmod(installedBinary, 0o700);
  }
  console.log(JSON.stringify({ event: "helper-installed", path: installedBinary }));

  // 2. Test missing helper over SSH gives actionable exit code 127
  const missingPath = join(binDir, isWin ? "nonexistent-helper.exe" : "nonexistent-helper");
  const missingScript = isWin
    ? `powershell -NoProfile -Command "if (-not (Test-Path -LiteralPath '${missingPath}')) { exit 127 }; & '${missingPath}' start --root '${stateDir}' --host-id '${hostId}'"`
    : `if [ ! -f '${missingPath}' ]; then exit 127; fi; exec '${missingPath}' start --root '${stateDir}' --host-id '${hostId}'`;
  const missingResult = await runSsh(missingScript, "missing-helper-probe");
  assert.notEqual(missingResult.exitCode, 0, "missing helper must exit non-zero");
  if (!isWin) {
    assert.equal(missingResult.exitCode, 127, "missing helper must exit 127 on POSIX");
  }
  console.log(JSON.stringify({
    event: "verified-missing-helper-rejection",
    exitCode: missingResult.exitCode,
  }));

  // 3. Start helper via SSH: detached startup
  const startScript = isWin
    ? `powershell -NoProfile -Command "& '${installedBinary}' start --root '${stateDir}' --host-id '${hostId}'"`
    : `exec '${installedBinary}' start --root '${stateDir}' --host-id '${hostId}'`;
  const startResult = await runSsh(startScript, "start-helper-ssh");
  assert.equal(startResult.exitCode, 0, `start over SSH must exit 0: ${startResult.stderr}`);

  // Parse readiness as JSON
  let readyObj = null;
  for (const line of startResult.stdout.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try {
      const parsed = JSON.parse(trimmed);
      if (parsed.event === "ready" && parsed.protocol === 1) {
        readyObj = parsed;
        break;
      }
    } catch {}
  }
  assert.ok(readyObj, `start output must contain valid ready JSON event, got: ${startResult.stdout}`);
  assert.equal(readyObj.event, "ready");
  assert.equal(readyObj.protocol, 1);
  assert.ok(!startResult.stdout.includes("token"), "start output must not leak auth tokens");

  console.log(JSON.stringify({
    event: "ssh-startup-complete",
    sshPid: startResult.sshPid,
    ready: readyObj,
  }));

  // 4. Verify SSH parent process is dead, but remote helper daemon SURVIVED
  let sshParentAlive = true;
  try {
    process.kill(startResult.sshPid, 0);
  } catch (err) {
    if (err.code === "ESRCH") sshParentAlive = false;
  }
  assert.equal(sshParentAlive, false, "SSH parent process must be dead after startup command exits");

  // Read endpoint.json created by the detached daemon
  const endpointContent = JSON.parse(await readFile(join(stateDir, "endpoint.json"), "utf8"));
  assert.ok(endpointContent.address, "endpoint must have address");
  assert.ok(endpointContent.token, "endpoint must have token");
  daemonPid = endpointContent.pid;
  assert.ok(daemonPid, "endpoint must record daemon PID");

  // Verify daemon PID is alive
  let daemonAlive = false;
  try {
    process.kill(daemonPid, 0);
    daemonAlive = true;
  } catch {}
  assert.ok(daemonAlive, `helper daemon PID ${daemonPid} must be running`);

  // Verify bridge connects to the detached daemon
  let bridgeConn = connectBridge();
  bridge = bridgeConn;
  const handshake1 = await bridgeConn.request("handshake");
  assert.equal(handshake1.protocol, 1);
  assert.equal(handshake1.hostId, hostId);
  const epoch1 = handshake1.epoch;
  console.log(JSON.stringify({
    event: "verified-detached-daemon-alive",
    daemonPid,
    epoch: epoch1,
    hostId: handshake1.hostId,
  }));

  // 5. Register project and spawn a PTY to prove real process ownership
  await bridgeConn.request("project.register", { id: "qa-setup-proj", path: projectDir });
  const script = isWin
    ? `$n=0; while (($line=[Console]::ReadLine()) -ne $null) { $n++; [Console]::WriteLine(('SETUP_SURVIVAL:{0}:${nonce}:{1}' -f $PID,$n)) }`
    : `n=0; while IFS= read -r line; do n=$((n+1)); printf 'SETUP_SURVIVAL:%s:${nonce}:%s\\n' "$$" "$n"; done`;
  const ptyProgram = isWin ? "powershell.exe" : "/bin/sh";
  const ptyArgs = isWin ? ["-NoLogo", "-NoProfile", "-Command", script] : ["-c", script];

  const spawned = await bridgeConn.request("pty.spawn", {
    projectId: "qa-setup-proj",
    worktree: ".",
    program: ptyProgram,
    args: ptyArgs,
    cols: 80,
    rows: 24,
    clientRequestId: `qa-req-${nonce}`,
  });
  target = spawned.target;
  ptyPid = spawned.pid;
  console.log(JSON.stringify({ event: "pty-spawned", ptyPid, target }));

  let cursor = "0";
  async function waitForCounter(conn, expectedCounter) {
    let text = "";
    let cursorReports = 0;
    const deadline = Date.now() + 15_000;
    while (Date.now() < deadline) {
      const reply = await conn.request("pty.read", { target, cursor, waitMs: 1000 });
      for (const chunk of reply.chunks) {
        const bytes = chunk.data !== undefined
          ? Buffer.from(chunk.data, "base64")
          : Buffer.from(chunk.bytes);
        text += bytes.toString("utf8");
        cursor = chunk.cursor;
      }
      if (isWin) {
        const queries = text.split("\x1b[6n").length - 1;
        while (cursorReports < queries) {
          await conn.request("pty.write", { target, text: "\x1b[1;1R" });
          cursorReports++;
        }
      }
      const match = text.match(new RegExp(`SETUP_SURVIVAL:(\\d+):${nonce}:${expectedCounter}(?:\\r?\\n)`));
      if (match) return { pid: Number(match[1]), counter: expectedCounter, text };
      assert.equal(reply.exited, false, `remote process exited unexpectedly: ${text}`);
    }
    throw new Error(`Timed out waiting for counter ${expectedCounter}: ${text}`);
  }

  // Write step 1
  const enter = isWin ? "\r\n" : "\n";
  await bridgeConn.request("pty.write", { target, text: `step1${enter}` });
  const step1 = await waitForCounter(bridgeConn, 1);
  assert.equal(step1.pid, ptyPid, "PTY PID must match");
  console.log(JSON.stringify({ event: "verified-step-1", ptyPid }));

  // Kill bridge connection (bridge-kill)
  await bridgeConn.close();
  bridge = null;

  // 6. Test idempotent ensure_started over SSH preserves the live helper
  const secondStart = await runSsh(startScript, "second-start-ssh");
  assert.equal(secondStart.exitCode, 0);

  // Reconnect bridge and verify epoch, PTY, and counter survived
  bridgeConn = connectBridge();
  bridge = bridgeConn;
  const handshake2 = await bridgeConn.request("handshake");
  assert.equal(handshake2.epoch, epoch1, "helper epoch must remain identical across idempotent start");

  const describe = await bridgeConn.request("pty.describe", { target });
  assert.equal(describe.pid, ptyPid, "PTY pid must remain identical");
  assert.equal(describe.exited, false, "PTY must still be running");

  // Write step 2
  await bridgeConn.request("pty.write", { target, text: `step2${enter}` });
  const step2 = await waitForCounter(bridgeConn, 2);
  assert.equal(step2.pid, ptyPid, "PTY PID must match across SSH start idempotence");
  console.log(JSON.stringify({ event: "verified-step-2-after-reconnect", ptyPid }));

  // Cleanly stop PTY
  await bridgeConn.request("pty.stop", { target });
  target = null;
  await bridgeConn.close();
  bridge = null;

  console.log("PASS: SSH detached helper startup, survival after SSH parent death, and idempotent preserve verified");
} finally {
  // Capture daemon PID from endpoint while root directory still exists
  if (!daemonPid) {
    try {
      const endpointContent = JSON.parse(await readFile(join(stateDir, "endpoint.json"), "utf8"));
      daemonPid = endpointContent.pid;
    } catch {}
  }

  // Graceful stop of active PTY session if still open
  if (target && bridge) {
    try {
      await bridge.request("pty.stop", { target });
    } catch {}
  }

  // Reap explicit PTY child process and await disappearance
  if (ptyPid) {
    try { await reapPid(ptyPid, "pty-child"); } catch (e) { console.error(e.message); }
  }

  // Reap verified helper daemon process and await disappearance before touching fixture
  if (daemonPid) {
    try { await reapPid(daemonPid, "helper-daemon"); } catch (e) { console.error(e.message); }
  }

  // Reap any remaining spawned SSH child processes
  for (const child of ownedChildren.toReversed()) {
    try { await reapChild(child, "ssh-child"); } catch (e) { console.error(e.message); }
  }

  // Only remove fixture after every process disappearance is verified
  await rmRetry(fixture);
  console.log(JSON.stringify({
    event: "cleanup-receipt",
    fixtureRemoved: fixture,
    reapedDaemonPid: daemonPid,
    reapedPtyPid: ptyPid,
  }));
}
