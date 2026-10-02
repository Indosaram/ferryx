import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, mkdir, chmod, readdir, readFile, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const binary = resolve(process.argv[2] ?? "");
assert(process.argv[2], "Pass the freshly built standalone helper executable");
const fixture = await mkdtemp(join(tmpdir(), "fx-reboot-"));
const runtime = join(fixture, "runtime");
const storage = join(fixture, "receipts");
const project = join(fixture, "project");
const bin = join(fixture, "bin");
const children = [];
const host = "qa-reboot-isolated";
const logicalSessionId = "qa-original-request";
const conversation = "qa-exact-conversation";
let initialPid;
let recoveredPid;
let stopped = false;

async function bounded(promise, label) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`${label}: timeout`)), 30_000); }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

function launch(args) {
  const child = spawn(binary, args, {
    stdio: ["pipe", "pipe", "pipe"],
    env: { ...process.env, FERRYX_RECOVERY_ROOT: storage, PATH: bin + (process.platform === "win32" ? ";" : ":") + process.env.PATH },
  });
  children.push(child);
  child.stderr.on("data", bytes => process.stderr.write(bytes));
  return child;
}

async function terminate(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const closed = once(child, "close");
  child.kill("SIGKILL");
  await bounded(closed, "owned fixture child close");
}

async function start() {
  const daemon = launch(["daemon", "--root", runtime, "--host-id", host]);
  await bounded((async () => {
    let text = "";
    for await (const bytes of daemon.stdout) {
      text += bytes.toString();
      if (text.includes('"event":"ready"')) return;
    }
    throw new Error("isolated helper exited before ready");
  })(), "helper ready");
  const bridge = launch(["bridge", "--stdio", "--root", runtime]);
  const iterator = bridge.stdout[Symbol.asyncIterator]();
  let buffered = Buffer.alloc(0);
  return {
    daemon, bridge,
    async raw(op, params = {}) {
      const bytes = Buffer.from(JSON.stringify({ protocol: 1, token: "", op, params }));
      const header = Buffer.alloc(4);
      header.writeUInt32BE(bytes.length);
      bridge.stdin.write(Buffer.concat([header, bytes]));
      return bounded((async () => {
        while (buffered.length < 4 || buffered.length < 4 + buffered.readUInt32BE()) {
          const next = await iterator.next();
          assert(!next.done, `${op}: unexpected bridge EOF`);
          buffered = Buffer.concat([buffered, next.value]);
          if (buffered.length >= 4) assert(buffered.readUInt32BE() <= 1024 * 1024);
        }
        const length = buffered.readUInt32BE();
        const reply = JSON.parse(buffered.subarray(4, length + 4));
        buffered = buffered.subarray(length + 4);
        return reply;
      })(), op);
    },
    async request(op, params = {}) {
      const reply = await this.raw(op, params);
      assert(reply.ok, `${op}: ${reply.error}`);
      return reply.data;
    },
  };
}

async function receiptPath() {
  const hosts = join(storage, "hosts");
  const hostDir = (await readdir(hosts))[0];
  const sessions = join(hosts, hostDir, "sessions");
  return join(sessions, (await readdir(sessions))[0]);
}

async function awaitOutput(client, target, predicate) {
  let text = "";
  let cursor = "0";
  let agentAfterRevision = "0";
  return bounded((async () => {
    for (;;) {
      const output = await client.request("pty.read", { target, cursor, agentAfterRevision, waitMs: 10000 });
      if (output.agentState) agentAfterRevision = output.agentState.revision;
      for (const chunk of output.chunks) {
        text += Buffer.from(chunk.data, "base64").toString();
        cursor = chunk.cursor;
      }
      if (text.includes("\x1b[6n")) {
        await client.request("pty.write", { target, text: "\x1b[1;1R" });
        text = text.replaceAll("\x1b[6n", "");
      }
      if (predicate(output, text)) return { output, text };
      assert(!output.exited, `fixture process exited: ${text}`);
    }
  })(), "exact output/state event");
}

try {
  for (const dir of [runtime, storage, project, bin]) await mkdir(dir, { mode: 0o700 });
  const sessions = join(project, ".omo", "sessions");
  await mkdir(sessions, { recursive: true });
  await writeFile(join(sessions, conversation + ".jsonl"), JSON.stringify({ type: "session", id: conversation }) + "\n", { mode: 0o600 });
  const shim = join(bin, process.platform === "win32" ? "omo.cmd" : "omo");
  await writeFile(shim, process.platform === "win32"
    ? "@echo off\r\necho RECOVERED_ARGS: %~1 %~2\r\nset /p stop=\r\n"
    : "#!/bin/sh\nprintf 'RECOVERED_ARGS: %s %s\\n' \"$1\" \"$2\"\nread stop\n", { mode: 0o700 });
  if (process.platform !== "win32") await chmod(shim, 0o700);

  let client = await start();
  const handshake = await client.request("handshake");
  assert(handshake.capabilities.includes("ptyRecoveryV1"));
  await client.request("project.register", { id: "qa-project", path: project });
  const reporter = `const net=require("node:net");const s=net.connect(Number(process.env.FERRYX_AGENT_STATE_PORT),"127.0.0.1",()=>s.end(JSON.stringify({type:"agentState",sessionId:process.env.FERRYX_SESSION_ID,token:process.env.FERRYX_AGENT_STATE_TOKEN,state:"blocked",agent:"omo",providerSession:{key:"session_id",id:${JSON.stringify(conversation)}}})+"\\n"));process.stdin.resume();`;
  const initial = await client.request("pty.spawn", { projectId: "qa-project", clientRequestId: logicalSessionId, program: process.execPath, args: ["-e", reporter], cols: 100, rows: 30 });
  initialPid = initial.pid;
  await awaitOutput(client, initial.target, output => output.agentState?.providerSession?.id === conversation);
  const recordPath = await receiptPath();
  const record = JSON.parse(await readFile(recordPath, "utf8"));
  assert.equal(record.providerSession.id, conversation);
  assert.equal(record.disabled, false, "blocked agent must not be mistaken for an exit");
  await terminate(client.bridge);
  await terminate(client.daemon);
  try { process.kill(initialPid, "SIGKILL"); } catch (error) { if (error.code !== "ESRCH") throw error; }

  client = await start();
  const sameBoot = await client.raw("pty.recover", { logicalSessionId, previousTarget: initial.target });
  assert.equal(sameBoot.ok, false);
  assert(sameBoot.error.startsWith("RECOVERY_REFUSED:"));
  record.bootId = "qa-simulated-previous-kernel-boot";
  await writeFile(recordPath, JSON.stringify(record), { mode: 0o600 });
  const startedAt = performance.now();
  const recovered = await client.request("pty.recover", { logicalSessionId, previousTarget: initial.target });
  recoveredPid = recovered.pid;
  assert.notDeepEqual(recovered.target, initial.target);
  const resumed = await awaitOutput(client, recovered.target, (_, text) => text.includes("RECOVERED_ARGS: --session " + conversation));
  const duplicate = await client.request("pty.recover", { logicalSessionId, previousTarget: initial.target });
  assert.deepEqual(duplicate, recovered, "lost-response retry must not launch a duplicate");
  const fenced = await client.raw("pty.describe", { target: initial.target });
  assert.equal(fenced.ok, false);
  assert.equal(fenced.error, "TARGET_EXPIRED");
  await client.request("pty.stop", { target: recovered.target });
  stopped = true;
  const closed = await client.raw("pty.recover", { logicalSessionId, previousTarget: initial.target });
  assert.equal(closed.ok, false);
  assert(closed.error.startsWith("RECOVERY_REFUSED:"));
  console.log(JSON.stringify({
    verdict: "PASS", platform: process.platform, helperVersion: handshake.helperVersion,
    bootChange: "synthetic receipt edit; no production machine reboot",
    logicalSessionId, previousTarget: initial.target, recoveredTarget: recovered.target,
    exactConversation: conversation, resumeOutput: resumed.text,
    sameBootRefused: true, oldTargetFenced: true, lostResponseIdempotent: true,
    explicitStopRefused: true, elapsedMs: Math.round(performance.now() - startedAt),
  }, null, 2));
} finally {
  if (recoveredPid && !stopped) {
    try { process.kill(recoveredPid, "SIGKILL"); } catch (error) { if (error.code !== "ESRCH") throw error; }
  }
  for (const child of children.reverse()) await terminate(child);
  await rm(fixture, { recursive: true, force: true });
}
