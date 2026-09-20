import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { once } from "node:events";
import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Live-host QA for the darwin (and linux) remote helper. Framing and marker
// reading mirror ssh-bridge-survival.mjs; that harness is never modified.
const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const SSH_OPTIONS = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=6", "-o", "StrictHostKeyChecking=yes", "-o", "UpdateHostKeys=no"];

function log(event, fields = {}) {
  console.log(JSON.stringify({ event, ...fields }));
}

async function bounded(promise, label, ms = 20_000) {
  let timer;
  try {
    return await Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`${label}: timed out after ${ms}ms`)), ms); })]);
  } finally {
    clearTimeout(timer);
  }
}

function quote(value) {
  return `'${String(value).replaceAll("'", `'\\''`)}'`;
}

async function runSsh(host, remoteCommand, label, { stdinPath } = {}) {
  const child = spawn("ssh", ["-T", ...SSH_OPTIONS, host, remoteCommand], { stdio: [stdinPath ? "pipe" : "ignore", "pipe", "pipe"] });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (d) => { stdout += d.toString(); });
  child.stderr.on("data", (d) => { stderr += d.toString(); });
  if (stdinPath) createReadStream(stdinPath).pipe(child.stdin);
  const [exitCode] = await bounded(once(child, "exit"), `ssh: ${label}`);
  return { exitCode, stdout, stderr };
}

// Second, independent SSH connection carrying the length-prefixed bridge protocol.
function openBridge(host, helper, root, label) {
  const child = spawn("ssh", ["-T", ...SSH_OPTIONS, host, `exec ${quote(helper)} bridge --stdio --root ${quote(root)}`], { stdio: ["pipe", "pipe", "pipe"] });
  let buffered = Buffer.alloc(0);
  let stderrText = "";
  const output = child.stdout[Symbol.asyncIterator]();
  child.stderr.on("data", (d) => { stderrText += d.toString(); });
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
        const message = JSON.parse(buffered.subarray(4, 4 + len).toString("utf8"));
        buffered = buffered.subarray(4 + len);
        return message;
      };
      const reply = await bounded(read(), `${label} -> ${op}`);
      assert.equal(reply.ok, true, `${op} failed: ${reply.error}`);
      return reply.data;
    },
    async close() {
      child.stdin.end();
      if (child.exitCode === null && child.signalCode === null) {
        const closed = once(child, "close");
        child.kill("SIGTERM");
        try { await bounded(closed, `reap ${label}`, 5000); } catch { child.kill("SIGKILL"); await bounded(closed, `force reap ${label}`, 5000); }
      }
      log("bridge-closed", { label, sshPid: child.pid });
    },
  };
}

async function readSentinel(bridge, target, cursor, sentinel, expectedPid) {
  const parts = [];
  let byteLength = 0;
  for (;;) {
    const reply = await bridge.request("pty.read", { target, cursor, waitMs: 2000 });
    assert.notEqual(reply.gap, true, "sentinel output was lost");
    if (expectedPid !== undefined) assert.equal(reply.pid, expectedPid);
    cursor = reply.cursor;
    for (const chunk of reply.chunks) {
      const bytes = Buffer.from(chunk.data, "base64");
      byteLength += bytes.length;
      assert(byteLength <= 1024 * 1024, "sentinel exceeds output bound");
      parts.push(bytes);
    }
    const text = Buffer.concat(parts).toString("utf8").replaceAll("\r\n", "\n");
    if (text.includes(sentinel)) return { ...reply, text, cursor };
    assert.equal(reply.exited, false, `process exited before sentinel: ${text}`);
  }
}

export async function verifyHost({ host, artifact }) {
  const run = randomUUID().slice(0, 8);
  const hostId = `qa-macos-${run}`;
  const sentinel = `SENTINEL-${randomUUID()}`;
  await stat(artifact);

  const probe = await runSsh(host, "uname -s; uname -m", "probe");
  assert.equal(probe.exitCode, 0, `probe failed: ${probe.stderr}`);
  const [remoteOs, remoteArch] = probe.stdout.trim().split(/\s+/);
  log("remote-probed", { host, remoteOs, remoteArch });

  const mktemp = await runSsh(host, `mktemp -d ${quote(`/tmp/ferryx-qa-${run}.XXXXXX`)}`, "mktemp");
  assert.equal(mktemp.exitCode, 0, `mktemp failed: ${mktemp.stderr}`);
  const qaRoot = mktemp.stdout.trim();
  assert(qaRoot.startsWith("/tmp/ferryx-qa-"), `unexpected QA root: ${qaRoot}`);
  const helper = `${qaRoot}/ferryx-remote-helper`;
  const stateRoot = `${qaRoot}/state`;
  const projectDir = `${qaRoot}/project`;
  log("qa-root-created", { host, qaRoot });

  let helperPid = null;
  let bridge = null;
  try {
    const install = await runSsh(host, `umask 077 && mkdir -p ${quote(projectDir)} && cat > ${quote(helper)} && chmod 700 ${quote(helper)} && ${quote(helper)} --version 2>/dev/null; echo installed`, "install", { stdinPath: artifact });
    assert.equal(install.exitCode, 0, `install failed: ${install.stderr}`);
    log("helper-installed", { host, helper });

    const start = await runSsh(host, `exec ${quote(helper)} start --root ${quote(stateRoot)} --host-id ${quote(hostId)}`, "start-helper");
    assert.equal(start.exitCode, 0, `helper start failed: ${start.stderr}`);
    assert(start.stdout.includes('"event":"ready"'), `helper must report ready, got: ${start.stdout}`);
    log("helper-started", { host, stdout: start.stdout.trim() });

    // The PID is proved ours: endpoint.json lives under a root we created this run
    // and records the unique host id we passed to `start`.
    const endpointRead = await runSsh(host, `cat ${quote(`${stateRoot}/endpoint.json`)}`, "endpoint");
    assert.equal(endpointRead.exitCode, 0, `endpoint read failed: ${endpointRead.stderr}`);
    const endpoint = JSON.parse(endpointRead.stdout);
    assert.equal(endpoint.hostId ?? null, null, "endpoint.json must expose exactly address/token/pid");
    // macOS canonicalizes /tmp to /private/tmp, so ownership is proven by the unique run token.
    assert(
      typeof endpoint.address === "string" && endpoint.address.includes(`/ferryx-qa-${run}`),
      `helper endpoint must live under the QA root we created this run, got: ${endpoint.address}`,
    );
    helperPid = endpoint.pid;
    assert(Number.isInteger(helperPid) && helperPid > 0, "endpoint must record the helper daemon PID");
    log("helper-pid-captured", { host, helperPid, address: endpoint.address });

    bridge = openBridge(host, helper, stateRoot, `bridge-${run}`);
    const handshake = await bridge.request("handshake");
    assert.equal(handshake.protocol, 1, "handshake protocol must be 1");
    assert.equal(handshake.hostId, hostId);
    assert(handshake.capabilities.includes("sshHelperV1"), "handshake must advertise sshHelperV1");
    log("handshake-verified", { host, protocol: handshake.protocol, capabilities: handshake.capabilities, ownerId: handshake.ownerId, epoch: handshake.epoch });

    await bridge.request("project.register", { id: "qa-proj", path: projectDir });
    const spawned = await bridge.request("pty.spawn", { projectId: "qa-proj", worktree: ".", program: "/bin/sh", args: ["-c", "exec /bin/sh -i"], clientRequestId: `qa-${run}` });
    const target = spawned.target;
    assert(spawned.pid > 0, "spawned PTY must report a remote PID");
    log("pty-spawned", { host, remotePid: spawned.pid, target });

    await bridge.request("pty.write", { target, data: Buffer.from(`echo ${sentinel}\n`).toString("base64") });
    const echoed = await readSentinel(bridge, target, "0", sentinel, spawned.pid);
    log("sentinel-round-trip", { host, sentinel, remotePid: echoed.pid, output: echoed.text.trim().split("\n").slice(-3) });

    await bridge.request("pty.stop", { target });
    log("pty-stopped", { host, target });
    await bridge.close();
    bridge = null;

    console.log(`PASS: ${host} (${remoteOs} ${remoteArch}) handshake protocol 1 + sshHelperV1 + sentinel round trip + explicit stop`);
    return { host, remoteOs, remoteArch, hostId, sentinel, helperPid, qaRoot };
  } finally {
    const cleanup = [];
    if (bridge) { try { await bridge.close(); } catch (error) { cleanup.push(error); } }
    if (helperPid) {
      const command = `kill ${helperPid} 2>/dev/null; sleep 1; kill -0 ${helperPid} 2>/dev/null && kill -9 ${helperPid}; sleep 1; kill -0 ${helperPid} 2>/dev/null && echo HELPER_STILL_RUNNING || echo HELPER_REAPED`;
      const reap = await runSsh(host, command, "reap-helper");
      log("cleanup-helper-reap", { host, command, exitCode: reap.exitCode, stdout: reap.stdout.trim() });
      if (!reap.stdout.includes("HELPER_REAPED")) cleanup.push(new Error(`helper PID ${helperPid} survived cleanup on ${host}`));
    }
    const removal = `rm -rf ${quote(qaRoot)}; test -e ${quote(qaRoot)} && echo QA_ROOT_PRESENT || echo QA_ROOT_REMOVED`;
    const removed = await runSsh(host, removal, "remove-root");
    log("cleanup-root-removed", { host, command: removal, exitCode: removed.exitCode, stdout: removed.stdout.trim() });
    if (!removed.stdout.includes("QA_ROOT_REMOVED")) cleanup.push(new Error(`QA root ${qaRoot} survived cleanup on ${host}`));
    if (cleanup.length) throw new AggregateError(cleanup, `cleanup unproved on ${host}`);
  }
}

if (import.meta.main) {
  const host = process.argv[2];
  const artifact = process.argv[3] ?? resolve(REPO, "src-tauri/resources/helpers/aarch64-apple-darwin/ferryx-remote-helper");
  assert(host, "usage: node scripts/qa/ssh-macos-remote-survival.mjs <ssh-host> [helper-artifact]");
  await verifyHost({ host, artifact });
}
