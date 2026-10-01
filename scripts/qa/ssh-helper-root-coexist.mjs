#!/usr/bin/env node
/**
 * scripts/qa/ssh-helper-root-coexist.mjs
 *
 * Linux-only coexistence probe for two helper runtime roots owned by the caller.
 * Executed on the remote Linux host (never locally): it starts the real helper
 * service twice, for the same host id but two distinct private roots, and proves
 * the two runtimes coexist without either one killing the other.
 *
 *   <helper> start --root <rootA> --host-id <host>
 *   <helper> start --root <rootB> --host-id <host>
 *
 * Wire contract (exact; there is no newline-delimited or line-oriented mode):
 *   request frame : 4-byte big-endian body length + UTF-8 JSON body, decoded from
 *                   Request {protocol: 1, token, op, params} with deny_unknown_fields
 *                   (src-tauri/src/ferryx_scope/ssh/helper.rs:33, read by :40);
 *                   replies reuse write_frame (:61), the same 4-byte header
 *   reply frame   : {"ok":true,"data":<op payload>} or {"ok":false,"error":<string>}
 *                   (src-tauri/src/ferryx_scope/ssh/process.rs:63) - handshake data is
 *                   the identity object, pty.list data is a bare session array
 * Assertions over that socket:
 *   1. both handshakes answer protocol 1 with the same hostId, one non-empty
 *      helperVersion, and every unconditional capability;
 *   2. `pty.list` - the helper's exact enumeration op; `op` is a plain String field
 *      matched by string arms (src-tauri/src/ferryx_scope/ssh/helper.rs:630,
 *      allowlist process.rs:703), there is no enum/`listSessions` route - is empty in
 *      both runtimes. Only `handshake` and `pty.list` are ever sent, and neither
 *      spawns a shell: `pty.spawn` is the only spawning arm (helper.rs:407) and
 *      `serve` never spawns on connect (process.rs:72);
 *   3. each endpoint pid is the MainPID of a distinct `ferryx-helper-*` systemd
 *      --user unit, so both daemons are real services rather than the
 *      direct-spawn fallback (which has no unit MainPID);
 *   4. a repeated `start` for rootA is idempotent: endpoint pid and handshake
 *      epoch are unchanged.
 *
 * Cleanup signals ONLY the two owned pids: each endpoint pid is re-audited against
 * /proc/<pid>/exe (helper binary realpath) and /proc/<pid>/cmdline (--root canonical
 * root, --host-id) for its own root, plus zero child processes and zero sessions,
 * before any signal is sent. No production helper is touched, and none is signalled
 * on a mismatch. Termination is observed through the subscribed socket close event
 * with a bounded timeout, then procfs must show the pid gone (absent, zombie, or a
 * reused pid with a different identity) - never `kill(pid, 0)`, which a zombie still
 * answers. A root without an endpoint record is skipped as never-started after a
 * read-only /proc scan proves no matching daemon leaked. No systemctl stop, no
 * pkill, no broad signal, no fixed sleeps. A failed safety validation skips the
 * kill and is reported; cleanup failures are never swallowed.
 *
 * Usage:
 *   node scripts/qa/ssh-helper-root-coexist.mjs <helper-executable> <private-root-base>
 *
 * Machine output: newline-delimited sentinel records `FERRYX_HELPER_ROOT_COEXIST <json>`
 * (that delimiter belongs to this script's reporting, not to the helper protocol);
 * exit 0 only when every assertion and both cleanups verified.
 */

import { execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
import { chmodSync, constants, readdirSync, readFileSync, realpathSync, statSync, writeSync } from "node:fs";
import { access, mkdir } from "node:fs/promises";
import { createConnection } from "node:net";
import { join, resolve } from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

const START_TIMEOUT_MS = 30_000;
const CALL_TIMEOUT_MS = 10_000;
const CLOSE_TIMEOUT_MS = 10_000;
const UNIT_QUERY_TIMEOUT_MS = 10_000;
const LABEL = "FERRYX_HELPER_ROOT_COEXIST";
const REQUIRED_CAPABILITIES = ["sshHelperV1", "dagStreamingV1", "dagSubscribeV1"];

/** Synchronous sentinel write: process.exit below must never truncate machine output. */
function emit(event) {
  const buffer = Buffer.from(`${LABEL} ${JSON.stringify(event)}\n`, "utf8");
  let offset = 0;
  while (offset < buffer.length) offset += writeSync(1, buffer, offset, buffer.length - offset);
}

function bounded(promise, label, ms) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`${label}: timed out after ${ms}ms`)), ms);
    }),
  ]).finally(() => clearTimeout(timer));
}

function run(command, args, timeout = UNIT_QUERY_TIMEOUT_MS) {
  return execFileAsync(command, args, { timeout, maxBuffer: 4 * 1024 * 1024 });
}

function readEndpoint(root) {
  const path = join(root, "endpoint.json");
  const raw = readFileSync(path, "utf8");
  const value = JSON.parse(raw);
  if (typeof value.address !== "string" || value.address.length === 0) {
    throw new Error(`${path}: endpoint address missing`);
  }
  if (typeof value.token !== "string" || value.token.length === 0) {
    throw new Error(`${path}: endpoint token missing`);
  }
  if (!Number.isInteger(value.pid) || value.pid <= 0) {
    throw new Error(`${path}: endpoint pid missing`);
  }
  return { path, address: value.address, token: value.token, pid: value.pid };
}

/** Framed JSON client for one helper endpoint (mirrors helper read_frame/write_frame). */
function openFramedSocket(socketPath, label) {
  const socket = createConnection({ path: socketPath });
  let buffer = Buffer.alloc(0);
  let pending = null;
  let socketError = null;

  const fail = (error) => {
    if (pending) {
      const waiters = pending;
      pending = null;
      waiters.reject(error);
    }
  };

  socket.on("data", (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    while (pending && buffer.length >= 4) {
      const length = buffer.readUInt32BE(0);
      if (buffer.length < 4 + length) break;
      const value = JSON.parse(buffer.subarray(4, 4 + length).toString("utf8"));
      buffer = buffer.subarray(4 + length);
      const waiters = pending;
      pending = null;
      waiters.resolve(value);
    }
  });
  socket.on("error", (error) => {
    socketError = error.message;
    fail(new Error(`${label}: socket error: ${error.message}`));
  });
  socket.on("close", () => {
    fail(new Error(`${label}: socket closed before a response arrived`));
  });

  return {
    label,
    socket,
    get socketError() {
      return socketError;
    },
    async connect() {
      await bounded(
        new Promise((res, rej) => {
          socket.once("connect", res);
          socket.once("error", rej);
        }),
        `${label}: connect ${socketPath}`,
        CALL_TIMEOUT_MS
      );
    },
    async write(value) {
      const body = Buffer.from(JSON.stringify(value), "utf8");
      const header = Buffer.alloc(4);
      header.writeUInt32BE(body.length, 0);
      await bounded(
        new Promise((res, rej) => {
          socket.write(Buffer.concat([header, body]), (error) => (error ? rej(error) : res()));
        }),
        `${label}: write`,
        CALL_TIMEOUT_MS
      );
    },
    read() {
      if (pending) throw new Error(`${label}: concurrent read`);
      return bounded(
        new Promise((res, rej) => {
          pending = { resolve: res, reject: rej };
        }),
        `${label}: read`,
        CALL_TIMEOUT_MS
      );
    },
    destroy() {
      socket.destroy();
    },
  };
}

async function call(transport, token, op, params = {}) {
  await transport.write({ protocol: 1, token, op, params });
  const response = await transport.read();
  if (response === null || typeof response !== "object" || response.ok !== true) {
    // Reply envelope: {"ok":false,"error":<string>} for every helper rejection.
    const detail =
      response && response.error !== undefined ? response.error : JSON.stringify(response);
    throw new Error(`${transport.label}: ${op} rejected: ${detail}`);
  }
  if (!("data" in response)) {
    throw new Error(`${transport.label}: ${op} reply missing data: ${JSON.stringify(response)}`);
  }
  return response.data;
}

/**
 * The `start` CLI prints newline-delimited JSON on its own stdout (unrelated to the
 * frame protocol): the ready event is the parsed {event:"ready"} record.
 */
async function startRuntime(helper, hostId, root) {
  const { stdout, stderr } = await run(helper, ["start", "--root", root, "--host-id", hostId], START_TIMEOUT_MS);
  const ready = stdout
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => {
      try {
        return JSON.parse(line);
      } catch {
        return null;
      }
    })
    .find((value) => value && value.event === "ready");
  if (!ready) {
    throw new Error(`start ${root}: no ready event in stdout: ${stdout.trim()} ${stderr.trim()}`);
  }
  if (ready.protocol !== 1) {
    throw new Error(`start ${root}: ready protocol ${JSON.stringify(ready.protocol)} != 1`);
  }
  return stderr.trim();
}

async function connectEndpoint(root, endpoint, label) {
  const transport = openFramedSocket(endpoint.address, label);
  await transport.connect();
  const handshake = await call(transport, endpoint.token, "handshake", {});
  return { transport, handshake };
}

function assertHandshake(handshake, hostId, label) {
  if (handshake.protocol !== 1) {
    throw new Error(`${label}: handshake protocol ${JSON.stringify(handshake.protocol)} != 1`);
  }
  if (handshake.hostId !== hostId) {
    throw new Error(`${label}: handshake hostId ${JSON.stringify(handshake.hostId)} != ${hostId}`);
  }
  if (typeof handshake.helperVersion !== "string" || handshake.helperVersion.length === 0) {
    throw new Error(`${label}: handshake helperVersion missing`);
  }
  if (!Array.isArray(handshake.capabilities)) {
    throw new Error(`${label}: handshake capabilities missing`);
  }
  for (const capability of REQUIRED_CAPABILITIES) {
    if (!handshake.capabilities.includes(capability)) {
      throw new Error(`${label}: handshake missing capability ${capability}`);
    }
  }
}

async function listSessions(transport, token, label) {
  const sessions = await call(transport, token, "pty.list", {});
  if (!Array.isArray(sessions)) {
    throw new Error(`${label}: pty.list returned ${JSON.stringify(sessions)}`);
  }
  return sessions;
}

/** pid -> unit name for every running `ferryx-helper-*` systemd --user service. */
async function unitMainPidMap() {
  const listed = await run("systemctl", [
    "--user",
    "--no-pager",
    "list-units",
    "--all",
    "--plain",
    "--no-legend",
  ]);
  const units = listed.stdout
    .split("\n")
    .map((line) => line.trim().split(/\s+/)[0] ?? "")
    .filter((unit) => unit.startsWith("ferryx-helper-"));
  const map = new Map();
  for (const unit of units) {
    const shown = await run("systemctl", ["--user", "show", unit, "-p", "MainPID", "--value"]);
    const pid = Number.parseInt(shown.stdout.trim(), 10);
    if (Number.isInteger(pid) && pid > 0) map.set(pid, unit);
  }
  return map;
}

function childrenOf(pid) {
  const raw = readFileSync(`/proc/${pid}/task/${pid}/children`, "utf8").trim();
  return raw.length === 0 ? [] : raw.split(/\s+/);
}

/**
 * Termination proof from procfs, not from `kill(pid, 0)`: a zombie still answers
 * signal 0, and a reused pid would answer for a foreign process.
 */
function terminationState(pid, canonicalRoot, hostId, helperRealpath) {
  let stat;
  try {
    stat = readFileSync(`/proc/${pid}/stat`, "utf8");
  } catch (error) {
    if (error.code === "ENOENT") return { gone: true, state: "absent", pidReused: false };
    throw error;
  }
  const state = stat.slice(stat.lastIndexOf(")") + 2).split(" ")[0];
  if (state === "Z" || state === "X") return { gone: true, state, pidReused: false };
  let owner;
  try {
    owner = endpointOwnership(canonicalRoot, canonicalRoot, hostId, helperRealpath, pid);
  } catch (error) {
    if (error.code === "ENOENT") return { gone: true, state, pidReused: false };
    throw error;
  }
  const ours = owner.exeMatches && owner.rootMatches && owner.hostMatches;
  return { gone: !ours, state, pidReused: !ours };
}

function endpointOwnership(root, canonicalRoot, hostId, helperRealpath, pid) {
  const exePath = realpathSync(`/proc/${pid}/exe`);
  const argv = readFileSync(`/proc/${pid}/cmdline`, "utf8").split("\0").filter((v) => v.length > 0);
  const rootIndex = argv.indexOf("--root");
  const hostIndex = argv.indexOf("--host-id");
  return {
    exePath,
    argv,
    exeMatches: exePath === helperRealpath,
    rootMatches: rootIndex >= 0 && argv[rootIndex + 1] === canonicalRoot,
    hostMatches: hostIndex >= 0 && argv[hostIndex + 1] === hostId,
    root,
    canonicalRoot,
  };
}

/**
 * Read-only /proc scan for a daemon already running at this exact root/host/helper.
 * Used only when the endpoint record is absent: nothing is ever signalled from it.
 */
function findProcessesForRoot(canonicalRoot, hostId, helperRealpath) {
  const matches = [];
  for (const entry of readdirSync("/proc")) {
    if (!/^\d+$/.test(entry)) continue;
    const pid = Number.parseInt(entry, 10);
    let argv;
    try {
      argv = readFileSync(`/proc/${pid}/cmdline`, "utf8").split("\0").filter((v) => v.length > 0);
    } catch {
      continue;
    }
    const rootIndex = argv.indexOf("--root");
    const hostIndex = argv.indexOf("--host-id");
    if (rootIndex < 0 || hostIndex < 0) continue;
    if (argv[rootIndex + 1] !== canonicalRoot || argv[hostIndex + 1] !== hostId) continue;
    try {
      if (realpathSync(`/proc/${pid}/exe`) !== helperRealpath) continue;
    } catch {
      continue;
    }
    matches.push(pid);
  }
  return matches;
}

async function cleanupOwnedRoot({ root, canonicalRoot, hostId, helperRealpath }) {
  const report = {
    event: "cleanup",
    root,
    canonicalRoot,
    pid: null,
    skipped: false,
    exeMatches: false,
    rootMatches: false,
    hostMatches: false,
    childProcesses: null,
    sessions: null,
    kill: "skipped",
    socketClose: "not-observed",
    socketError: null,
    procState: null,
    pidReused: false,
    processGone: false,
    verified: false,
    error: null,
  };

  let endpoint;
  try {
    endpoint = readEndpoint(root);
    report.pid = endpoint.pid;
  } catch (error) {
    if (error.code === "ENOENT") {
      // No endpoint record: this root never started a runtime, so there is nothing
      // to kill. A read-only scan still proves no matching daemon leaked.
      const orphans = findProcessesForRoot(canonicalRoot, hostId, helperRealpath);
      report.skipped = true;
      report.kill = "not-needed";
      report.verified = orphans.length === 0;
      report.note = "no endpoint record: nothing was started at this root";
      if (orphans.length > 0) {
        report.error = `no endpoint record but matching daemon(s) still run: ${orphans.join(",")}`;
      }
      emit(report);
      return report;
    }
    report.error = `endpoint unreadable, nothing killed: ${error.message}`;
    emit(report);
    return report;
  }

  let transport = null;
  try {
    try {
      const owner = endpointOwnership(root, canonicalRoot, hostId, helperRealpath, endpoint.pid);
      report.exeMatches = owner.exeMatches;
      report.rootMatches = owner.rootMatches;
      report.hostMatches = owner.hostMatches;
      report.argv = owner.argv;
    } catch (error) {
      report.error = `ownership validation failed, nothing killed: ${error.message}`;
      emit(report);
      return report;
    }
    if (!(report.exeMatches && report.rootMatches && report.hostMatches)) {
      report.error = "process identity does not match this root/host/helper, nothing killed";
      emit(report);
      return report;
    }

    let children = [];
    try {
      children = childrenOf(endpoint.pid);
    } catch (error) {
      report.error = `child enumeration failed, nothing killed: ${error.message}`;
      emit(report);
      return report;
    }
    report.childProcesses = children.length;

    const connected = await connectEndpoint(root, endpoint, `cleanup ${root}`);
    transport = connected.transport;
    const sessions = await listSessions(transport, endpoint.token, transport.label);
    report.sessions = sessions.length;

    if (report.childProcesses !== 0 || report.sessions !== 0) {
      report.error = "live children or sessions remain, nothing killed";
      emit(report);
      return report;
    }

    // Subscribe to the socket close event before signalling, then await it.
    const closed = new Promise((res) => {
      transport.socket.once("close", () => res("closed"));
      const onError = () => {
        report.socketError = transport.socketError;
        res("error");
      };
      transport.socket.once("error", onError);
    });

    process.kill(endpoint.pid, "SIGTERM");
    report.kill = "sent";
    await bounded(closed, `cleanup ${root}: socket close`, CLOSE_TIMEOUT_MS);
    report.socketClose = "observed";

    const termination = terminationState(endpoint.pid, canonicalRoot, hostId, helperRealpath);
    report.procState = termination.state;
    report.pidReused = termination.pidReused;
    report.processGone = termination.gone;
    report.verified = report.processGone;
    if (!report.processGone) {
      report.error = `pid ${endpoint.pid} still alive after SIGTERM and socket close`;
    }
  } catch (error) {
    report.error = `cleanup failed: ${error.message}`;
  } finally {
    if (transport) transport.destroy();
  }

  emit(report);
  return report;
}

async function main() {
  const [helperArg, rootBaseArg, ...extra] = process.argv.slice(2);
  if (extra.length > 0) throw new Error(`unexpected extra arguments: ${extra.join(" ")}`);
  if (!helperArg || !rootBaseArg) {
    throw new Error("usage: ssh-helper-root-coexist.mjs <helper-executable> <private-root-base>");
  }
  if (process.platform !== "linux") {
    emit({ event: "result", ok: false, reason: `unsupported platform ${process.platform}` });
    return 2;
  }

  const helper = resolve(helperArg);
  const helperRealpath = realpathSync(helper);
  await access(helper, constants.X_OK);
  if (!statSync(helper).isFile()) throw new Error(`${helper} is not a file`);

  const rootBase = resolve(rootBaseArg);
  const baseStat = statSync(rootBase);
  if (!baseStat.isDirectory()) throw new Error(`${rootBase} is not a directory`);
  if ((baseStat.mode & 0o077) !== 0) {
    throw new Error(`${rootBase} is not private (mode ${(baseStat.mode & 0o777).toString(8)})`);
  }

  const hostId = `coexist-${randomUUID()}`;
  const runId = randomUUID();
  const rootA = join(rootBase, `rootA-${runId}`);
  const rootB = join(rootBase, `rootB-${runId}`);
  for (const root of [rootA, rootB]) {
    await mkdir(root, { recursive: true, mode: 0o700 });
    // The helper refuses a non-private root; the caller's umask must not decide this.
    chmodSync(root, 0o700);
  }

  const failures = [];
  const state = { hostId, helper, helperRealpath, roots: [rootA, rootB], runtimes: [] };

  try {
    for (const [label, root] of [
      ["A", rootA],
      ["B", rootB],
    ]) {
      const stderr = await startRuntime(helper, hostId, root);
      const endpoint = readEndpoint(root);
      const canonicalRoot = realpathSync(root);
      if (endpoint.address !== join(canonicalRoot, "helper.sock")) {
        throw new Error(`${root}: endpoint address ${endpoint.address} is outside the root`);
      }
      const connected = await connectEndpoint(root, endpoint, `runtime ${label}`);
      assertHandshake(connected.handshake, hostId, `runtime ${label}`);
      const sessions = await listSessions(connected.transport, endpoint.token, connected.transport.label);
      if (sessions.length !== 0) {
        throw new Error(`runtime ${label}: ${sessions.length} sessions before any spawn`);
      }
      const runtime = { label, root, canonicalRoot, endpoint, ...connected, stderr };
      state.runtimes.push(runtime);
      emit({
        event: "started",
        label,
        root: canonicalRoot,
        pid: endpoint.pid,
        helperVersion: connected.handshake.helperVersion,
        capabilities: connected.handshake.capabilities,
        unitFallbackNotice: stderr.length > 0 ? stderr : null,
      });
    }

    const [runtimeA, runtimeB] = state.runtimes;
    if (runtimeA.handshake.hostId !== runtimeB.handshake.hostId) {
      throw new Error("handshakes disagree on hostId");
    }
    if (runtimeA.handshake.helperVersion !== runtimeB.handshake.helperVersion) {
      throw new Error(
        `handshakes disagree on helperVersion: ${runtimeA.handshake.helperVersion} vs ${runtimeB.handshake.helperVersion}`
      );
    }

    const units = await unitMainPidMap();
    const unitA = units.get(runtimeA.endpoint.pid) ?? null;
    const unitB = units.get(runtimeB.endpoint.pid) ?? null;
    state.units = { unitA, unitB, pidA: runtimeA.endpoint.pid, pidB: runtimeB.endpoint.pid };
    emit({ event: "systemd", ...state.units });
    if (runtimeA.endpoint.pid === runtimeB.endpoint.pid) {
      throw new Error("both roots report the same endpoint pid");
    }
    if (!unitA || !unitB) {
      throw new Error(
        `endpoint pids are not systemd --user MainPIDs (A=${unitA ?? "none"}, B=${unitB ?? "none"}): direct-spawn fallback in use`
      );
    }
    if (unitA === unitB) {
      throw new Error(`both roots share unit ${unitA}, coexistence is not proven`);
    }

    await startRuntime(helper, hostId, rootA);
    const endpointAfter = readEndpoint(rootA);
    const handshakeAfter = await call(runtimeA.transport, runtimeA.endpoint.token, "handshake", {});
    state.idempotent = {
      pidUnchanged: endpointAfter.pid === runtimeA.endpoint.pid,
      epochUnchanged: JSON.stringify(handshakeAfter.epoch) === JSON.stringify(runtimeA.handshake.epoch),
      observedPid: endpointAfter.pid,
    };
    emit({ event: "idempotent", root: runtimeA.canonicalRoot, ...state.idempotent });
    if (!state.idempotent.pidUnchanged) {
      throw new Error(
        `repeated start changed the pid: ${runtimeA.endpoint.pid} -> ${endpointAfter.pid}`
      );
    }
    if (!state.idempotent.epochUnchanged) {
      throw new Error("repeated start changed the runtime epoch");
    }
  } catch (error) {
    failures.push(error.message);
  }

  const cleanup = [];
  for (const root of [rootA, rootB]) {
    cleanup.push(
      await cleanupOwnedRoot({
        root,
        canonicalRoot: realpathSync(root),
        hostId,
        helperRealpath,
      })
    );
  }
  for (const runtime of state.runtimes) runtime.transport.destroy();

  const cleanupFailures = cleanup.filter((report) => !report.verified).map((report) => ({
    root: report.root,
    pid: report.pid,
    error: report.error,
  }));
  const ok = failures.length === 0 && cleanupFailures.length === 0;
  emit({
    event: "result",
    ok,
    hostId,
    helper,
    helperRealpath,
    roots: state.roots,
    units: state.units ?? null,
    idempotent: state.idempotent ?? null,
    handshakes: state.runtimes.map((runtime) => ({
      root: runtime.canonicalRoot,
      pid: runtime.endpoint.pid,
      hostId: runtime.handshake.hostId,
      helperVersion: runtime.handshake.helperVersion,
      capabilities: runtime.handshake.capabilities,
      epoch: runtime.handshake.epoch ?? null,
    })),
    failures,
    cleanup,
    cleanupFailures,
  });
  return ok ? 0 : 1;
}

const code = await main().catch((error) => {
  emit({ event: "result", ok: false, fatal: error.message });
  return 1;
});
process.exit(code);
