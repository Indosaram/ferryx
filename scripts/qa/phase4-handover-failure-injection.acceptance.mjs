#!/usr/bin/env node
/**
 * Phase 4 acceptance: handover failure injection, asserted through the real request and ownership
 * gates and through the real attach path.
 *
 * Source of truth: `.omo/plans/local-pty-crash-survival-20261116.md` (rev 5), Phase 4, lines 248-263:
 *
 *   (a) successor dies BEFORE the decision -> the predecessor is still alive, must still serve, and a
 *       retried handover commits;
 *   (b) successor dies AFTER the decision  -> the predecessor cannot resume serving and cannot read
 *       or write the transferred PTYs;
 *   (c) predecessor dies before the decision -> the handover aborts, authority stays with the dead
 *       predecessor's durable state, and the successor must NOT adopt the PTYs. Direct-owned sessions
 *       are lost; keeper-owned sessions must stay discoverable and recoverable through the keeper.
 *
 * Not a record-only check. Every case drives the production wire:
 *
 *   daemon    NDJSON on `<FERRYX_RUNTIME_DIR>/daemon.sock`: `handshake` (v6), `spawn`,
 *             `describeLiveSessions`, `attach`, `write`, `prepareHandover`, `abortHandover`
 *             (`src-tauri/src/daemon/protocol.rs`).
 *   handover  the successor is a real daemon started with `--handover-from`, so it runs the production
 *             candidate path: `ListSessions` -> `DescribeSession` -> `TransferSessions` -> authority
 *             handoff -> `CommitHandover` (`src-tauri/src/daemon/server.rs`).
 *   keeper    `crates/ferryx-ptykeeper`: 60-byte `FXKP` header, JSON payload, base64 byte fields, on
 *             `<FERRYX_RUNTIME_DIR>/keeper.sock` (`crates/ferryx-ptykeeper/src/protocol.rs`).
 *
 * Fault injection is a relay in front of the predecessor's legacy socket, the same shape as
 * `run_delivery_shortfall_case` in `src-tauri/tests/daemon_handover_transfer_contract.rs`. The
 * successor gets `--handover-from <relay socket>`; the relay forwards NDJSON verbatim until the
 * candidate writes `commitHandover`, and that line is the injection point:
 *
 *   (a) kill the successor, do not forward        -> the predecessor never reaches `record_decision`
 *   (b) no injection at all                         -> the handover runs production end to end; the
 *                                                     successor is killed only once `daemon.sock`
 *                                                     answers as it, which is strictly after the
 *                                                     durable decision
 *   (c) kill the predecessor, do not forward       -> the predecessor dies holding its own authority
 *
 * No fixed sleep decides any outcome. Our own processes are awaited on their `exit` event (SIGCHLD,
 * not a poll) the way `worktree/git/drain.rs::subscribe` awaits a pid on Darwin with
 * `EVFILT_PROC`/`NOTE_EXIT`. A PTY child is not our child and has no such event, so a bounded
 * `until()` probe over `ps -o ... -p <pid>` decides whether it is gone - the Phase 3 idiom. The
 * deadline, not elapsed time, is what makes that result meaningful.
 *
 * Isolation, asserted before anything is spawned and not relaxable by any flag:
 *   FERRYX_RUNTIME_DIR, FERRYX_LEGACY_RUNTIME_DIR, FERRYX_DATA_DIR, FERRYX_SESSION_DIR,
 *   FERRYX_KEEPER_DIR, FERRYX_DAEMON_PLIST_PATH, FERRYX_PTY_KEEPER_PLIST_PATH
 * all inside one owned temp root, none of them `/tmp/ferryx-<uid>` or under `$HOME/.ferryx`, runtime
 * and legacy roots non-overlapping, every unix socket path inside the 103-byte `sun_path` budget
 * (computed, not assumed), and the keeper plist on the QA label only. Children get a scrubbed
 * environment: no inherited `FERRYX_*`, so `supervisor::supported()` can never see a production
 * supervisor name and treat this runtime as managed.
 *
 * Usage:
 *   node scripts/qa/phase4-handover-failure-injection.acceptance.mjs --list
 *   node scripts/qa/phase4-handover-failure-injection.acceptance.mjs \
 *     --daemon-binary src-tauri/target/release/ferryx \
 *     [--keeper-binary crates/ferryx-ptykeeper/target/release/ferryx-ptykeeper] \
 *     [--case <id>]... [--root <dir>] [--keep] [--json] [--allow-partial]
 *
 * Exit codes: 0 exactly the gate's five cases ran and all passed; 1 at least one failed or was
 * blocked, or the selection was not exactly those five; 2 usage error, coverage drift, or an
 * isolation refusal - all raised before the first process is spawned.
 */

import { spawn } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { once } from 'node:events';
import { watch } from 'node:fs';
import { appendFile, chmod, mkdir, mkdtemp, readdir, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { homedir, tmpdir, userInfo } from 'node:os';
import net from 'node:net';
import { basename, dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { KEEPER_BINARY_NAME, isBundledDaemon, keeperPayloadDirFor, stageAppBundle } from '../lib/keeper-bundle.mjs';

/** This checkout, so the default binary locations are this tree's targets. */
const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

// -------------------------------------------------------------------------------------------
// Constants pinned to real interfaces. Each names the item it mirrors; drift is a harness bug.
// -------------------------------------------------------------------------------------------

/** `daemon/protocol.rs`: DAEMON_PROTOCOL_VERSION under default features. */
const DAEMON_PROTOCOL_VERSION = 6;
/** `endpoint.rs` / `handover_socket.rs`: macOS `sun_path` budget, NUL excluded. */
const MAX_SOCKET_PATH_BYTES = 103;
/** `handover_socket.rs::get_handover_socket_path`: `handover-<uuid>.sock` = 50 bytes, exact. */
const HANDOVER_SOCKET_BYTES = 'handover-'.length + 36 + '.sock'.length;
/** `handover.rs::generate_legacy_socket_path`: `legacy-<pid>-<millis>[-<seq>].sock`, generous bound. */
const LEGACY_NAME_BYTES = 'legacy-'.length + 6 + 1 + 13 + 1 + 8 + '.sock'.length;
/** This harness's own relay socket, which stands in for the predecessor's legacy socket. */
const RELAY_SOCKET_NAME = 'legacy.sock';
/** `keeper_launchd.rs`: QA_PLIST_LABEL. */
const QA_KEEPER_LABEL = 'com.ferryx.qa.ptykeeper';
/** The production label. This harness never writes, loads, or unloads it. */
const PRODUCTION_KEEPER_LABEL = 'com.ferryx.ptykeeper';
/** `crash_supervisor.rs`: KEEPER_SOCKET_NAME. */
const KEEPER_SOCKET_NAME = 'keeper.sock';
/** `crates/ferryx-ptykeeper`: MAGIC / HEADER_BYTES / FRAMING_VERSION / MAX_FRAME_PAYLOAD_BYTES. */
const KEEPER_MAGIC = Buffer.from('FXKP', 'ascii');
const KEEPER_HEADER_BYTES = 60;
const KEEPER_FRAMING_VERSION = 1;
const KEEPER_MAX_PAYLOAD_BYTES = 1024 * 1024;
/** `protocol.rs`: NO_REQUEST_ID. */
const KEEPER_NO_REQUEST_ID = '-'.repeat(36);
/** `keeper.rs`: MAX_WRITE_BYTES. */
const KEEPER_MAX_WRITE_BYTES = 64 * 1024;
/**
 * `keeper_startup.rs::KEEPER_OWNERSHIP_POLICY_ENV`: the startup surface that pins new-session
 * routing. Case (c) needs it because its premise is that one fixture is *direct*-owned: a daemon
 * whose startup pass enabled keeper ownership routes every spawn to the keeper, so the "direct"
 * fixture would be keeper-owned and its child would legitimately survive the predecessor's death.
 */
const KEEPER_OWNERSHIP_ENV = 'FERRYX_KEEPER_OWNERSHIP';
/** `keeper_startup.rs::OwnershipPolicy::parse`: every spelling the daemon accepts. */
const OWNERSHIP_POLICY_VALUES = Object.freeze([
  'enabled',
  'enable',
  'on',
  'true',
  '1',
  'disabled',
  'disable',
  'off',
  'false',
  '0',
  'unchanged',
  'auto',
]);

/**
 * `handover_transaction.rs`: `DecisionAccounting` serde values (`rename_all = "snake_case"`). Spelled
 * out rather than imported because this file speaks the wire, not the crate.
 */
const DECISION_ACCOUNTING = Object.freeze({
  SUCCESSOR_IDENTITY_UNUSED: 'successor_identity_unused',
  SESSION_INVENTORY_UNUSED: 'session_inventory_unused',
  SUCCESSOR_IDENTITY_AND_INVENTORY_UNUSED: 'successor_identity_and_inventory_unused',
});
const DECISION_ACCOUNTING_VALUES = Object.freeze(Object.values(DECISION_ACCOUNTING));
/**
 * `handover.rs::DECISION_ACCOUNTING_UNPOPULATED`: the typed warning a decision record emits when its
 * successor identity or inventory is absent by construction. Plan Phase 4 requires that warning, so
 * this harness reads it out of the isolated daemon log instead of assuming it was emitted.
 */
const DECISION_ACCOUNTING_EVENT = 'handover.decision_accounting_unpopulated';
/** `HandoverManager::record_decision(false)`: the rollback reason an abort always writes. */
const ABORT_ROLLBACK_REASON = 'predecessor abort decision';

/**
 * Plan rev 5 lines 248-263: the cases a Phase 4 gate run must cover. This list is the harness's own
 * copy of the gate contract, kept equal to `PHASE4_REQUIRED_CASES` in
 * `scripts/qa/combined-release-gate.mjs`, and checked against the real case table below, so a case
 * that stops existing cannot silently shrink what "accepted" means.
 */
const GATE_REQUIRED_CASES = Object.freeze([
  'a-successor-dies-before-decision',
  'b-successor-dies-after-decision',
  'c-predecessor-dies-before-decision-direct',
  'c-keeper-owned-session-survives-predecessor-death',
  'c-keeper-session-adopted-by-daemon',
]);
/** `logging.rs::logs_dir`: `<FERRYX_DATA_DIR>/logs/daemon.log`, inside this run's owned root. */
const DAEMON_LOG_NAME = 'daemon.log';

/** `protocol.rs`: Kind discriminants. Never renumbered, so they are spelled out here. */
const KIND = Object.freeze({
  Hello: 0x0001,
  HelloAck: 0x0002,
  Spawn: 0x0010,
  SpawnAck: 0x0011,
  Write: 0x0020,
  Resize: 0x0030,
  Signal: 0x0031,
  Kill: 0x0040,
  Attach: 0x0050,
  AttachAck: 0x0051,
  Output: 0x0052,
  Head: 0x0053,
  HeadAck: 0x0054,
  Replay: 0x0060,
  ReplayChunk: 0x0061,
  ReplayEnd: 0x0062,
  ReplayGapEvent: 0x0063,
  OwnershipQuery: 0x0070,
  OwnershipList: 0x0071,
  Detach: 0x0090,
  Error: 0x00f0,
  Shutdown: 0x00f1,
});
const KIND_NAME = new Map(Object.entries(KIND).map(([name, value]) => [value, name]));

/**
 * Typed gaps: a seam the plan requires that the tree does not have yet. A gap is never a pass.
 */
const GAP = Object.freeze({
  DAEMON_BINARY_ABSENT: 'no daemon binary was found, so no handover can be exercised at all',
  KEEPER_BINARY_ABSENT: 'no keeper binary was found, so the keeper half of case (c) cannot run',
  KEEPER_START_FAILED: 'the keeper binary exists but never bound its socket',
  PLATFORM_UNSUPPORTED: 'daemon handover and the keeper transport are unix only',
  HELPER_ABSENT: 'an OS helper the case needs (`ps`) is unavailable',
  RUNTIME_SOCKET_PATH_TOO_LONG: 'the isolated root exceeds the unix socket path budget',
  LOG_WATCH_UNAVAILABLE:
    'the isolated daemon log cannot be subscribed to, so the accounting warning cannot be ordered against the decision that should have emitted it',
  ACCOUNTING_BARRIER_ABSENT:
    'a decision record carrying every field owes no warning, and absence is only assertable after a log record the daemon emits strictly past the decision',
  KEEPER_DAEMON_ADOPTION_ABSENT:
    'the daemon spawned a session through its own PTY manager and the keeper never listed it, so no spawn reached the keeper: either the startup pass did not activate keeper ownership, or the spawn path does not consult the routing flag. The payload gate is reported separately (`keeper_startup::ensure_keeper` derives its payload from current_exe() through bundle_keeper_dir()), and this harness stages the daemon into Contents/MacOS with Contents/Resources/keeper, so a payload refusal here would name the staged path the daemon actually ran from',
});

const FAIL = Object.freeze({
  USAGE: 'a required argument is missing or malformed',
  DEADLINE: 'a bounded wait expired before its condition held',
  PROTOCOL: 'a wire frame did not match the protocol this harness speaks',
  REFUSED: 'the peer answered with a typed refusal the case did not expect',
  SAFETY: 'an operation would have left the isolation contract',
  IDENTITY_MISMATCH: 'a process identity changed across the handover boundary',
  DECISION_UNEXPECTED: 'the durable decision record is not in the state the case requires',
  DECISION_TOO_EARLY: 'a durable decision existed before the successor reached CommitHandover',
  ADOPTION_WITHOUT_DECISION: 'a successor adopted the PTYs without a committed decision authorising it',
  SERVE_PROOF_MISSING: 'the predecessor could not prove it was still serving its session',
  ACCOUNTING_CONTRADICTS_RECORD:
    'the decision record declares an accounting that its own successor identity or inventory contradicts',
  ACCOUNTING_UNDECLARED:
    'the decision record leaves its successor identity or inventory unpopulated without declaring that by construction',
  ACCOUNTING_UNKNOWN: 'the decision record carries an accounting variant this harness does not know',
  RECORD_INVENTORY_INCOMPLETE:
    'the decision record does not name the session the decision was about, so it is not accounting for the transfer that happened',
  ACCOUNTING_WARNING_ABSENT:
    'the daemon wrote an unpopulated decision record but emitted no handover.decision_accounting_unpopulated warning',
  PTY_SURVIVED_LOSS: 'a PTY whose last owner died is still running, so it was never really transferred',
  DIRECT_SESSION_NOT_LOST:
    'a direct-owned session outlived the predecessor whose authority it depended on, so it is now an ownerless PTY',
  LEAKED_PROCESS: 'a process this case started was still alive after its owner was gone',
  COVERAGE_DRIFT:
    'the case table and the coverage contract disagree, so a clause could be reported as covered by a case that does not exist',
  PARTIAL_SELECTION:
    'the selection does not cover the plan gate, so a pass would be a claim about cases that never ran',
});

class AcceptanceError extends Error {
  constructor(kind, code, detail, extra) {
    super(`${kind} ${code}: ${detail}`);
    this.kind = kind;
    this.code = code;
    this.detail = detail;
    this.extra = extra ?? {};
  }
}

const fail = (code, detail, extra) => new AcceptanceError('FAIL', code, detail, extra);
const gap = (code, detail) => new AcceptanceError('GAP', code, detail ?? GAP[code]);

// -------------------------------------------------------------------------------------------
// Utilities
// -------------------------------------------------------------------------------------------

const sha256 = (buffer) => createHash('sha256').update(buffer).digest('hex');
const b64 = (buffer) => Buffer.from(buffer).toString('base64');
const unb64 = (text) => Buffer.from(typeof text === 'string' ? text : '', 'base64');
const nowIso = () => new Date().toISOString();
const sleep = (ms) => new Promise((tick) => setTimeout(tick, ms));

/** Bounded probe: only the deadline decides, never elapsed time. */
async function until(label, timeoutMs, probe, intervalMs) {
  const deadline = Date.now() + timeoutMs;
  const step = intervalMs ?? 25;
  for (;;) {
    const value = await probe();
    if (value) return value;
    if (Date.now() >= deadline) {
      throw fail(FAIL.DEADLINE, `${label}: condition never held within ${timeoutMs}ms`);
    }
    await sleep(step);
  }
}

/** True when `candidate` is strictly inside `root`. */
function isUnder(root, candidate) {
  if (!isAbsolute(root) || !isAbsolute(candidate)) return false;
  const delta = relative(root, candidate);
  return delta !== '' && delta !== '..' && !delta.startsWith('../') && !isAbsolute(delta);
}

const overlaps = (a, b) => a === b || isUnder(a, b) || isUnder(b, a);

function tryParseLine(line) {
  try {
    return JSON.parse(line);
  } catch {
    return null;
  }
}

async function exists(path) {
  try {
    await stat(path);
    return true;
  } catch {
    return false;
  }
}

/** Bounds a promise without leaving a dangling timer. */
async function bounded(promise, label, timeoutMs) {
  let timer = null;
  try {
    return await Promise.race([
      promise,
      new Promise((_resolve, reject) => {
        timer = setTimeout(() => reject(fail(FAIL.DEADLINE, `${label}: exceeded ${timeoutMs}ms`)), timeoutMs);
      }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}

/**
 * The isolated layout. `mkdtemp` gives a root nobody else can name, and every path a child sees is
 * derived from it, so a mistake can only ever point inside it.
 *
 * The root is short on purpose: `/tmp/fx4-XXXXXX` leaves room for `handover-<uuid>.sock` inside the
 * 103-byte `sun_path` budget, which `mkdtemp(tmpdir())` does not (`/var/folders/.../T` alone is 48
 * bytes before any component). `assertIsolation` measures that instead of trusting this comment.
 */
function planPaths(root) {
  const runtimeDir = join(root, 'rt');
  return {
    root,
    runtimeDir,
    legacyRuntimeDir: join(root, 'lrt'),
    dataDir: join(root, 'data'),
    sessionDir: join(root, 'sess'),
    keeperDir: join(root, 'kpr'),
    relayDir: join(root, 'relay'),
    // The staged app bundle this case's daemon runs from. Inside the case root, so the isolation
    // contract covers the executable as well as the runtime it is given.
    appDir: join(root, 'app'),
    // `write_keeper_launchd_plist` creates `$HOME/Library/Logs/Ferryx`; an inherited real `HOME`
    // would put that outside the fixture the moment the install pass runs.
    homeDir: join(root, 'home'),
    evidenceDir: join(root, 'ev'),
    keeperSocketPath: join(runtimeDir, KEEPER_SOCKET_NAME),
    keeperPlistPath: join(root, 'kpr', `${QA_KEEPER_LABEL}.plist`),
    daemonPlistPath: join(root, 'kpr', 'daemon.plist'),
    daemonSocketPath: join(runtimeDir, 'daemon.sock'),
    relayPath: join(root, 'relay', RELAY_SOCKET_NAME),
    repo: REPO_ROOT,
  };
}

// -------------------------------------------------------------------------------------------
// Isolation
// -------------------------------------------------------------------------------------------

/**
 * Evaluated on resolved paths only, and not relaxable by any flag: an unisolated run would put the
 * user's real daemon, keeper or sessions within reach, which is the one thing this harness must
 * never do. Called before anything is spawned.
 */
function assertIsolation(paths) {
  const uid = typeof process.getuid === 'function' ? process.getuid() : userInfo().uid;
  const home = homedir();
  const production = [
    `/tmp/ferryx-${uid}`,
    `/private/tmp/ferryx-${uid}`,
    `/tmp/rorca-${uid}`,
    `/private/tmp/rorca-${uid}`,
    join(home, '.ferryx'),
  ];

  const requireOwned = (label, dir) => {
    if (!dir || !isAbsolute(dir)) {
      throw fail(FAIL.SAFETY, `${label} must be an absolute path, got ${JSON.stringify(dir)}`, {
        guard: 'absolute',
      });
    }
    for (const banned of production) {
      if (dir === banned || isUnder(banned, dir)) {
        throw fail(FAIL.SAFETY, `${label} ${dir} is a production location`, { guard: 'production-location' });
      }
    }
    if (!isUnder(paths.root, dir)) {
      throw fail(FAIL.SAFETY, `${label} ${dir} is not under the owned root ${paths.root}`, {
        guard: 'root-containment',
      });
    }
  };

  for (const [label, dir] of [
    ['FERRYX_RUNTIME_DIR', paths.runtimeDir],
    ['FERRYX_LEGACY_RUNTIME_DIR', paths.legacyRuntimeDir],
    ['FERRYX_DATA_DIR', paths.dataDir],
    ['FERRYX_SESSION_DIR', paths.sessionDir],
    ['FERRYX_KEEPER_DIR', paths.keeperDir],
    ['relay directory', paths.relayDir],
    ['FERRYX_DAEMON_PLIST_PATH', paths.daemonPlistPath],
    ['FERRYX_PTY_KEEPER_PLIST_PATH', paths.keeperPlistPath],
    ['HOME', paths.homeDir],
    ['staged app bundle', paths.appDir],
  ]) {
    requireOwned(label, dir);
  }

  // Mirrors `server::transport_qa_isolated_profile`: distinct, non-nested, absolute. The two roots
  // must not overlap, or a "legacy" socket could land in the runtime dir and be mistaken for a
  // current one (`cross_rename` would also silently turn false).
  if (overlaps(paths.runtimeDir, paths.legacyRuntimeDir)) {
    throw fail(FAIL.SAFETY, `${paths.runtimeDir} and ${paths.legacyRuntimeDir} overlap`, {
      guard: 'runtime-legacy-overlap',
    });
  }

  // `HandoverSocketListener::bind` and `generate_legacy_socket_path` both fail with ENAMETOOLONG
  // past `sun_path`, and that failure would read as a product defect. Refuse it up front, measuring
  // the longest name each can produce in the directories this run will actually use.
  const worstCase = [
    ['keeper', paths.keeperSocketPath],
    ['handover', join(paths.runtimeDir, `handover-${randomUUID()}.sock`)],
    // `generate_legacy_socket_path` mints `legacy-<pid>-<millis>[-<seq>].sock`; this is that name at
    // the widest each field can get, which is the longest path a predecessor can make in the runtime.
    ['legacy', join(paths.runtimeDir, `legacy-${'9'.repeat(6)}-${'9'.repeat(13)}-${'9'.repeat(8)}.sock`)],
    ['relay', paths.relayPath],
  ];
  const legacyNameBytes = Buffer.byteLength(basename(worstCase[2][1]));
  if (legacyNameBytes !== LEGACY_NAME_BYTES) {
    throw fail(
      FAIL.SAFETY,
      `the legacy socket name is ${legacyNameBytes} bytes, but the tree's widest is ${LEGACY_NAME_BYTES}`,
      { guard: 'socket-name-drift' },
    );
  }
  const sockets = worstCase.map(([name, path]) => ({ name, path, bytes: Buffer.byteLength(path) }));
  for (const socket of sockets) {
    if (socket.bytes > MAX_SOCKET_PATH_BYTES) {
      throw fail(
        FAIL.SAFETY,
        `the ${socket.name} socket path ${socket.path} is ${socket.bytes} bytes, over the ${MAX_SOCKET_PATH_BYTES} byte unix socket limit`,
        { guard: 'socket-budget', code: GAP.RUNTIME_SOCKET_PATH_TOO_LONG },
      );
    }
  }
  const handoverBytes = sockets.find((socket) => socket.name === 'handover').bytes;
  if (handoverBytes !== Buffer.byteLength(paths.runtimeDir) + 1 + HANDOVER_SOCKET_BYTES) {
    throw fail(FAIL.SAFETY, 'the handover socket budget does not match the name the tree generates', {
      guard: 'socket-name-drift',
    });
  }

  // The production label is checked first: a plist carrying it is the one mutation this harness must
  // name exactly, and the QA-label check below would otherwise shadow it with a weaker verdict.
  if (paths.keeperPlistPath.includes(PRODUCTION_KEEPER_LABEL)) {
    throw fail(FAIL.SAFETY, `keeper plist ${paths.keeperPlistPath} names the production label`, {
      guard: 'production-label',
    });
  }
  if (basename(paths.keeperPlistPath) !== `${QA_KEEPER_LABEL}.plist`) {
    throw fail(FAIL.SAFETY, `keeper plist ${paths.keeperPlistPath} does not carry the QA label`, {
      guard: 'qa-label',
    });
  }
  return {
    label: QA_KEEPER_LABEL,
    plist: paths.keeperPlistPath,
    sockets: sockets.map(({ name, bytes }) => ({ name, bytes, budget: MAX_SOCKET_PATH_BYTES })),
  };
}

/**
 * `launchd::agent_management_supported_for(dev, isolated)` is `!dev && !isolated`, and
 * `supervisor::supported()` also needs `SUPERVISOR_NAME_ENV`. An isolated runtime with no name
 * override never manages a real unit, so `register_running_generation_for_login` returns without
 * writing anything. Recorded, never applied: this harness never invokes `launchctl`.
 */
function plannedLaunchdIdentity(paths) {
  return {
    label: QA_KEEPER_LABEL,
    plistPath: paths.keeperPlistPath,
    daemonPlistPath: paths.daemonPlistPath,
    applied: false,
    registration: 'refused by supervisor::supported() on an isolated runtime',
    note: 'label and plist paths are asserted only; launchctl is never invoked by this harness',
  };
}

/** Every child sees exactly the environment this harness builds, never an inherited production one. */
function childEnv(base, extra) {
  const scrubbed = {};
  for (const [key, value] of Object.entries(base)) {
    if (key.startsWith('FERRYX_')) continue;
    scrubbed[key] = value;
  }
  return Object.assign(scrubbed, extra);
}

// -------------------------------------------------------------------------------------------
// Evidence: persistent, raw, appended - a crashed run still leaves its trace.
// -------------------------------------------------------------------------------------------

class Evidence {
  constructor(dir) {
    this.dir = dir;
    this.framesPath = join(dir, 'frames.jsonl');
    this.casesDir = join(dir, 'cases');
  }

  async init() {
    await mkdir(this.casesDir, { recursive: true });
    await writeFile(this.framesPath, '');
  }

  async record(record) {
    await appendFile(
      this.framesPath,
      `${JSON.stringify(Object.assign({ at: nowIso() }, record))}\n`,
    ).catch(() => null);
  }

  async writeCase(id, value) {
    const path = join(this.casesDir, `${id}.json`);
    await writeFile(path, `${JSON.stringify(value, null, 2)}\n`);
    return path;
  }

  /**
   * Stores the exact bytes the product wrote, beside the run rather than inside the case report: a
   * decision record is the primary evidence for an accounting claim, so it is kept verbatim (with
   * its sha256) instead of only the fields this harness happened to parse.
   */
  async captureRaw(name, bytes) {
    const buffer = Buffer.isBuffer(bytes) ? bytes : Buffer.from(String(bytes), 'utf8');
    const path = join(this.dir, name);
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, buffer);
    return { path, bytes: buffer.length, sha256: sha256(buffer) };
  }

  /**
   * The isolated daemon log for this run: `<FERRYX_DATA_DIR>/logs/daemon.log` (`logging.rs::logs_dir`).
   * A missing log is `null`, never an error: the run may not have reached the point that writes it.
   */
  async daemonLog(dataDir) {
    const path = join(dataDir, 'logs', DAEMON_LOG_NAME);
    try {
      return { path, text: await readFile(path, 'utf8') };
    } catch (error) {
      if (error.code === 'ENOENT') return { path, text: null };
      throw error;
    }
  }

  /** Every rotated generation too, newest first, so a record rotated mid-case is still found. */
  async daemonLogRecords(dataDir) {
    const dir = join(dataDir, 'logs');
    let names;
    try {
      names = await readdir(dir);
    } catch (error) {
      if (error.code === 'ENOENT') return { paths: [], text: '' };
      throw error;
    }
    const logs = names.filter((name) => name === DAEMON_LOG_NAME || name.startsWith(`${DAEMON_LOG_NAME}.`)).sort();
    const paths = logs.map((name) => join(dir, name));
    const texts = await Promise.all(paths.map((path) => readFile(path, 'utf8').catch(() => '')));
    return { paths, text: texts.join('') };
  }
}

// -------------------------------------------------------------------------------------------
// Process identity: the acceptance subject, so it is proved against the OS, not against a claim.
// -------------------------------------------------------------------------------------------

/**
 * `ps -ax -o pid=,ppid=,lstart=` under `LC_ALL=C`. `lstart` is what Darwin exposes as a process
 * start time, compared at second granularity because that is its resolution.
 */
async function processTable() {
  const child = spawn('ps', ['-ax', '-o', 'pid=,ppid=,lstart='], {
    env: { ...process.env, LC_ALL: 'C' },
    stdio: ['ignore', 'pipe', 'ignore'],
  });
  let stdout = '';
  child.stdout.setEncoding('utf8');
  child.stdout.on('data', (chunk) => {
    stdout += chunk;
  });
  await bounded(once(child, 'close'), 'ps -ax', 20_000).catch(() => null);
  const rows = new Map();
  for (const line of stdout.split('\n')) {
    const match = /^\s*(\d+)\s+(\d+)\s+(.+?)\s*$/.exec(line);
    if (!match) continue;
    rows.set(Number(match[1]), { ppid: Number(match[2]), lstart: match[3], startedAt: Date.parse(match[3]) });
  }
  return rows;
}

const processAlive = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error.code === 'EPERM';
  }
};

/**
 * `(pid, start time)` is the identity: a pid alone is reusable, so every identity claim in this file
 * is checked against the start time the OS reports for that pid.
 *
 * The two reporters use different units, and that is not a detail to paper over:
 *   - the daemon's `localChildStartTime` is the raw Darwin start token divided by 1000, MILLIseconds
 *     (`suspension/unix.rs`);
 *   - the keeper's `processStartTime` is `pbi_start_tvsec * 1_000_000 + pbi_start_tvusec`, MICROseconds
 *     (`keeper.rs`).
 */
function startTimeToMicros(value, unit) {
  return unit === 'ms' ? Number(value) * 1000 : Number(value);
}

function assertProcessIdentity(table, pid, startTime, label, unit = 'us') {
  if (!Number.isInteger(pid) || pid <= 0) {
    throw fail(FAIL.IDENTITY_MISMATCH, `${label}: pid ${JSON.stringify(pid)} is not a process`);
  }
  if (!Number.isFinite(startTime) || startTime <= 0) {
    throw gap(
      GAP.HELPER_ABSENT,
      `${label}: the peer reported no start time for pid ${pid}, so its identity cannot be proved`,
    );
  }
  const row = table.get(pid);
  if (!row) throw fail(FAIL.IDENTITY_MISMATCH, `${label}: pid ${pid} is absent from the process table`);
  const reportedSeconds = Math.floor(startTimeToMicros(startTime, unit) / 1_000_000);
  const osSeconds = Math.floor(row.startedAt / 1000);
  if (Number.isFinite(row.startedAt) && Math.abs(reportedSeconds - osSeconds) > 1) {
    throw fail(
      FAIL.IDENTITY_MISMATCH,
      `${label}: pid ${pid} reports start ${reportedSeconds}s, the OS reports ${osSeconds}s (${row.lstart})`,
    );
  }
  return { pid, lstart: row.lstart, ppid: row.ppid, reportedSeconds };
}

/**
 * Waits, bounded, for a pid this harness does not own to disappear.
 *
 * A PTY child is not our child, so there is no SIGCHLD to await and no `exit` event to subscribe to:
 * the only observable is the pid no longer existing. This is a deadline-bounded state check, the same
 * idiom the Phase 3 harness uses for the same reason; the deadline, not elapsed time, is what makes
 * its result meaningful.
 */
async function untilGone(pid, label, timeoutMs) {
  await until(`${label}: pid ${pid} gone`, timeoutMs, () => (processAlive(pid) ? null : true));
}

// -------------------------------------------------------------------------------------------
// Daemon client: NDJSON on `daemon.sock`, protocol version 6.
// -------------------------------------------------------------------------------------------

class DaemonRefusal extends Error {
  constructor(code, message) {
    super(`daemon refused ${code ?? 'error'}: ${message}`);
    this.code = code;
    this.text = message;
  }
}

/**
 * Line-delimited JSON with an ordered frame queue. Replies travel on the same pipe as stream frames,
 * so ordering on one dedicated connection is what makes "the next line is my answer" true.
 */
class LineChannel {
  constructor(socket) {
    this.socket = socket;
    this.buffer = '';
    this.queue = [];
    this.pending = [];
    this.closed = null;
    socket.setEncoding('utf8');
    socket.on('data', (chunk) => {
      this.buffer += chunk;
      for (;;) {
        const newline = this.buffer.indexOf('\n');
        if (newline < 0) return;
        const line = this.buffer.slice(0, newline);
        this.buffer = this.buffer.slice(newline + 1);
        if (!line.trim()) continue;
        const parsed = tryParseLine(line) ?? { type: 'unparseable', raw: line };
        const pending = this.pending.shift();
        if (pending) pending.resolve(parsed);
        else this.queue.push(parsed);
      }
    });
    socket.on('close', () => this.#failAll(new Error('daemon socket closed')));
    socket.on('error', (error) => this.#failAll(error));
  }

  #failAll(error) {
    this.closed = error;
    for (const entry of this.pending.splice(0)) entry.reject(error);
  }

  send(object) {
    this.socket.write(`${JSON.stringify(object)}\n`);
  }

  next(label, timeoutMs) {
    if (this.queue.length) return Promise.resolve(this.queue.shift());
    if (this.closed) return Promise.reject(this.closed);
    const window = timeoutMs ?? 30_000;
    return new Promise((resolveFrame, rejectFrame) => {
      const entry = {
        resolve: (frame) => {
          clearTimeout(timer);
          resolveFrame(frame);
        },
        reject: (error) => {
          clearTimeout(timer);
          rejectFrame(error);
        },
      };
      const timer = setTimeout(() => {
        const index = this.pending.indexOf(entry);
        if (index >= 0) this.pending.splice(index, 1);
        rejectFrame(fail(FAIL.DEADLINE, `${label}: no daemon frame within ${window}ms`));
      }, window);
      this.pending.push(entry);
    });
  }

  close() {
    this.socket.destroy();
  }
}

class DaemonClient {
  constructor(channel, identity, evidence) {
    this.channel = channel;
    this.identity = identity;
    this.evidence = evidence;
  }

  static async connect(socketPath, evidence) {
    const socket = net.createConnection({ path: socketPath });
    try {
      await bounded(once(socket, 'connect'), `daemon connect ${socketPath}`, 10_000);
    } catch (error) {
      // A refused or vanished endpoint is an ordinary observation here: callers probe a socket that
      // may legitimately have no owner. Destroy before rethrowing so no later `error` event lands on
      // a socket with no listener.
      socket.destroy();
      throw error;
    }
    const channel = new LineChannel(socket);
    channel.send({ type: 'handshake', version: DAEMON_PROTOCOL_VERSION });
    const identity = await channel.next('daemon handshake');
    if (identity.type === 'protocolMismatch') {
      throw fail(
        FAIL.PROTOCOL,
        `daemon expects protocol ${identity.expectedVersion}, this harness sends ${DAEMON_PROTOCOL_VERSION}`,
      );
    }
    if (identity.type !== 'handshakeOk') throw fail(FAIL.PROTOCOL, `daemon handshake answered ${identity.type}`);
    await evidence?.record({ wire: 'daemon', verb: 'handshake', pid: identity.pid, epoch: identity.epoch });
    return new DaemonClient(channel, identity, evidence);
  }

  async call(request, label, timeoutMs) {
    this.channel.send(request);
    const reply = await this.channel.next(label ?? `daemon ${request.type}`, timeoutMs);
    await this.evidence?.record({ wire: 'daemon', verb: request.type, reply: reply.type });
    if (reply.type === 'error') throw new DaemonRefusal(reply.code, reply.message);
    return reply;
  }

  async registerWorkspace(workspaceId, repoRoot) {
    const reply = await this.call({ type: 'registerWorkspace', workspaceId, repoRoot }, 'registerWorkspace', 60_000);
    if (reply.type !== 'registerWorkspaceOk') throw fail(FAIL.PROTOCOL, `registerWorkspace answered ${reply.type}`);
  }

  async spawn(options) {
    const reply = await this.call(
      {
        type: 'spawn',
        clientRequestId: `phase4-${randomUUID()}`,
        workspaceId: options.workspaceId,
        worktree: null,
        cwd: options.cwd,
        cols: options.cols ?? 80,
        rows: options.rows ?? 24,
        shell: options.shell,
        sessionId: options.sessionId,
      },
      `daemon spawn ${options.sessionId}`,
      60_000,
    );
    if (reply.type !== 'spawnOk') throw fail(FAIL.PROTOCOL, `daemon spawn answered ${reply.type}`);
    return reply;
  }

  async write(sessionId, data) {
    await this.call({ type: 'write', sessionId, data: b64(data) }, `daemon write ${sessionId}`);
  }

  async liveSessions() {
    const reply = await this.call({ type: 'describeLiveSessions' }, 'daemon describeLiveSessions');
    if (reply.type !== 'describeLiveSessionsOk') {
      throw fail(FAIL.PROTOCOL, `describeLiveSessions answered ${reply.type}`);
    }
    return reply.sessions ?? [];
  }

  /**
   * `PrepareHandover` binds a "legacy" socket and pauses nothing by itself: it returns the path a
   * candidate dials and the sessions that candidate will be offered. This is the real entry gate.
   */
  async prepareHandover() {
    const reply = await this.call({ type: 'prepareHandover' }, 'daemon prepareHandover', 60_000);
    if (reply.type === 'handoverRejected') throw new DaemonRefusal('HANDOVER_REJECTED', reply.reason);
    if (reply.type !== 'prepareHandoverOk') throw fail(FAIL.PROTOCOL, `prepareHandover answered ${reply.type}`);
    await this.evidence?.record({
      wire: 'daemon',
      verb: 'prepareHandover',
      legacySocketPath: reply.legacySocketPath,
      activeSessions: reply.activeSessions,
    });
    return { legacySocketPath: reply.legacySocketPath, activeSessions: reply.activeSessions ?? [] };
  }

  /**
   * The documented resume gate: `resume_paused_readers()` runs only from this handler, so a
   * predecessor whose exported readers were paused serves nothing until this succeeds. It also
   * requires `HandoverStatus::Prepared`, so an accepted abort is itself proof the predecessor
   * stayed in the pre-decision state.
   */
  async abortHandover() {
    const reply = await this.call({ type: 'abortHandover' }, 'daemon abortHandover', 60_000);
    if (reply.type !== 'abortHandoverOk') throw fail(FAIL.PROTOCOL, `abortHandover answered ${reply.type}`);
  }

  /**
   * `attach` converts this connection into a stream: one `attachOk` with the retained history, then
   * live `output` frames on the same socket.
   */
  async attach(sessionId, afterSequence) {
    this.channel.send({
      type: 'attach',
      sessionId,
      afterSequence: afterSequence === null || afterSequence === undefined ? null : afterSequence,
    });
    const first = await this.channel.next(`daemon attach ${sessionId}`);
    if (first.type === 'error') throw new DaemonRefusal(first.code, first.message);
    if (first.type !== 'attachOk') throw fail(FAIL.PROTOCOL, `daemon attach answered ${first.type}`);
    const history = unb64(first.history);
    await this.evidence?.record({
      wire: 'daemon',
      verb: 'attach',
      sessionId,
      epoch: first.epoch,
      startSequence: first.startSequence ?? null,
      endSequence: first.endSequence ?? null,
      gap: first.gap ?? null,
      historyBytes: history.length,
      historySha256: sha256(history),
    });
    return {
      sessionId,
      epoch: Number(first.epoch),
      startSequence: first.startSequence == null ? null : Number(first.startSequence),
      endSequence: first.endSequence == null ? null : Number(first.endSequence),
      gap: first.gap ?? null,
      ptyCols: first.ptyCols ?? null,
      ptyRows: first.ptyRows ?? null,
      history,
      stream: this.channel,
    };
  }

  /** `DescribeSession`: the only wire that reports `readerPaused`, i.e. the export freeze. */
  async describeSession(sessionId) {
    const reply = await this.call({ type: 'describeSession', sessionId }, `daemon describeSession ${sessionId}`);
    if (reply.type !== 'describeSessionOk') throw fail(FAIL.PROTOCOL, `describeSession answered ${reply.type}`);
    return reply.session ?? {};
  }

  close() {
    this.channel.close();
  }
}

/**
 * Consumes an attached daemon stream: collects output and demultiplexes the frames that are not
 * bytes. Attaching happens before any input is written, so a marker asserted later cannot have been
 * missed in the window between the snapshot and the first read.
 */
class AttachedSession {
  constructor(attachment, evidence) {
    this.attachment = attachment;
    this.evidence = evidence;
    this.chunks = [];
    this.other = [];
    this.ended = null;
    this.pump = this.#run();
  }

  async #run() {
    for (;;) {
      let frame;
      try {
        frame = await this.attachment.stream.next(`daemon stream ${this.attachment.sessionId}`);
      } catch (error) {
        this.ended = error;
        return;
      }
      if (frame.type === 'output') {
        const data = unb64(frame.data);
        this.chunks.push({ sequence: Number(frame.sequence), data });
        await this.evidence?.record({
          wire: 'daemon',
          verb: 'output',
          sessionId: this.attachment.sessionId,
          sequence: Number(frame.sequence),
          bytes: data.length,
          sha256: sha256(data),
        });
        continue;
      }
      this.other.push(frame);
    }
  }

  get bytes() {
    return Buffer.concat(this.chunks.map((chunk) => chunk.data));
  }

  // No `write` here on purpose. An attached stream is output-only: the daemon reads those bytes just
  // to notice a disconnect and terminates the stream on any client data ("Unexpected client data on
  // attach stream", `daemon/server.rs`), which is why the real client keeps the attach half as a
  // keepalive and sends input through `DaemonRequest::Write` on a control connection.

  async waitFor(label, timeoutMs, predicate) {
    return until(label, timeoutMs, () => (predicate(this.bytes, this.chunks) ? this.bytes : null));
  }
}

// -------------------------------------------------------------------------------------------
// Keeper client: a faithful re-implementation of the 60-byte FXKP framing.
//
// Only the verbs case (c)'s keeper half needs are implemented. Each one is the real wire: the keeper
// binary is a separate process, so a spawn here creates a real PTY holding a real child, and a
// predecessor's death cannot touch it.
// -------------------------------------------------------------------------------------------

class KeeperRefusal extends Error {
  constructor(code, message, offset) {
    super(`keeper refused ${code}: ${message}`);
    this.code = code;
    this.text = message;
    this.offset = offset;
  }
}

class KeeperClient {
  constructor(socket, evidence) {
    this.socket = socket;
    this.evidence = evidence;
    this.buffer = Buffer.alloc(0);
    this.sequence = 0;
    /** Per correlation id: queued frames plus at most one waiting reader. */
    this.pending = new Map();
    /** Frames the keeper initiated: `OUTPUT` live push, a lossy reset, a `SHUTDOWN` notice. */
    this.unsolicited = [];
    /** Newest epoch this client presented per session; `KILL` must present the fenced one. */
    this.epochs = new Map();
    this.closed = null;
    socket.on('data', (chunk) => this.#onData(chunk));
    // The keeper closing mid-request is a product failure, not a harness bug: raising it as a typed
    // protocol failure keeps a keeper crash from being reported as our own defect.
    socket.on('close', () => this.#failAll(fail(FAIL.PROTOCOL, 'the keeper closed the connection before answering')));
    socket.on('error', (error) => this.#failAll(error));
  }

  static async connect(socketPath, evidence) {
    const socket = net.createConnection({ path: socketPath });
    await bounded(once(socket, 'connect'), `keeper connect ${socketPath}`, 10_000);
    socket.setNoDelay(true);
    return new KeeperClient(socket, evidence);
  }

  #failAll(error) {
    this.closed = error;
    for (const entry of this.pending.values()) {
      entry.waiter?.reject(error);
      entry.waiter = null;
    }
    this.pending.clear();
  }

  #onData(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    for (;;) {
      if (this.closed) return;
      if (this.buffer.length < KEEPER_HEADER_BYTES) return;
      if (!this.buffer.subarray(0, 4).equals(KEEPER_MAGIC)) {
        this.#failAll(fail(FAIL.PROTOCOL, `bad keeper magic ${this.buffer.subarray(0, 4).toString('hex')}`));
        return;
      }
      const version = this.buffer.readUInt16BE(4);
      if (version !== KEEPER_FRAMING_VERSION) {
        this.#failAll(
          fail(FAIL.PROTOCOL, `keeper framing version ${version}, this harness speaks ${KEEPER_FRAMING_VERSION}`),
        );
        return;
      }
      const kind = this.buffer.readUInt16BE(6);
      const payloadLength = this.buffer.readUInt32BE(8);
      if (payloadLength > KEEPER_MAX_PAYLOAD_BYTES) {
        this.#failAll(
          fail(FAIL.PROTOCOL, `keeper announced ${payloadLength} payload bytes, cap is ${KEEPER_MAX_PAYLOAD_BYTES}`),
        );
        return;
      }
      const total = KEEPER_HEADER_BYTES + payloadLength;
      if (this.buffer.length < total) return;
      const requestId = this.buffer.subarray(12, 48).toString('ascii');
      const frameSequence = Number(this.buffer.readBigUInt64BE(48));
      const fdCount = this.buffer.readUInt16BE(56);
      const flags = this.buffer.readUInt16BE(58);
      // This client passes no descriptors and the keeper sends none. A frame claiming descriptors
      // means the kernel dropped them and the framing below would no longer be trustworthy.
      if (fdCount !== 0 || flags !== 0) {
        this.#failAll(fail(FAIL.PROTOCOL, `keeper frame declares ${fdCount} fds and flags ${flags}`));
        return;
      }
      let payload = null;
      if (payloadLength > 0) {
        try {
          payload = JSON.parse(this.buffer.subarray(KEEPER_HEADER_BYTES, total).toString('utf8'));
        } catch (error) {
          this.#failAll(fail(FAIL.PROTOCOL, `keeper payload is not JSON: ${String(error)}`));
          return;
        }
      }
      this.buffer = this.buffer.subarray(total);
      const frame = { kind, name: KIND_NAME.get(kind) ?? `0x${kind.toString(16)}`, requestId, frameSequence, payload };
      const entry = this.pending.get(requestId);
      if (entry) {
        if (entry.waiter) {
          const waiter = entry.waiter;
          entry.waiter = null;
          waiter.resolve(frame);
        } else {
          entry.queue.push(frame);
        }
        continue;
      }
      // Nothing is outstanding for this id. `OUTPUT` and `SHUTDOWN` carry the all-dash id by
      // construction, so those are live-push evidence; anything else means correlation is lost.
      if (requestId === KEEPER_NO_REQUEST_ID || kind === KIND.Output || kind === KIND.Shutdown) {
        this.unsolicited.push(frame);
        continue;
      }
      this.#failAll(fail(FAIL.PROTOCOL, `uncorrelated keeper frame ${frame.name} for ${requestId}`));
      return;
    }
  }

  #encode(kind, payload, requestId) {
    const body = Buffer.from(JSON.stringify(payload ?? {}), 'utf8');
    if (body.length > KEEPER_MAX_PAYLOAD_BYTES) {
      throw fail(FAIL.PROTOCOL, `keeper request payload ${body.length} bytes, cap is ${KEEPER_MAX_PAYLOAD_BYTES}`);
    }
    const id = requestId ?? KEEPER_NO_REQUEST_ID;
    const header = Buffer.alloc(KEEPER_HEADER_BYTES);
    KEEPER_MAGIC.copy(header, 0);
    header.writeUInt16BE(KEEPER_FRAMING_VERSION, 4);
    header.writeUInt16BE(kind, 6);
    header.writeUInt32BE(body.length, 8);
    header.write(id, 12, 36, 'ascii');
    header.writeBigUInt64BE(BigInt(this.sequence), 48);
    header.writeUInt16BE(0, 56);
    header.writeUInt16BE(0, 58);
    this.sequence += 1;
    return Buffer.concat([header, body]);
  }

  async #write(bytes, label) {
    await bounded(
      new Promise((resolveWrite, rejectWrite) => {
        this.socket.write(bytes, (error) => (error ? rejectWrite(error) : resolveWrite()));
      }),
      label,
      10_000,
    );
  }

  /**
   * The next frame for one correlation id. A request may be answered by a run of frames, so the
   * queue is per id rather than one slot: reading only the first frame of a run would leave the rest
   * to be misread as the answer to the following request.
   */
  #next(label, timeoutMs, requestId) {
    const window = timeoutMs ?? 20_000;
    const entry = this.pending.get(requestId) ?? { queue: [], waiter: null };
    this.pending.set(requestId, entry);
    if (entry.queue.length > 0) return Promise.resolve(entry.queue.shift());
    return new Promise((resolveFrame, rejectFrame) => {
      const timer = setTimeout(() => {
        entry.waiter = null;
        rejectFrame(fail(FAIL.DEADLINE, `${label}: no keeper frame within ${window}ms`));
      }, window);
      entry.waiter = {
        resolve: (frame) => {
          clearTimeout(timer);
          resolveFrame(frame);
        },
        reject: (error) => {
          clearTimeout(timer);
          rejectFrame(error);
        },
      };
    });
  }

  #release(requestId) {
    const entry = this.pending.get(requestId);
    if (!entry) return;
    this.pending.delete(requestId);
    entry.waiter?.reject(fail(FAIL.PROTOCOL, `request ${requestId} was released while still awaiting a frame`));
    entry.waiter = null;
  }

  async call(kind, payload, expected, label, timeoutMs) {
    // A real correlation id: the keeper echoes it, so an interleaved `OUTPUT` frame can never be
    // mistaken for the answer just requested.
    const requestId = randomUUID();
    const bytes = this.#encode(kind, payload, requestId);
    const waiting = this.#next(label ?? `keeper ${KIND_NAME.get(kind)}`, timeoutMs, requestId);
    await this.#write(bytes, `${label ?? KIND_NAME.get(kind)} write`);
    let frame;
    try {
      frame = await waiting;
    } finally {
      this.#release(requestId);
    }
    if (frame.kind === KIND.Error) {
      throw new KeeperRefusal(frame.payload?.code, frame.payload?.message, frame.payload?.offset);
    }
    if (expected !== undefined && frame.kind !== expected) {
      throw fail(FAIL.PROTOCOL, `${label ?? KIND_NAME.get(kind)}: expected ${KIND_NAME.get(expected)}, got ${frame.name}`);
    }
    await this.evidence?.record({ wire: 'keeper', verb: KIND_NAME.get(kind), reply: frame.name });
    return frame.payload ?? {};
  }

  async hello() {
    const ack = await this.call(
      KIND.Hello,
      { protocolMajor: 1, protocolMinor: 0, keeperVersion: 'qa' },
      KIND.HelloAck,
      'keeper hello',
    );
    if (Number(ack.protocolMajor) !== 1) {
      throw fail(FAIL.PROTOCOL, `keeper speaks protocol major ${ack.protocolMajor}, this harness speaks 1`);
    }
    return ack;
  }

  async spawn(options) {
    const ack = await this.call(
      KIND.Spawn,
      {
        sessionId: options.sessionId,
        argv: options.argv,
        env: Array.isArray(options.env) ? options.env.map(([key, value]) => [key, value]) : [],
        cwd: options.cwd,
        cols: options.cols ?? 80,
        rows: options.rows ?? 24,
        epoch: options.epoch ?? 1,
        requestId: randomUUID(),
      },
      KIND.SpawnAck,
      `keeper spawn ${options.sessionId}`,
      30_000,
    );
    this.epochs.set(options.sessionId, Number(ack.epoch ?? options.epoch ?? 1));
    return {
      sessionId: ack.sessionId,
      pid: Number(ack.pid),
      pgid: ack.pgid == null ? null : Number(ack.pgid),
      processStartTime: ack.processStartTime == null ? null : Number(ack.processStartTime),
      epoch: Number(ack.epoch),
      sequenceOffset: Number(ack.sequenceOffset ?? 0),
    };
  }

  async write(sessionId, epoch, bytes) {
    let total = 0;
    for (let offset = 0; offset < bytes.length; offset += KEEPER_MAX_WRITE_BYTES) {
      const slice = bytes.subarray(offset, offset + KEEPER_MAX_WRITE_BYTES);
      const ack = await this.call(
        KIND.Write,
        { sessionId, epoch, data: b64(slice) },
        KIND.Write,
        `keeper write ${sessionId}`,
      );
      total += Number(ack.written ?? 0);
    }
    this.epochs.set(sessionId, epoch);
    return total;
  }

  /**
   * `chunked` is opt-in on the keeper side: a peer that reads one frame per request would desync on
   * a run it never asked for, so the suffix is only streamed when asked for.
   */
  async attach(sessionId, epoch, afterSequence) {
    const payload = { sessionId, epoch, chunked: true };
    if (afterSequence !== null && afterSequence !== undefined) payload.afterSequence = afterSequence;
    const requestId = randomUUID();
    const bytes = this.#encode(KIND.Attach, payload, requestId);
    const deadline = Date.now() + 60_000;
    const next = () => this.#next(`keeper attach ${sessionId}`, Math.max(1_000, deadline - Date.now()), requestId);
    await this.#write(bytes, `keeper attach ${sessionId} write`);
    try {
      const ack = await next();
      if (ack.kind === KIND.Error) throw new KeeperRefusal(ack.payload?.code, ack.payload?.message, ack.payload?.offset);
      if (ack.kind !== KIND.AttachAck) throw fail(FAIL.PROTOCOL, `keeper attach answered ${ack.name}`);
      this.epochs.set(sessionId, Number(ack.payload.epoch));
      let data = unb64(ack.payload.data);
      const gap = ack.payload.replayGap ?? null;
      // The run only follows when there is no gap: a refused range has nothing to stream, so waiting
      // for chunks here would sit on the deadline and report a false protocol failure.
      if (ack.payload.dataChunked && !gap) {
        for (;;) {
          const frame = await next();
          if (frame.kind === KIND.ReplayChunk) {
            data = Buffer.concat([data, unb64(frame.payload?.data)]);
            continue;
          }
          if (frame.kind === KIND.ReplayEnd) {
            if (frame.payload?.more) continue;
            break;
          }
          throw fail(FAIL.PROTOCOL, `keeper attach run sent ${frame.name}, expected REPLAY_CHUNK or REPLAY_END`);
        }
      }
      return {
        sessionId: ack.payload.sessionId,
        epoch: Number(ack.payload.epoch),
        sequenceOffset: Number(ack.payload.sequenceOffset ?? 0),
        data,
        gap,
      };
    } finally {
      this.#release(requestId);
    }
  }

  async head(sessionId) {
    const ack = await this.call(KIND.Head, { sessionId }, KIND.HeadAck, `keeper head ${sessionId}`);
    return Number(ack.sequenceOffset ?? 0);
  }

  async ownership() {
    const ack = await this.call(KIND.OwnershipQuery, {}, KIND.OwnershipList, 'keeper ownership');
    return (ack.sessions ?? []).map((entry) => ({
      sessionId: entry.id,
      pid: entry.pid == null ? null : Number(entry.pid),
      epoch: Number(entry.epoch ?? 0),
      sequenceOffset: Number(entry.sequenceOffset ?? 0),
      cols: Number(entry.cols ?? 0),
      rows: Number(entry.rows ?? 0),
      owner: entry.owner ?? null,
      generation: entry.generation == null ? null : Number(entry.generation),
    }));
  }

  /**
   * `server::fenced_epoch` requires an `epoch` on `KILL`, so a kill always presents one. The registry
   * does not report the fenced epoch, so the newest epoch this client presented is tracked instead,
   * which is exact because nothing else talks to this keeper.
   */
  async kill(sessionId, epoch) {
    const resolved = epoch ?? this.epochs.get(sessionId);
    if (resolved === undefined) {
      throw fail(FAIL.USAGE, `kill ${sessionId} needs an epoch: the registry does not report the fenced one`);
    }
    await this.call(KIND.Kill, { sessionId, epoch: resolved }, KIND.Kill, `keeper kill ${sessionId}`);
    this.epochs.delete(sessionId);
  }

  close() {
    this.socket.destroy();
  }
}

// -------------------------------------------------------------------------------------------
// Supervision: only children of this process, only inside the owned root.
// -------------------------------------------------------------------------------------------

/** A started child plus the exact argv/env it was started with, for the evidence trail. */
class Supervised {
  constructor(kind, child, argv, env, evidence) {
    this.kind = kind;
    this.child = child;
    this.argv = argv;
    this.env = env;
    this.evidence = evidence;
    this.stdout = '';
    this.stderr = '';
    this.exited = null;
    // A spawn failure (missing file, no exec bit, wrong architecture) arrives asynchronously as an
    // 'error' event. Left unhandled it kills this process with a stack trace instead of naming the
    // binary that could not run, which is the one thing the operator needs to see.
    this.spawnError = null;
    child.stdout?.setEncoding('utf8');
    child.stderr?.setEncoding('utf8');
    child.on('error', (error) => {
      this.spawnError = error;
    });
    child.stdout?.on('data', (chunk) => {
      this.stdout += chunk;
    });
    child.stderr?.on('data', (chunk) => {
      this.stderr += chunk;
    });
    child.on('exit', (code, signal) => {
      this.exited = { code, signal };
    });
  }

  get pid() {
    return this.child.pid ?? null;
  }

  /**
   * The SIGCHLD notification itself, not a poll. `once` resolves on the kernel's report that this
   * child is gone, which is exactly the event a crash case must be built on.
   */
  async waitForExit(timeoutMs) {
    if (this.exited) return this.exited;
    const outcome = await bounded(once(this.child, 'exit'), `${this.kind} exit`, timeoutMs);
    return { code: outcome[0], signal: outcome[1] };
  }

  /** Real, unconditional termination - this is the crash the acceptance case is about. */
  async kill(signal) {
    if (this.exited) return this.exited;
    this.child.kill(signal);
    const outcome = await this.waitForExit(15_000);
    await this.evidence?.record({
      process: this.kind,
      pid: this.pid,
      signal,
      exit: outcome,
      stdout: this.stdout.slice(-8_192),
      stderr: this.stderr.slice(-8_192),
    });
    return outcome;
  }

  async stop() {
    if (this.exited) return;
    this.child.kill('SIGTERM');
    try {
      await this.waitForExit(5_000);
    } catch {
      this.child.kill('SIGKILL');
      await this.waitForExit(5_000).catch(() => null);
    }
  }
}

// -------------------------------------------------------------------------------------------
// The relay: the fault-injection point, in front of the predecessor's legacy socket.
//
// A successor launched with `--handover-from <relay>` treats the relay as its predecessor, so every
// candidate request the production code makes arrives here first:
// `listSessions`, one `describeSession` per session, `transferSessions`, then `commitHandover`.
// `decide(line)` returns whether that line may be forwarded; a line it withholds is the injection,
// and the case performs its kill inside `decide`, which is why the kill is ordered *before* the
// forward rather than raced with it.
//
// No descriptor relay is needed: authority fds travel on the successor's own
// `handover-<uuid>.sock`, which the predecessor dials out to, so this proxy only ever carries
// newline-delimited JSON.
// -------------------------------------------------------------------------------------------

class RelayProxy {
  constructor(server, evidence, listenPath, upstreamPath, decide) {
    this.server = server;
    this.evidence = evidence;
    this.listenPath = listenPath;
    this.upstreamPath = upstreamPath;
    this.decide = decide ?? (() => 'forward');
    this.sockets = new Set();
    this.lines = [];
    this.replies = [];
  }

  static async listen(options) {
    await chmod(options.dir, 0o700);
    const server = net.createServer();
    const proxy = new RelayProxy(server, options.evidence, options.listenPath, options.upstreamPath, options.decide);
    server.on('connection', (downstream) => proxy.#accept(downstream, options));
    await new Promise((resolveListen, rejectListen) => {
      server.once('error', rejectListen);
      server.listen(options.listenPath, () => {
        server.removeListener('error', rejectListen);
        resolveListen();
      });
    });
    await options.evidence?.record({
      step: 'relay-listening',
      listenPath: options.listenPath,
      upstreamPath: options.upstreamPath,
    });
    return proxy;
  }

  #accept(downstream, options) {
    this.sockets.add(downstream);
    downstream.on('close', () => this.sockets.delete(downstream));
    downstream.on('error', () => downstream.destroy());
    const upstream = net.createConnection({ path: this.upstreamPath });
    this.sockets.add(upstream);
    upstream.on('close', () => this.sockets.delete(upstream));
    upstream.on('error', () => {
      downstream.destroy();
    });
    // The predecessor's replies are relayed byte for byte: the candidate's own parser decides
    // whether they are well formed, and this harness must not reshape them. They are *observed*
    // on a separate buffer, so the relay's own copy of the bytes is never rewritten by parsing.
    let replyPending = '';
    upstream.setEncoding('utf8');
    upstream.on('data', (chunk) => {
      if (!downstream.destroyed) downstream.write(chunk);
      replyPending += chunk;
      for (;;) {
        const newline = replyPending.indexOf('\n');
        if (newline < 0) return;
        const line = replyPending.slice(0, newline);
        replyPending = replyPending.slice(newline + 1);
        const reply = tryParseLine(line);
        const kind = reply?.type ?? 'unparseable';
        this.replies.push(kind);
        this.evidence?.record({ step: 'relay-reply', type: kind }).catch(() => null);
      }
    });
    upstream.on('end', () => downstream.end());

    let pending = '';
    // One decision at a time per connection: `decide` may await a kill, and a later line must never
    // overtake the line whose decision has not been reached yet. The chain also keeps a rejected
    // decision from becoming an unhandled rejection - the case observes it through the kill itself.
    let tail = Promise.resolve();
    downstream.setEncoding('utf8');
    downstream.on('data', (chunk) => {
      pending += chunk;
      for (;;) {
        const newline = pending.indexOf('\n');
        if (newline < 0) return;
        const line = pending.slice(0, newline);
        pending = pending.slice(newline + 1);
        const request = tryParseLine(line);
        this.lines.push(request?.type ?? 'unparseable');
        tail = tail.then(() => this.#forward(upstream, downstream, options, line, request)).catch(() => null);
      }
    });
    downstream.on('end', () => {
      if (!upstream.destroyed) upstream.end();
    });
  }

  async #forward(upstream, downstream, options, line, request) {
    let verdict = 'forward';
    try {
      verdict = (await this.decide(request, line)) ?? 'forward';
    } catch (error) {
      // A failing `decide` must not silently forward: that would turn an injection the case
      // intended into a normal handover and report a false pass.
      await this.evidence?.record({ step: 'relay-decide-failed', error: String(error?.message ?? error) });
      verdict = 'drop';
    }
    if (verdict === 'drop') {
      // A withheld line means the peer this proxy stood for is gone: the candidate must see that as
      // a closed predecessor, not as a predecessor that is still thinking. Leaving the stream open
      // would turn the injection into a 10s read timeout inside `LegacyPeer::send_request`.
      await this.evidence?.record({ step: 'relay-withheld', verb: request?.type ?? null });
      downstream.destroy();
      upstream.destroy();
      return;
    }
    if (verdict !== 'forward') {
      throw fail(FAIL.PROTOCOL, `relay decision ${JSON.stringify(verdict)} is neither forward nor drop`);
    }
    if (!upstream.destroyed) upstream.write(`${line}\n`);
    else downstream.destroy();
  }

  async close() {
    for (const socket of this.sockets) socket.destroy();
    this.sockets.clear();
    await new Promise((resolveClose) => this.server.close(() => resolveClose()));
  }
}

// -------------------------------------------------------------------------------------------
// Durable decision record: `legacy.with_extension("transaction")` beside the legacy socket.
// -------------------------------------------------------------------------------------------

const decisionPath = (legacySocketPath) => legacySocketPath.replace(/\.sock$/, '.transaction');

/**
 * `HandoverManager::recorded_decision`: `Some(Retired)` only for a committed decision, `Some(Active)`
 * for a rollback that recorded its reason, `None` for no record or a record that settles nothing. The
 * record's `transferId` must equal the legacy socket path it sits beside, or the daemon itself treats
 * the read as an identity mismatch.
 */
async function readDecision(legacySocketPath) {
  const path = decisionPath(legacySocketPath);
  let raw;
  try {
    raw = await readFile(path, 'utf8');
  } catch (error) {
    if (error.code === 'ENOENT') return { present: false, path, state: null, settled: null };
    throw error;
  }
  const transaction = JSON.parse(raw);
  if (transaction.transferId !== legacySocketPath) {
    throw fail(FAIL.DECISION_UNEXPECTED, `${path} describes ${transaction.transferId}, not ${legacySocketPath}`);
  }
  const settled =
    transaction.state === 'retired' ? 'Retired' : transaction.state === 'active' && transaction.rollbackReason != null ? 'Active' : null;
  return { present: true, path, raw, state: transaction.state, settled, transaction };
}

/** The inventory a record names, or `[]`. `sessionInventory` is always written, so a wrong type is a bug. */
const recordInventory = (transaction) =>
  Array.isArray(transaction?.sessionInventory) ? transaction.sessionInventory : [];

/**
 * `HandoverManager::unpopulated_accounting`, in its own check order: the single field a decision
 * record leaves unpopulated by construction, or `null` when it carries every field.
 */
function unpopulatedField(transaction) {
  if (Number(transaction?.successorPid ?? 0) === 0) return 'successorPid';
  if (Number(transaction?.successorEpoch ?? 0) === 0) return 'successorEpoch';
  if (recordInventory(transaction).length === 0) return 'sessionInventory';
  return null;
}

/**
 * `HandoverManager::accounting_declaration`, derived from the record itself: the declaration this
 * record truthfully owes. A populated inventory is never declared unused, and a fully accounted
 * record owes nothing.
 */
function deriveDeclaration(transaction) {
  const identityUnused = Number(transaction?.successorPid ?? 0) === 0 || Number(transaction?.successorEpoch ?? 0) === 0;
  const inventoryUnused = recordInventory(transaction).length === 0;
  if (identityUnused && inventoryUnused) return DECISION_ACCOUNTING.SUCCESSOR_IDENTITY_AND_INVENTORY_UNUSED;
  if (identityUnused) return DECISION_ACCOUNTING.SUCCESSOR_IDENTITY_UNUSED;
  if (inventoryUnused) return DECISION_ACCOUNTING.SESSION_INVENTORY_UNUSED;
  return null;
}

/**
 * Asserts the plan's Phase 4 accounting contract on a decision record the production daemon wrote:
 *
 *   - the declaration present on disk is one this build knows;
 *   - it equals the declaration the record itself derives (no record claims "unused" for a field it
 *     populated, and no unpopulated field goes undeclared);
 *   - when a session id is supplied, the inventory names exactly that session, so a record that is
 *     `retired` but silent about the transfer it decided is refused rather than counted as a pass;
 *   - `expectUnpopulated` (when given) is the field the case requires to be absent by construction.
 *
 * This is an assertion about the bytes the daemon wrote, not a re-implementation of the writer: the
 * declaration is compared to the record, and the inventory to the real session that was transferred.
 */
function assertDecisionAccounting(decision, options = {}) {
  const { sessionId = null, expectUnpopulated = null, label = decision?.path ?? 'decision record' } = options;
  if (!decision?.present) {
    throw fail(FAIL.DECISION_UNEXPECTED, `${label}: accounting was required but no record exists`);
  }
  const transaction = decision.transaction;
  const declared = transaction.decisionAccounting ?? null;
  const derived = deriveDeclaration(transaction);
  const observed = {
    path: decision.path,
    state: decision.state,
    settled: decision.settled,
    successorPid: Number(transaction.successorPid ?? 0),
    successorEpoch: Number(transaction.successorEpoch ?? 0),
    inventory: recordInventory(transaction).map((entry) => entry?.sessionId ?? null),
    declared,
    derived,
    unpopulated: unpopulatedField(transaction),
  };
  if (declared !== null && !DECISION_ACCOUNTING_VALUES.includes(declared)) {
    throw fail(FAIL.ACCOUNTING_UNKNOWN, `${label}: decisionAccounting ${JSON.stringify(declared)} is not a variant this build writes`, {
      accounting: observed,
    });
  }
  if (declared !== derived) {
    // Two distinct defects, kept distinct so the failure names the one that happened: an explicit
    // declaration that contradicts the record, and an unpopulated field left undeclared.
    if (declared === null) {
      throw fail(
        FAIL.ACCOUNTING_UNDECLARED,
        `${label}: ${JSON.stringify(derived)} derives ${derived} (unpopulated ${observed.unpopulated}) but the record declares nothing`,
        { accounting: observed },
      );
    }
    throw fail(
      FAIL.ACCOUNTING_CONTRADICTS_RECORD,
      `${label}: the record declares ${declared} but its own fields derive ${derived ?? 'nothing unpopulated'}`,
      { accounting: observed },
    );
  }
  if (expectUnpopulated !== null && observed.unpopulated !== expectUnpopulated) {
    throw fail(
      FAIL.ACCOUNTING_CONTRADICTS_RECORD,
      `${label}: the case requires ${expectUnpopulated} to be unpopulated, the record leaves ${observed.unpopulated ?? 'nothing'} absent`,
      { accounting: observed },
    );
  }
  if (sessionId !== null) {
    const named = observed.inventory.filter((entry) => entry === sessionId);
    if (named.length !== 1) {
      throw fail(
        FAIL.RECORD_INVENTORY_INCOMPLETE,
        `${label}: the inventory ${JSON.stringify(observed.inventory)} names ${sessionId} ${named.length} times, expected exactly one`,
        { accounting: observed },
      );
    }
  }
  return observed;
}

/** Occurrences of the typed warning `DECISION_ACCOUNTING_UNPOPULATED` in a daemon log body. */
function accountingWarnings(text) {
  const lines = String(text ?? '').split('\n');
  const matched = [];
  // Built from the constant so the event name has one spelling: a log line that drifts from
  // `handover.rs::DECISION_ACCOUNTING_UNPOPULATED` must fail this read, not silently match nothing.
  const event = new RegExp(`event="?${DECISION_ACCOUNTING_EVENT.replace(/\./g, '\\.')}"?`);
  for (const line of lines) {
    if (!event.test(line)) continue;
    const field = /field="?([A-Za-z]+)"?/.exec(line);
    const committed = /committed="?(true|false)"?/.exec(line);
    matched.push({ field: field ? field[1] : null, committed: committed ? committed[1] === 'true' : null, line });
  }
  return matched;
}

/**
 * A subscription to the isolated daemon log's accounting warnings, opened **before** the transition
 * whose warning it must observe.
 *
 * The log write is asynchronous (`logging.rs` hands each record to a worker), so a snapshot taken
 * after the fact cannot distinguish "the warning was never emitted" from "the write had not landed
 * yet". This subscribes to the kernel's file-change notification on the log *directory* - a rename or
 * a rotation also reports on the directory, and the live file may not exist until the first record -
 * and re-reads the whole log on each notification. The only thing that ends the wait is the record
 * appearing or the deadline expiring: no sleep, no interval poll, and no claim about elapsed time.
 *
 * Notifications are coalesced by re-reading rather than by trusting the event payload: the handler
 * only signals "something changed", which is exactly what `fs.watch` guarantees.
 */
class AccountingWatch {
  constructor(evidence, dataDir, baseline) {
    this.evidence = evidence;
    this.dataDir = dataDir;
    this.baseline = baseline;
    this.watcher = null;
    this.pending = [];
    this.signal = null;
    this.closed = null;
  }

  /**
   * Opens the subscription. Called before the trigger, never after it. `ctx` is required only to
   * outlive the case: the watch registers there so a throw before this object's own `close` still
   * releases it.
   */
  static async open(ctx, evidence, dataDir) {
    const baseline = await accountingSnapshot(evidence, dataDir);
    const watch = new AccountingWatch(evidence, dataDir, baseline);
    await mkdir(join(dataDir, 'logs'), { recursive: true, mode: 0o700 });
    const notify = () => {
      const waiter = watch.signal;
      if (waiter) {
        watch.signal = null;
        waiter();
      } else {
        watch.pending.push(true);
      }
    };
    try {
      watch.watcher = watchLogDirectory(join(dataDir, 'logs'), notify);
    } catch (error) {
      // A subscription that cannot be opened is reported, never silently downgraded to a poll: the
      // case would then be asserting on a snapshot it could not order against the transition.
      throw gap(
        GAP.LOG_WATCH_UNAVAILABLE,
        `could not subscribe to ${join(dataDir, 'logs')} for accounting warnings: ${String(error)}`,
      );
    }
    await evidence.record({
      step: 'accounting-watch-opened',
      dataDir,
      logs: baseline.paths,
      baseline: baseline.warnings.length,
    });
    ctx?.watches?.push(watch);
    return watch;
  }

  /** Resolves on the next file-change notification, or when one arrived while the caller was busy. */
  async nextChange(timeoutMs, label) {
    if (this.pending.length > 0) {
      this.pending.shift();
      return;
    }
    if (this.closed) throw this.closed;
    await bounded(
      new Promise((resolveSignal) => {
        this.signal = resolveSignal;
      }),
      label,
      timeoutMs,
    );
  }

  /**
   * Waits for the record this decision owes, and for nothing that it does not.
   *
   * A record that leaves a field unpopulated owes exactly one warning naming that field: the loop ends
   * when it appears. A record that carries every field owes none, and absence can only be asserted
   * after the log writer has provably flushed *past* the decision - so that branch requires a
   * `barrier`, a pattern for a record the daemon emits strictly after `record_decision`. Without one
   * this refuses as a typed gap instead of waiting out a clock: a clock expiry would report "no
   * warning" for a log that simply had not been written yet, which is exactly the false pass the plan
   * forbids.
   */
  async expect(decision, label, options = {}) {
    const { timeoutMs = 30_000, barrier = null } = options;
    const derived = decision?.present ? deriveDeclaration(decision.transaction) : null;
    const field = decision?.present ? unpopulatedField(decision.transaction) : null;
    const wanted = this.baseline.warnings.length + (derived === null ? 0 : 1);
    if (derived === null && barrier === null) {
      throw gap(
        GAP.ACCOUNTING_BARRIER_ABSENT,
        `${label}: the record carries every field, so the claim is that no warning was emitted, and absence is only assertable after a log record emitted past the decision`,
      );
    }
    const deadline = Date.now() + timeoutMs;
    let observed = null;
    for (;;) {
      const snapshot = await accountingSnapshot(this.evidence, this.dataDir);
      const fresh = snapshot.warnings.slice(this.baseline.warnings.length);
      const matching = derived === null ? fresh : fresh.filter((entry) => entry.field === field);
      if (derived !== null && matching.length > 0) {
        observed = { snapshot, fresh, matching };
        break;
      }
      if (derived === null) {
        // The negative claim: the barrier proves the writer passed the decision, and by then no
        // warning may have appeared.
        if (fresh.length > 0) {
          throw fail(
            FAIL.ACCOUNTING_WARNING_ABSENT,
            `${label}: a fully accounted record still warned ${JSON.stringify(fresh.map((entry) => entry.field))}`,
          );
        }
        if (snapshot.text && barrier.test(snapshot.text)) {
          observed = { snapshot, fresh, matching: [] };
          break;
        }
      }
      const remaining = deadline - Date.now();
      if (remaining <= 0) {
        throw fail(
          FAIL.ACCOUNTING_WARNING_ABSENT,
          `${label}: the log gained ${fresh.length} warning(s) ${JSON.stringify(fresh.map((entry) => entry.field))}, none for ${field} (${derived}), within ${timeoutMs}ms`,
        );
      }
      try {
        await this.nextChange(remaining, `${label}: daemon log changed`);
      } catch (error) {
        // A subscription deadline is this assertion's own failure, not a generic harness timeout: the
        // report has to say which accounting claim went unanswered.
        throw fail(
          FAIL.ACCOUNTING_WARNING_ABSENT,
          `${label}: no log change answered the ${derived === null ? 'no-warning' : `warning for ${field}`} claim within ${timeoutMs}ms (${String(error?.detail ?? error)})`,
        );
      }
    }
    const { snapshot, fresh, matching } = observed;
    if (snapshot.warnings.length !== wanted) {
      throw fail(
        FAIL.ACCOUNTING_WARNING_ABSENT,
        `${label}: the log holds ${snapshot.warnings.length} warning(s), expected ${wanted} for this decision`,
      );
    }
    await this.evidence.record({
      step: 'accounting-warning',
      label,
      logs: snapshot.paths,
      derived,
      unpopulated: field,
      fresh: fresh.map((entry) => ({ field: entry.field, committed: entry.committed })),
    });
    return { logs: snapshot.paths, derived, unpopulated: field, fresh: fresh.map((entry) => entry.field), matching: matching.length };
  }

  close() {
    this.closed = this.closed ?? new Error('the accounting watch was closed');
    this.watcher?.close();
    this.watcher = null;
  }
}

/**
 * `fs.watch` on one directory. The callback is not trusted to describe what changed: rotations report
 * `rename`, content writes report `change`, and the same file may report both, so the handler only
 * signals and the caller re-reads. An error is surfaced through the same signal, which makes the
 * caller re-read and then decide on the bytes rather than on the notification.
 */
function watchLogDirectory(dir, notify) {
  const watcher = watch(dir, { persistent: true, recursive: false }, () => notify());
  watcher.on('error', () => notify());
  return watcher;
}

/** The warning state of one isolated runtime's daemon log, as a snapshot a watch diffs against. */
async function accountingSnapshot(evidence, dataDir) {
  const log = await evidence.daemonLogRecords(dataDir);
  return { paths: log.paths, text: log.text, warnings: accountingWarnings(log.text) };
}

/**
 * What the isolated daemon said about its keeper startup pass, read from the log it wrote.
 *
 * The gap below is only honest if it names the observed cause rather than a remembered one, so the
 * line the daemon emitted at startup travels with the gap.
 */
async function keeperStartupEvidence(ctx) {
  const log = await ctx.evidence.daemonLogRecords(ctx.paths.dataDir);
  const lines = String(log.text ?? '').split('\n');
  const startupLine = lines.find((line) => /keeper startup pass/.test(line)) ?? null;
  const keeperLines = lines.filter((line) => /keeper/i.test(line)).slice(-20);
  return { startupLine, keeperLines, logs: log.paths };
}

/**
 * Stages this case's daemon into a minimal macOS app bundle, and returns the executable to run.
 *
 * `keeper_startup::ensure_keeper` -> `bundle_keeper_dir(current_exe())` reads the executable's own
 * path: the parent must be named `MacOS` and the payload `../Resources/keeper/ferryx-ptykeeper` must
 * exist. There is no environment override, so a `cargo` output answers `PayloadAbsent`,
 * `start_production` aborts, the new-session routing flag is never written, and no session this run
 * creates can ever be keeper-owned -- which is precisely the fact the adoption case measures. Staging
 * the real bytes into the shipped layout is what turns that case from a reported gap into a
 * measurement.
 *
 * The bundle goes inside the case root so the isolation contract covers the executable too, and a
 * path that is already a bundle is returned untouched.
 */
async function stageCaseDaemon(paths, binaries) {
  const requested = binaries.daemonBinary;
  if (!requested) return null;
  // Only macOS compiles the keeper at all (`daemon/mod.rs` gates the module), so staging elsewhere
  // would change where the daemon runs from without enabling anything.
  if (process.platform !== 'darwin') return requested;
  if (!binaries.keeperBinary) return requested;
  if (isBundledDaemon({ daemonPath: requested })) return requested;
  const staged = stageAppBundle({
    appPath: join(paths.appDir, 'Ferryx.app'),
    daemonBinary: requested,
    keeperBinary: binaries.keeperBinary,
    sourceDir: REPO_ROOT,
    triples: [],
  });
  return staged.daemon;
}

/**
 * The keeper payload a daemon at `daemonBinary` would derive, mirroring
 * `keeper_startup::bundle_keeper_dir`: `<exe dir>/../Resources/keeper` and only when the executable's
 * own directory is named `Contents/MacOS`.
 *
 * This is what decides whether adoption is reachable at all, so the adoption case reports the
 * computed answer instead of asserting a cause it never checked. It shares `keeperPayloadDirFor` with
 * the staging code above, so the reported layout and the staged layout cannot disagree.
 */
function bundleKeeperPayload(daemonBinary) {
  const payload = keeperPayloadDirFor(daemonBinary);
  return {
    daemonBinary,
    bundleLayout: payload !== null,
    payloadDir: payload,
    payloadBinary: payload === null ? null : join(payload, KEEPER_BINARY_NAME),
  };
}

/**
 * Keeps the decision record byte-for-byte. The record is the primary evidence for every accounting
 * claim, so the run stores the bytes the product wrote beside the run rather than only the fields
 * this harness parsed out of them.
 */
async function captureDecision(ctx, name, decision) {
  if (!decision?.present) return { present: false, path: decision?.path ?? null };
  const stored = await ctx.evidence.captureRaw(join('decisions', name), decision.raw);
  return { present: true, recordPath: decision.path, stored };
}

/**
 * Keeps what each real process said, byte for byte, for the case that produced it. A failure is
 * frequently only explained by the candidate's own stderr (a refused frame, a `resolve_commit`
 * abort), and a case that recorded only its own verdict would have thrown that away.
 */
async function captureProcessStreams(ctx, label, processes) {
  const captured = {};
  for (const [key, process] of Object.entries(processes)) {
    if (!process) continue;
    const stdout = await ctx.evidence.captureRaw(join('processes', `${label}-${key}.stdout`), process.stdout ?? '');
    const stderr = await ctx.evidence.captureRaw(join('processes', `${label}-${key}.stderr`), process.stderr ?? '');
    captured[key] = { pid: process.pid ?? null, exit: process.exited ?? null, stdout, stderr };
    await ctx.evidence.record({
      step: 'process-streams',
      label,
      role: key,
      pid: process.pid ?? null,
      exit: process.exited ?? null,
      stdoutBytes: stdout.bytes,
      stderrBytes: stderr.bytes,
      stderrTail: (process.stderr ?? '').slice(-2_048),
    });
  }
  return captured;
}

// -------------------------------------------------------------------------------------------
// Context: the isolated paths, the real binaries, and every child this case started.
// -------------------------------------------------------------------------------------------

class Context {
  constructor(options, paths, evidence, binaries) {
    this.options = options;
    this.paths = paths;
    this.evidence = evidence;
    this.binaries = binaries;
    this.children = [];
    this.clients = [];
    this.relays = [];
    this.watches = [];
    this.keeper = null;
    this.keeperProcess = null;
    this.daemon = null;
    this.daemonStarts = 0;
    this.workspaceId = `phase4-workspace-${randomUUID()}`;
  }

  get daemonSocketPath() {
    return this.paths.daemonSocketPath;
  }

  env(extra) {
    return childEnv(process.env, {
      FERRYX_RUNTIME_DIR: this.paths.runtimeDir,
      FERRYX_LEGACY_RUNTIME_DIR: this.paths.legacyRuntimeDir,
      FERRYX_DATA_DIR: this.paths.dataDir,
      FERRYX_SESSION_DIR: this.paths.sessionDir,
      FERRYX_KEEPER_DIR: this.paths.keeperDir,
      FERRYX_DAEMON_PLIST_PATH: this.paths.daemonPlistPath,
      FERRYX_PTY_KEEPER_PLIST_PATH: this.paths.keeperPlistPath,
      // The keeper install pass creates `$HOME/Library/Logs/Ferryx` once it runs.
      HOME: this.paths.homeDir,
      FERRYX_HANDOVER_V5: '1',
      FERRYX_QA_HARNESS: 'phase4-handover-failure-injection',
      ...extra,
    });
  }

  startProcess(kind, argv, extraEnv) {
    const env = this.env(extraEnv ?? {});
    const child = spawn(argv[0], argv.slice(1), {
      cwd: this.paths.repo,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    const supervised = new Supervised(kind, child, argv, env, this.evidence);
    this.children.push(supervised);
    return supervised;
  }

  /** The real keeper binary, then its real socket, then a real handshake. */
  async startKeeper() {
    const argv = [this.binaries.keeperBinary, '--socket', this.paths.keeperSocketPath];
    const process = this.startProcess('keeper', argv);
    this.keeperProcess = process;
    await this.evidence.record({ process: 'keeper', step: 'started', pid: process.pid, socket: this.paths.keeperSocketPath });
    this.keeper = await until('keeper socket accepts connections', 30_000, async () => {
      if (process.spawnError) {
        throw gap(
          GAP.KEEPER_START_FAILED,
          `could not execute ${this.binaries.keeperBinary}: ${process.spawnError.code ?? process.spawnError.message}`,
        );
      }
      if (process.exited) {
        throw gap(
          GAP.KEEPER_START_FAILED,
          `the keeper exited before binding its socket: ${process.stderr.slice(-2_048)}`,
        );
      }
      try {
        const client = await KeeperClient.connect(this.paths.keeperSocketPath, this.evidence);
        await client.hello();
        return client;
      } catch {
        return null;
      }
    });
    return this.keeper;
  }

  /**
   * A real daemon in this isolated runtime root, awaited on its handshake - the readiness signal
   * every later request depends on, so it cannot be true while the socket is not serving.
   */
  async startDaemon(options) {
    const role = options?.role ?? 'initial';
    this.daemonStarts += 1;
    const argv = [this.binaries.daemonBinary, '--daemon'];
    if (options?.handoverFrom) argv.push('--handover-from', options.handoverFrom);
    // Ownership routing is a startup environment decision (`keeper_startup.rs::OwnershipPolicy`),
    // never a CLI verb. A value the daemon does not parse is refused here instead of forwarded: the
    // daemon's own answer to a typo is to log it and keep the persisted flag, which would make a case
    // that asked for a disable run with routing it never had.
    let ownership = null;
    if (options?.ownership) {
      ownership = String(options.ownership).trim().toLowerCase();
      if (!OWNERSHIP_POLICY_VALUES.includes(ownership)) {
        throw fail(
          FAIL.USAGE,
          `${KEEPER_OWNERSHIP_ENV}=${JSON.stringify(options.ownership)} is not a spelling the daemon parses; it accepts ${OWNERSHIP_POLICY_VALUES.join('|')}`,
        );
      }
    }
    const process = this.startProcess(
      `daemon-${role}-${this.daemonStarts}`,
      argv,
      ownership ? { [KEEPER_OWNERSHIP_ENV]: ownership } : undefined,
    );
    await this.evidence.record({
      process: `daemon-${role}`,
      step: 'started',
      generation: this.daemonStarts,
      pid: process.pid,
      handoverFrom: options?.handoverFrom ?? null,
      ownership: ownership ? `${KEEPER_OWNERSHIP_ENV}=${ownership}` : 'default',
      executable: this.binaries.daemonBinary,
      bundled: this.paths.appDir !== undefined && isUnder(this.paths.appDir, this.binaries.daemonBinary),
    });
    const client = await until(`${role} daemon handshake`, 60_000, async () => {
      if (process.spawnError) {
        throw gap(
          GAP.DAEMON_BINARY_ABSENT,
          `could not execute ${this.binaries.daemonBinary}: ${process.spawnError.code ?? process.spawnError.message}`,
        );
      }
      if (process.exited) {
        throw fail(
          FAIL.PROTOCOL,
          `${role} daemon exited (${JSON.stringify(process.exited)}) before binding ${this.daemonSocketPath}: ${process.stderr.slice(-2_048)}`,
        );
      }
      try {
        return await DaemonClient.connect(this.daemonSocketPath, this.evidence);
      } catch {
        return null;
      }
    });
    this.clients.push(client);
    // The client answers requests; the process carries the SIGCHLD event. Both belong to the same
    // daemon, so they are handed back together rather than looked up by pid later.
    client.process = process;
    if (!this.daemon) this.daemon = client;
    await client.registerWorkspace(this.workspaceId, this.paths.repo).catch(() => null);
    await this.evidence.record({
      process: `daemon-${role}`,
      step: 'ready',
      generation: this.daemonStarts,
      pid: process.pid,
      identity: client.identity,
    });
    return client;
  }

  /**
   * A successor is the production candidate: `--handover-from` is the only thing that selects the
   * handover path, and `cli.rs` reads it before it decides whether to announce readiness, so this
   * process announces nothing and binds nothing until it commits.
   */
  async startSuccessor(handoverFrom) {
    const argv = [this.binaries.daemonBinary, '--daemon', '--handover-from', handoverFrom];
    const process = this.startProcess(`daemon-successor-${this.daemonStarts}`, argv);
    await this.evidence.record({ process: 'successor', step: 'started', pid: process.pid, handoverFrom });
    return process;
  }

  /**
   * The injection point of every case that needs one, owned by the context so teardown closes it
   * even when the case throws before it can.
   */
  async startRelay(options) {
    const relay = await RelayProxy.listen({
      dir: this.paths.relayDir,
      listenPath: this.paths.relayPath,
      upstreamPath: options.upstreamPath,
      evidence: this.evidence,
      decide: options.decide,
    });
    this.relays.push(relay);
    return relay;
  }

  async shutdown() {
    for (const relay of this.relays) await relay.close().catch(() => null);
    this.relays.length = 0;
    // A case that throws before its own `finally` runs must not leave a persistent watcher holding
    // the event loop open, so every subscription is also owned here.
    for (const watch of this.watches) watch.close();
    this.watches.length = 0;
    for (const client of this.clients) client.close();
    this.clients.length = 0;
    this.keeper?.close();
    this.keeper = null;
    for (const process of this.children) await process.stop().catch(() => null);
    this.children.length = 0;
  }

  /** Removes every session this case left in the keeper, through the keeper's own kill verb. */
  async reapKeeperSessions() {
    if (!this.keeper) return [];
    let owned = [];
    try {
      owned = await this.keeper.ownership();
    } catch {
      return [];
    }
    const killed = [];
    for (const entry of owned) {
      try {
        await this.keeper.kill(entry.sessionId, entry.epoch);
        killed.push(entry.sessionId);
      } catch (error) {
        await this.evidence.record({ step: 'reap-failed', sessionId: entry.sessionId, epoch: entry.epoch });
      }
    }
    return killed;
  }
}

// -------------------------------------------------------------------------------------------
// Fixtures and proofs
// -------------------------------------------------------------------------------------------

/**
 * A PTY-backed shell. `SpawnOptions.shell` is a program-only preference, so this is where the
 * fixture program is chosen; nothing is injected into argv.
 */
async function spawnFixture(ctx, daemon) {
  const sessionId = randomUUID();
  await daemon.spawn({
    sessionId,
    workspaceId: ctx.workspaceId,
    cwd: ctx.paths.repo,
    shell: '/bin/sh',
  });
  const live = await until(`session ${sessionId} is live`, 30_000, async () => {
    const sessions = await daemon.liveSessions();
    return sessions.find((entry) => entry.backendId === sessionId) ?? null;
  });
  const table = await processTable();
  const identity = assertProcessIdentity(
    table,
    live.localChildPid,
    live.localChildStartTime,
    `session ${sessionId} child`,
    'ms',
  );
  await ctx.evidence.record({
    step: 'fixture-spawned',
    sessionId,
    pid: live.localChildPid,
    startTime: live.localChildStartTime,
    osIdentity: identity,
    readerOpen: live.readerOpen,
  });
  return { sessionId, pid: live.localChildPid, startTime: live.localChildStartTime, identity };
}

/**
 * Proves a daemon is actually serving a session: attach, then a two-write command whose *output*
 * cannot be confused with its input echo. The command is split mid-quote, so the echoed input
 * contains `MARK-%s` and the nonce separately but never `MARK-<nonce>`; only a real shell evaluating
 * `printf` produces the contiguous marker.
 *
 * The attach is made on a connection of its own: `attach` converts the connection that asked into a
 * stream, so doing it on the case's control connection would mix output frames into the reply
 * sequence of every later request.
 *
 * Input gets a second connection of its own for the same reason, one step further: the attach socket
 * carries output only, and the daemon closes it on any client byte. Input travels as
 * `DaemonRequest::Write`, which is what the real client does.
 */
async function proveServing(ctx, sessionId, nonce) {
  const client = await DaemonClient.connect(ctx.daemonSocketPath, ctx.evidence);
  let control = null;
  try {
    const attachment = await client.attach(sessionId, null);
    const attached = new AttachedSession(attachment, ctx.evidence);
    const marker = `MARK-${nonce}`;
    control = await DaemonClient.connect(ctx.daemonSocketPath, ctx.evidence);
    await control.write(sessionId, Buffer.from("printf 'MARK-%s\\n' '", 'utf8'));
    await control.write(sessionId, Buffer.from(`${nonce}'\n`, 'utf8'));
    const bytes = await attached.waitFor(`output ${marker}`, 30_000, (buffer) =>
      buffer.toString('utf8').includes(marker),
    );
    await ctx.evidence.record({
      step: 'serve-proof',
      sessionId,
      marker,
      servedBy: Number(client.identity.pid),
      epoch: attachment.epoch,
      replyBytes: bytes.length,
      historyBytes: attachment.history.length,
    });
    return {
      marker,
      replyBytes: bytes.length,
      sha256: sha256(bytes),
      historyBytes: attachment.history.length,
      servedBy: Number(client.identity.pid),
      epoch: attachment.epoch,
    };
  } finally {
    control?.close();
    client.close();
  }
}

/**
 * The same proof through the keeper: `exec cat` with echo disabled means a byte can only reach the
 * replay log by travelling through the PTY and back, so `head` growing past the written offset is
 * the child being alive and served. Returns the epoch the attach fenced, for the next call.
 */
async function proveKeeperServing(ctx, sessionId) {
  // The epoch is read from the keeper, never assumed. Any daemon that restarts while this session
  // is keeper-held attaches it at its own clock-seeded epoch (`EpochSource::new`), so the newest
  // fence after a replacement is that daemon's number, not the small one this harness spawned with.
  // Presenting 2 against a fence of 1795002835940 is an `EPOCH_MISMATCH`, which reads like a defect
  // in the keeper rather than a stale assumption in the harness.
  const entry = (await ctx.keeper.ownership()).find((row) => row.sessionId === sessionId);
  if (!entry) throw fail(FAIL.REFUSED, `the keeper no longer lists ${sessionId}`);
  const epoch = entry.epoch;
  const before = await ctx.keeper.head(sessionId);
  const marker = Buffer.from(`KMARK-${sessionId}-${epoch}\n`);
  const written = await ctx.keeper.write(sessionId, epoch, marker);
  if (written !== marker.length) {
    throw fail(FAIL.REFUSED, `the keeper accepted ${written} of ${marker.length} bytes`);
  }
  const after = await until(`keeper head of ${sessionId} past ${before}`, 30_000, async () => {
    const head = await ctx.keeper.head(sessionId);
    return head > before ? head : null;
  });
  // One attach above the fence, which is what makes this client the reader for the replay below.
  const replay = await ctx.keeper.attach(sessionId, epoch + 1, before);
  if (!replay.data.includes(marker)) {
    throw fail(
      FAIL.SERVE_PROOF_MISSING,
      `the keeper replayed ${replay.data.length} bytes from ${before} without the marker written at ${before}`,
    );
  }
  return {
    marker: marker.toString('utf8').trim(),
    written,
    presentedEpoch: epoch,
    headBefore: before,
    headAfter: after,
    resumedAt: replay.sequenceOffset,
    fenced: replay.epoch,
  };
}

/** A session started by the real keeper binary, with the identity the keeper reported. */
async function spawnKeeperFixture(ctx) {
  const sessionId = randomUUID();
  const epoch = 1;
  const ack = await ctx.keeper.spawn({
    sessionId,
    argv: ['/bin/sh', '-c', 'stty -opost -echo 2>/dev/null || true; exec cat'],
    env: [],
    cwd: ctx.paths.repo,
    epoch,
  });
  const table = await processTable();
  const identity = assertProcessIdentity(table, ack.pid, ack.processStartTime, `keeper session ${sessionId}`);
  await ctx.evidence.record({
    step: 'keeper-fixture-spawned',
    sessionId,
    pid: ack.pid,
    processStartTime: ack.processStartTime,
    epoch: ack.epoch,
    osIdentity: identity,
  });
  return { sessionId, pid: ack.pid, startTime: ack.processStartTime, epoch: ack.epoch, identity };
}

/**
 * A socket that nothing listens on must refuse. Returns the refusal code, so the case can record
 * *how* the endpoint was gone (ECONNREFUSED for a dead listener, ENOENT for an unlinked path).
 *
 * One settle only: `connect` or `error`, whichever the kernel reports first, resolved through a
 * single promise so neither branch can leave a listener behind on a socket that then errors again.
 */
async function expectRefused(socketPath, label, evidence) {
  const socket = net.createConnection({ path: socketPath });
  const settled = new Promise((resolveOutcome) => {
    socket.once('connect', () => resolveOutcome({ accepted: true }));
    socket.once('error', (error) => resolveOutcome({ accepted: false, code: error.code ?? String(error.message) }));
  });
  let outcome;
  try {
    outcome = await bounded(settled, label, 5_000);
  } finally {
    socket.destroy();
  }
  await evidence?.record({ step: 'expect-refused', socketPath, label, outcome });
  if (outcome.accepted) {
    throw fail(FAIL.REFUSED, `${label}: something still accepts connections on ${socketPath}`);
  }
  return outcome.code;
}

/** The pid a socket currently answers as, or `null` when nothing is listening on it. */
async function daemonPidAt(socketPath, evidence) {
  let client;
  try {
    client = await DaemonClient.connect(socketPath, evidence);
  } catch {
    return null;
  }
  const pid = Number(client.identity.pid);
  client.close();
  return pid;
}

/**
 * Commit completion is a change of who answers on the same socket path, not a sleep: a committed
 * successor serves the predecessor's own canonical listener, so `daemon.sock` starts reporting the
 * successor's pid at the moment authority was adopted. Bounded, because a handover that never
 * finishes must fail rather than hang.
 */
async function awaitDaemonPid(pid, socketPath, evidence, timeoutMs) {
  const seen = new Set();
  return until(`the socket ${socketPath} answers as pid ${pid}`, timeoutMs ?? 120_000, async () => {
    const observed = await daemonPidAt(socketPath, evidence);
    if (observed === null) return null;
    if (!seen.has(observed)) {
      seen.add(observed);
      await evidence?.record({ step: 'daemon-pid-observed', socketPath, pid: observed, wanted: Number(pid) });
    }
    return observed === Number(pid) ? { pid: observed } : null;
  });
}

/** Every `legacy-*` artifact a daemon left in one directory, sorted, so absence is checkable. */
async function legacyArtifacts(dir) {
  let entries;
  try {
    entries = await readdir(dir);
  } catch (error) {
    if (error.code === 'ENOENT') return [];
    throw error;
  }
  return entries.filter((name) => name.startsWith('legacy-')).sort();
}

// -------------------------------------------------------------------------------------------
// Cases
// -------------------------------------------------------------------------------------------

const CASES = [];

function defineCase(id, title, requires, run) {
  CASES.push({ id, title, requires, run });
}

/** Every `legacy-*` durable decision record a directory holds, so "no decision" is checkable. */
async function decisionArtifacts(dir) {
  return (await legacyArtifacts(dir)).filter((name) => name.endsWith('.transaction'));
}

/**
 * The inverse of `assertDecisionAccounting`: no durable decision exists at all. Both the renamed record
 * and its `create_new` staging file are checked, because a surviving `.transaction.pending` means a
 * decision was attempted and never made durable - which is not the same fact as "no decision".
 */
async function assertNoDecision(dir, label, expectVisible) {
  const names = await legacyArtifacts(dir);
  const records = names.filter((name) => name.endsWith('.transaction'));
  const staged = names.filter((name) => name.endsWith('.transaction.pending'));
  // Non-vacuity: "no record" only means something if this scan can see the transfer the record would
  // have belonged to. `prepare_handover` mints `legacy-<pid>-<millis>.sock` in the same directory, so
  // its absence from this listing would make every assertion below true for the wrong reason.
  if (expectVisible) {
    const name = basename(expectVisible);
    if (!names.includes(name)) {
      throw fail(
        FAIL.PROTOCOL,
        `${label}: the scan of ${dir} does not even list ${name}, so "no decision record" is vacuous`,
      );
    }
  }
  if (records.length > 0) {
    throw fail(FAIL.DECISION_TOO_EARLY, `${label}: decision record(s) ${JSON.stringify(records)} exist where the plan expects none`);
  }
  if (staged.length > 0) {
    throw fail(
      FAIL.DECISION_TOO_EARLY,
      `${label}: staging file(s) ${JSON.stringify(staged)} survived, so a decision was attempted without becoming durable`,
    );
  }
  return { records: [], staged: [], headerArtifacts: names.filter((name) => !name.endsWith('.sock') && !name.endsWith('.transaction') && !name.endsWith('.transaction.pending')) };
}

/**
 * A pid that had an owner must stop existing once its last owner is gone. A timeout here is not a
 * slow machine: it means something still holds the master, so the bytes were never really handed on.
 */
async function requireGone(pid, label, code, detail, timeoutMs) {
  try {
    await untilGone(pid, label, timeoutMs ?? 30_000);
  } catch (error) {
    if (error instanceof AcceptanceError && error.code === FAIL.DEADLINE) {
      throw fail(code, `${detail}: pid ${pid} was still running when the bound expired`);
    }
    throw error;
  }
}

/**
 * Waits, bounded, for a path to stop existing - the positive form of "unlinked".
 *
 * This one is a poll on purpose, and the reason is in the product: on the commit path `handover.rs`
 * moves to `Retired`, then spawns a task that unlinks the legacy socket after a 100ms pause, so the
 * path legitimately outlives the event that doomed it. There is no notification to subscribe to -
 * the unlink happens inside the daemon process - so a subscription here would have to be invented.
 * The bound is what keeps this honest: a socket that never goes away fails rather than being
 * reported as unlinked, and no case treats the *arrival* of this state as proof of anything else.
 */
async function untilAbsent(path, label, timeoutMs) {
  return until(`${label}: ${path} removed`, timeoutMs ?? 30_000, async () => ((await exists(path)) ? null : true));
}

/** A client for whatever answers on `daemon.sock` right now, plus the pid it claims. */
async function connectNow(ctx) {
  const client = await DaemonClient.connect(ctx.daemonSocketPath, ctx.evidence);
  ctx.clients.push(client);
  return client;
}

/**
 * Plan rev5 (a): a successor that dies BEFORE the durable decision leaves the predecessor alive and
 * still serving, nothing durable on disk, and the retry committing normally.
 *
 * The injection is the commit line itself: the successor is killed from the relay's decision, so the
 * kill is ordered strictly before the withheld line, and the predecessor is never asked to commit.
 */
defineCase(
  'a-successor-dies-before-decision',
  'successor killed at CommitHandover: predecessor keeps serving, no decision is written, the retry commits',
  ['daemon'],
  async (ctx) => {
    const predecessor = ctx.daemon.process;
    const fixture = await spawnFixture(ctx, ctx.daemon);

    const prepared = await ctx.daemon.prepareHandover();
    if (!prepared.activeSessions.includes(fixture.sessionId)) {
      throw fail(
        FAIL.PROTOCOL,
        `prepareHandover offered ${JSON.stringify(prepared.activeSessions)} without the live session ${fixture.sessionId}`,
      );
    }
    // The record can only land beside the path prepare just minted, so its absence below is a
    // statement about a known location rather than about a directory nobody looked in.
    const before = await readDecision(prepared.legacySocketPath);
    if (before.present) {
      throw fail(FAIL.DECISION_TOO_EARLY, `${before.path} already exists before any candidate ran`);
    }
    if (!(await exists(prepared.legacySocketPath))) {
      throw fail(FAIL.PROTOCOL, `prepareHandover returned ${prepared.legacySocketPath}, which nothing bound`);
    }

    let successor = null;
    let resolveCommit = null;
    const commitSeen = new Promise((resolveSeen) => {
      resolveCommit = resolveSeen;
    });
    await ctx.startRelay({
      upstreamPath: prepared.legacySocketPath,
      decide: async (request) => {
        if (request?.type !== 'commitHandover') return 'forward';
        await ctx.evidence.record({
          step: 'inject',
          variant: 'a-successor-dies-before-decision',
          at: 'commitHandover',
          successorPid: successor.pid,
          predecessorPid: predecessor.pid,
        });
        const outcome = await successor.kill('SIGKILL');
        resolveCommit(outcome);
        // `drop` destroys both sockets: the candidate that would have read this reply is gone, and
        // the line must never reach the predecessor.
        return 'drop';
      },
    });

    successor = await ctx.startSuccessor(ctx.paths.relayPath);
    const injected = await bounded(commitSeen, 'the successor reached the commit boundary', 180_000);
    const successorExit = await successor.waitForExit(30_000);
    if (successorExit.signal !== 'SIGKILL') {
      throw fail(
        FAIL.PROTOCOL,
        `the successor left through ${JSON.stringify(successorExit)}, not the injected SIGKILL (${JSON.stringify(injected)})`,
      );
    }

    // The predecessor is the surviving authority: its process, and the socket that answers as it.
    if (!processAlive(predecessor.pid)) {
      throw fail(FAIL.DECISION_UNEXPECTED, `the predecessor ${predecessor.pid} died with the successor`);
    }
    const ownerPid = await daemonPidAt(ctx.daemonSocketPath, ctx.evidence);
    if (ownerPid !== predecessor.pid) {
      throw fail(
        FAIL.DECISION_UNEXPECTED,
        `${ctx.daemonSocketPath} answers as ${ownerPid ?? 'nothing'}, not as the surviving predecessor ${predecessor.pid}`,
      );
    }

    // The decision is the whole point of (a): the commit boundary was reached and nothing durable
    // was written, so the sessions were never transferred in the sense that matters.
    const afterKill = await readDecision(prepared.legacySocketPath);
    if (afterKill.present) {
      throw fail(
        FAIL.DECISION_TOO_EARLY,
        `${afterKill.path} records state ${afterKill.state} although the commit line never reached the predecessor`,
      );
    }
    // The same check the (c) halves make: no record *and* no staging file, so a decision that was
    // attempted without becoming durable cannot be mistaken for one that was never attempted.
    const noDecision = await assertNoDecision(
      ctx.paths.runtimeDir,
      'successor died before the decision',
      prepared.legacySocketPath,
    );
    if (!(await exists(prepared.legacySocketPath))) {
      throw fail(FAIL.DECISION_UNEXPECTED, `the predecessor unlinked its legacy socket ${prepared.legacySocketPath}`);
    }

    // The positive proof of the export freeze: the reader for the exported session is parked, which
    // only happens after `export_session` succeeded. A predecessor that never exported would never
    // report `readerPaused` true here.
    //
    // `pause_reader` only arms `pause_requested`; the reader parks on its next turn of the loop, so
    // the flag follows the read that returns *after* the export, and a reader blocked on a quiet PTY
    // reports `readerPaused` false until something makes that read return. One byte of input is what
    // makes it return; the flag is then waited for under a bound instead of sampled once and hoped
    // for. The byte goes over this control connection because input is `DaemonRequest::Write` there -
    // never over the attach stream, which is output-only.
    let frozen;
    try {
      await ctx.daemon.write(fixture.sessionId, Buffer.from('\n', 'utf8'));
      frozen = await until(
        `session ${fixture.sessionId} reports readerPaused while the predecessor holds a prepared handover`,
        15_000,
        async () => {
          const described = await ctx.daemon.describeSession(fixture.sessionId);
          return described.readerPaused === true ? described : null;
        },
      );
    } catch (error) {
      if (!(error instanceof AcceptanceError && error.code === FAIL.DEADLINE)) throw error;
      throw fail(FAIL.SERVE_PROOF_MISSING, error.message);
    }
    await ctx.evidence.record({
      step: 'a-export-freeze-observed',
      sessionId: fixture.sessionId,
      readerPaused: frozen.readerPaused,
    });

    // The real resume gate. `resume_paused_readers()` runs only from this handler and it requires
    // `HandoverStatus::Prepared`, so an accepted abort is itself evidence the predecessor never left
    // the pre-decision state - which is the claim (a) makes about the predecessor.
    //
    // The accounting subscription opens here, strictly before the abort that should emit the warning:
    // a subscription opened afterwards could not tell a missing record from one not yet flushed.
    const abortWatch = await AccountingWatch.open(ctx, ctx.evidence, ctx.paths.dataDir);
    let aborted;
    let abortedWarning;
    try {
      await ctx.daemon.abortHandover();
      aborted = await readDecision(prepared.legacySocketPath);
      abortedWarning = await abortWatch.expect(aborted, 'abort decision');
    } finally {
      abortWatch.close();
    }
    if (aborted.settled !== 'Active') {
      throw fail(
        FAIL.DECISION_UNEXPECTED,
        `${aborted.path} settled as ${aborted.settled ?? 'nothing'} after an accepted AbortHandover`,
      );
    }
    if (aborted.transaction.rollbackReason !== ABORT_ROLLBACK_REASON) {
      throw fail(FAIL.DECISION_UNEXPECTED, `rollbackReason is ${JSON.stringify(aborted.transaction.rollbackReason)}`);
    }
    // Plan Phase 4 first bullet, on the abort decision: the record must account for the very session
    // it was deciding about, and must declare the successor identity unused rather than leave it to be
    // misread as "never committed". Asserted against the bytes the daemon wrote, not a fixture.
    const abortedAccounting = assertDecisionAccounting(aborted, {
      sessionId: fixture.sessionId,
      expectUnpopulated: 'successorPid',
      label: 'aborted decision record',
    });
    const abortedCaptured = await captureDecision(ctx, 'a-abort.transaction', aborted);
    await untilAbsent(prepared.legacySocketPath, 'the aborted legacy socket', 15_000);

    // The resume is asynchronous for the same reason the pause is: `resume_reader` clears
    // `pause_requested` and notifies, and the reader clears `reader_paused` when its park returns.
    // Sampled once this raced the thread; waited for under a bound it asserts the same thing.
    try {
      await until(
        `session ${fixture.sessionId} reports readerPaused false after AbortHandover`,
        15_000,
        async () => ((await ctx.daemon.describeSession(fixture.sessionId)).readerPaused === false ? true : null),
      );
    } catch (error) {
      if (!(error instanceof AcceptanceError && error.code === FAIL.DEADLINE)) throw error;
      throw fail(FAIL.SERVE_PROOF_MISSING, error.message);
    }
    const served = await proveServing(ctx, fixture.sessionId, randomUUID());
    if (served.servedBy !== predecessor.pid) {
      throw fail(FAIL.SERVE_PROOF_MISSING, `the serve proof came from ${served.servedBy}, not the predecessor`);
    }

    // The retry: a fresh prepare mints a fresh path (and so a fresh decision record), and this
    // candidate is not proxied at all - it runs the production path end to end.
    const retried = await ctx.daemon.prepareHandover();
    if (retried.legacySocketPath === prepared.legacySocketPath) {
      throw fail(FAIL.PROTOCOL, `the retried handover reused the aborted socket path ${retried.legacySocketPath}`);
    }
    if (!retried.activeSessions.includes(fixture.sessionId)) {
      throw fail(FAIL.SERVE_PROOF_MISSING, `the retry offered ${JSON.stringify(retried.activeSessions)} without ${fixture.sessionId}`);
    }

    // Subscribed before the retry's candidate starts, so the commit decision's warning is observed as
    // a new record rather than counted against a snapshot taken after it landed.
    const commitWatch = await AccountingWatch.open(ctx, ctx.evidence, ctx.paths.dataDir);
    const second = await ctx.startSuccessor(retried.legacySocketPath);
    await awaitDaemonPid(second.pid, ctx.daemonSocketPath, ctx.evidence, 180_000);
    const predecessorExit = await predecessor.waitForExit(120_000);
    if (predecessorExit.code !== 0) {
      throw fail(
        FAIL.DECISION_UNEXPECTED,
        `the committed predecessor left with ${JSON.stringify(predecessorExit)}, not exit 0`,
      );
    }

    const committed = await readDecision(retried.legacySocketPath);
    if (committed.settled !== 'Retired') {
      throw fail(FAIL.DECISION_UNEXPECTED, `${committed.path} settled as ${committed.settled ?? 'nothing'}, not Retired`);
    }
    if (committed.transaction.rollbackReason != null) {
      throw fail(FAIL.DECISION_UNEXPECTED, `the committed record carries rollbackReason ${JSON.stringify(committed.transaction.rollbackReason)}`);
    }
    // The committed record of the retried transfer: same session named, same by-construction
    // declaration, and no rollback reason - so the record a reader sees after a commit cannot be
    // mistaken for the earlier abort.
    const committedAccounting = assertDecisionAccounting(committed, {
      sessionId: fixture.sessionId,
      expectUnpopulated: 'successorPid',
      label: 'committed decision record (retry)',
    });
    const committedCaptured = await captureDecision(ctx, 'a-commit.transaction', committed);
    const committedWarning = await commitWatch.expect(committed, 'commit decision').finally(() => commitWatch.close());
    await untilAbsent(retried.legacySocketPath, 'the committed legacy socket', 30_000);
    // The aborted record from the first attempt is untouched: a later decision does not rewrite it.
    const firstRecord = await readDecision(prepared.legacySocketPath);
    if (firstRecord.settled !== 'Active') {
      throw fail(
        FAIL.DECISION_UNEXPECTED,
        `the first attempt's record settled as ${firstRecord.settled ?? 'nothing'} after a later commit`,
      );
    }

    const successorClient = await connectNow(ctx);
    if (Number(successorClient.identity.pid) !== second.pid) {
      throw fail(FAIL.IDENTITY_MISMATCH, `${ctx.daemonSocketPath} answers as ${successorClient.identity.pid}, not the successor ${second.pid}`);
    }
    const survivors = (await successorClient.liveSessions()).filter((entry) => entry.backendId === fixture.sessionId);
    if (survivors.length !== 1) {
      throw fail(
        FAIL.SERVE_PROOF_MISSING,
        `the committed successor lists ${survivors.length} entries for ${fixture.sessionId}, expected exactly one`,
      );
    }
    const survivor = survivors[0];
    if (Number(survivor.localChildPid) !== fixture.pid) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `the retried handover serves pid ${survivor.localChildPid}, the original child was ${fixture.pid}`,
      );
    }
    const identity = assertProcessIdentity(
      await processTable(),
      fixture.pid,
      survivor.localChildStartTime,
      `retried session ${fixture.sessionId}`,
      'ms',
    );
    if (identity.pid !== fixture.identity.pid) {
      throw fail(FAIL.IDENTITY_MISMATCH, `the retried session's identity is ${JSON.stringify(identity)}`);
    }
    const servedAfter = await proveServing(ctx, fixture.sessionId, randomUUID());
    if (servedAfter.servedBy !== second.pid) {
      throw fail(FAIL.SERVE_PROOF_MISSING, `the post-commit serve proof came from ${servedAfter.servedBy}, not the successor`);
    }

    return {
      sessionId: fixture.sessionId,
      childPid: fixture.pid,
      predecessorPid: predecessor.pid,
      successorPid: second.pid,
      injectedInto: successor.pid,
      decisionAfterKill: 'absent',
      noDecisionAfterKill: noDecision,
      decisionAfterAbort: aborted.settled,
      decisionAfterRetry: committed.settled,
      accounting: {
        abort: abortedAccounting,
        commit: committedAccounting,
        abortCaptured: abortedCaptured,
        commitCaptured: committedCaptured,
        abortWarning: abortedWarning,
        commitWarning: committedWarning,
      },
      readerPausedDuringCommit: frozen.readerPaused,
      serveProofReplies: [served.replyBytes, servedAfter.replyBytes],
      processStreams: await captureProcessStreams(ctx, 'a', {
        predecessor,
        injectedCandidate: successor,
        successor: second,
      }),
      retriedLegacySocket: retried.legacySocketPath,
      note: 'the kill is performed inside the relay decision, so it is ordered before the withheld commitHandover line',
    };
  },
);

/**
 * Plan rev5 (b): a successor that dies AFTER the decision leaves nobody able to serve those PTYs -
 * not the retired predecessor (whose authority and socket are gone) and not a fresh daemon (which
 * never adopted them). The PTY child must be gone with its last owner.
 *
 * No injection is needed here on purpose: the death this case is about happens after the commit, so
 * the handover runs production end to end and the crash is the successor's SIGKILL.
 */
defineCase(
  'b-successor-dies-after-decision',
  'successor killed after a committed handover: the predecessor never resumes, and nobody else can serve or adopt the PTYs',
  ['daemon'],
  async (ctx) => {
    const predecessor = ctx.daemon.process;
    const fixture = await spawnFixture(ctx, ctx.daemon);

    // Subscribed before anything decides: the commit in this case writes its record during the
    // handover, so a subscription opened afterwards could not order the warning against it.
    const commitWatch = await AccountingWatch.open(ctx, ctx.evidence, ctx.paths.dataDir);
    const prepared = await ctx.daemon.prepareHandover();
    const successor = await ctx.startSuccessor(prepared.legacySocketPath);

    // Commit completion is observed as authority changing hands on one fixed path: the successor
    // serves the predecessor's own canonical listener, so the socket starts reporting its pid.
    await awaitDaemonPid(successor.pid, ctx.daemonSocketPath, ctx.evidence, 180_000);
    const predecessorExit = await predecessor.waitForExit(120_000);
    if (predecessorExit.code !== 0) {
      throw fail(FAIL.DECISION_UNEXPECTED, `the committed predecessor left with ${JSON.stringify(predecessorExit)}`);
    }

    // Durable post-decision state: the decision is Retired (so `resolve_commit(false, Retired)` is
    // `Owner`, i.e. the predecessor may never roll back), and its legacy socket is unlinked, so no
    // attach path to the predecessor exists at all.
    const decision = await readDecision(prepared.legacySocketPath);
    if (decision.settled !== 'Retired') {
      throw fail(FAIL.DECISION_UNEXPECTED, `${decision.path} settled as ${decision.settled ?? 'nothing'}, not Retired`);
    }
    // Plan Phase 4 accounting, post-decision: the committed record names the transferred session and
    // declares the successor identity unused by construction. This is the record a later reader
    // consults to learn what moved, so it must not be silent about it.
    const committedAccounting = assertDecisionAccounting(decision, {
      sessionId: fixture.sessionId,
      expectUnpopulated: 'successorPid',
      label: 'committed decision record (successor died after it)',
    });
    const committedCaptured = await captureDecision(ctx, 'b-commit.transaction', decision);
    const committedWarning = await commitWatch.expect(decision, 'commit decision (b)').finally(() => commitWatch.close());
    await untilAbsent(prepared.legacySocketPath, 'the retired legacy socket', 30_000);
    const dangling = await decisionArtifacts(ctx.paths.runtimeDir);
    const settledNames = await Promise.all(dangling.map(async (name) => ({ name, record: await readDecision(join(ctx.paths.runtimeDir, name.replace(/\.transaction$/, '.sock'))) })));
    const active = settledNames.filter((entry) => entry.record.settled === 'Active');
    if (active.length > 0) {
      throw fail(
        FAIL.DECISION_UNEXPECTED,
        `a rollback record exists beside the committed one: ${JSON.stringify(active.map((entry) => entry.name))}`,
      );
    }

    // The successor really owns the session: same child pid and start time as before the handover.
    const successorClient = await connectNow(ctx);
    if (Number(successorClient.identity.pid) !== successor.pid) {
      throw fail(FAIL.IDENTITY_MISMATCH, `${ctx.daemonSocketPath} answers as ${successorClient.identity.pid}, not ${successor.pid}`);
    }
    const owned = (await successorClient.liveSessions()).filter((entry) => entry.backendId === fixture.sessionId);
    if (owned.length !== 1 || Number(owned[0].localChildPid) !== fixture.pid) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `the successor lists ${JSON.stringify(owned)} for ${fixture.sessionId}; expected one entry for pid ${fixture.pid}`,
      );
    }
    const served = await proveServing(ctx, fixture.sessionId, randomUUID());

    // The crash this case is about. Held master descriptors die with their last holder.
    const crash = await successor.kill('SIGKILL');
    if (crash.signal !== 'SIGKILL') {
      throw fail(FAIL.PROTOCOL, `the successor left through ${JSON.stringify(crash)}`);
    }
    await requireGone(
      fixture.pid,
      'the transferred PTY child',
      FAIL.PTY_SURVIVED_LOSS,
      'with the predecessor retired and the successor dead, nobody can serve this PTY, so a survivor means the handover never moved it',
      45_000,
    );
    const refusedBecause = await expectRefused(ctx.daemonSocketPath, 'the canonical socket after the successor died', ctx.evidence);

    // A fresh ordinary daemon in the same runtime: it inherits the durable record and nothing else,
    // and must adopt no PTY from it.
    const recovery = await ctx.startDaemon({ role: 'recovery' });
    const listed = await recovery.liveSessions();
    const adopted = listed.filter((entry) => entry.backendId === fixture.sessionId || Number(entry.localChildPid) === fixture.pid);
    if (adopted.length > 0) {
      throw fail(FAIL.ADOPTION_WITHOUT_DECISION, `a fresh daemon lists ${JSON.stringify(adopted)} after an ownerless PTY died`);
    }
    let attachRefusal = null;
    let attachSucceeded = false;
    try {
      const attachment = await recovery.attach(fixture.sessionId, null);
      attachSucceeded = true;
      attachment.stream.close();
    } catch (error) {
      if (!(error instanceof DaemonRefusal)) throw error;
      attachRefusal = error.code ?? error.text;
    }
    let writeRefusal = null;
    try {
      await recovery.write(fixture.sessionId, Buffer.from('x', 'utf8'));
    } catch (error) {
      if (!(error instanceof DaemonRefusal)) throw error;
      writeRefusal = error.code ?? error.text;
    }
    await ctx.evidence.record({
      step: 'b-fresh-daemon-refusals',
      attachRefusal,
      writeRefusal,
      attachSucceeded,
      listed: listed.length,
      socketRefusal: refusedBecause,
    });
    if (attachSucceeded) {
      throw fail(
        FAIL.ADOPTION_WITHOUT_DECISION,
        `a fresh daemon attached session ${fixture.sessionId}, whose PTY no live process holds`,
      );
    }
    if (writeRefusal === null) {
      throw fail(FAIL.SERVE_PROOF_MISSING, `a fresh daemon accepted a write to ${fixture.sessionId} without a PTY`);
    }

    return {
      sessionId: fixture.sessionId,
      childPid: fixture.pid,
      predecessorPid: predecessor.pid,
      successorPid: successor.pid,
      predecessorExit,
      decision: decision.settled,
      accounting: { commit: committedAccounting, captured: committedCaptured, warning: committedWarning },
      legacySocketUnlinked: true,
      ptyChildGone: true,
      socketRefusal: refusedBecause,
      attachRefusal,
      writeRefusal,
      serveProofReplies: served.replyBytes,
      processStreams: await captureProcessStreams(ctx, 'b', { predecessor, successor }),
      note: 'the successor was killed with no injection at all, so the death is strictly after the durable decision',
    };
  },
);

/**
 * Plan rev5 (c), direct-owned half: a predecessor that dies BEFORE the decision loses its sessions.
 *
 * The recorded authority is the dead predecessor's durable state, and it wrote no decision, so no
 * successor may adopt anything it held. The candidate staged the exports (adoption precedes the
 * commit request in production), so this case also proves the candidate gives them back rather than
 * keeping a PTY whose owner never authorised the move.
 */
defineCase(
  'c-predecessor-dies-before-decision-direct',
  'predecessor killed at CommitHandover: no decision is written, the candidate exits non-zero without serving, and the direct-owned session is lost',
  ['daemon'],
  async (ctx) => {
    const predecessor = ctx.daemon.process;
    const fixture = await spawnFixture(ctx, ctx.daemon);

    const prepared = await ctx.daemon.prepareHandover();
    if (!prepared.activeSessions.includes(fixture.sessionId)) {
      throw fail(
        FAIL.PROTOCOL,
        `prepareHandover offered ${JSON.stringify(prepared.activeSessions)} without the live session ${fixture.sessionId}`,
      );
    }
    // The pre-death half of "real attach verified": before the kill, the direct-owned session must
    // demonstrably be served by the predecessor itself. Without it, "the fresh daemon refused"
    // below would also hold for a session that never worked in the first place.
    const servedBeforeDeath = await proveServing(ctx, fixture.sessionId, randomUUID());
    if (Number(servedBeforeDeath.servedBy) !== predecessor.pid) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `${servedBeforeDeath.servedBy} answered for ${fixture.sessionId} before the kill, not the predecessor ${predecessor.pid}`,
      );
    }

    let successor = null;
    let killed = false;
    let relay = null;
    let resolveCommit = null;
    const commitSeen = new Promise((resolveSeen) => {
      resolveCommit = resolveSeen;
    });
    relay = await ctx.startRelay({
      upstreamPath: prepared.legacySocketPath,
      decide: async (request) => {
        if (request?.type !== 'commitHandover') return 'forward';
        if (killed) return 'drop';
        killed = true;
        await ctx.evidence.record({
          step: 'inject',
          variant: 'c-predecessor-dies-before-decision-direct',
          at: 'commitHandover',
          successorPid: successor.pid,
          predecessorPid: predecessor.pid,
          verbsSeen: relay.lines.slice(),
        });
        const outcome = await predecessor.kill('SIGKILL');
        resolveCommit(outcome);
        return 'drop';
      },
    });

    successor = await ctx.startSuccessor(ctx.paths.relayPath);
    const injected = await bounded(commitSeen, 'the successor reached the commit boundary', 180_000);
    if (injected.signal !== 'SIGKILL') {
      throw fail(FAIL.PROTOCOL, `the injected death reported ${JSON.stringify(injected)}, not SIGKILL`);
    }

    // The candidate must refuse to become an owner: its commit was never confirmed and the record
    // beside the legacy path is absent, so `resolve_commit` is `Abort`, not `Owner`.
    const successorExit = await successor.waitForExit(120_000);
    if (successorExit.signal !== null || successorExit.code === 0) {
      throw fail(
        FAIL.DECISION_UNEXPECTED,
        `the candidate left through ${JSON.stringify(successorExit)}; a candidate whose predecessor died before the decision must exit non-zero and must not be killed`,
      );
    }

    // Nothing durable exists: authority stayed with the dead predecessor's own state, and that state
    // records no decision at all - neither a durable record nor a staging file left mid-write.
    const noDecision = await assertNoDecision(
      ctx.paths.runtimeDir,
      'predecessor died before the decision',
      prepared.legacySocketPath,
    );
    const record = await readDecision(prepared.legacySocketPath);
    if (record.present) {
      throw fail(
        FAIL.DECISION_TOO_EARLY,
        `${record.path} records state ${record.state} although the commit line never reached the predecessor`,
      );
    }

    // A crashed predecessor leaves its socket files behind, so this is the listener being gone rather
    // than the path being unlinked. Both forms are a refusal; the code records which one.
    const refusedBecause = await expectRefused(
      ctx.daemonSocketPath,
      'the canonical socket after the predecessor was killed',
      ctx.evidence,
    );

    // Direct-owned sessions are lost with their only owner: the candidate relinquished the exports
    // it had staged, so no master descriptor is left anywhere.
    await requireGone(
      fixture.pid,
      'the direct-owned PTY child',
      FAIL.DIRECT_SESSION_NOT_LOST,
      'the predecessor died before the decision and the candidate aborted, so nothing can serve this PTY',
      45_000,
    );

    // A fresh ordinary daemon in the same runtime inherits the durable record - which is empty - and
    // must adopt nothing from it.
    const recovery = await ctx.startDaemon({ role: 'recovery' });
    const listed = await recovery.liveSessions();
    const adopted = listed.filter(
      (entry) => entry.backendId === fixture.sessionId || Number(entry.localChildPid) === fixture.pid,
    );
    let attachSucceeded = false;
    let attachRefusal = null;
    try {
      const attachment = await recovery.attach(fixture.sessionId, null);
      attachSucceeded = true;
      attachment.stream.close();
    } catch (error) {
      if (!(error instanceof DaemonRefusal)) throw error;
      attachRefusal = error.code ?? error.text;
    }
    let writeRefusal = null;
    try {
      await recovery.write(fixture.sessionId, Buffer.from('x', 'utf8'));
    } catch (error) {
      if (!(error instanceof DaemonRefusal)) throw error;
      writeRefusal = error.code ?? error.text;
    }
    await ctx.evidence.record({
      step: 'c-direct-recovery-refusals',
      attachRefusal,
      writeRefusal,
      attachSucceeded,
      listed: listed.length,
      adopted: adopted.length,
      socketRefusal: refusedBecause,
      candidateExit: successorExit,
      relayVerbs: relay.lines.slice(),
      relayReplies: relay.replies.slice(),
    });
    if (adopted.length > 0) {
      throw fail(
        FAIL.ADOPTION_WITHOUT_DECISION,
        `a fresh daemon lists ${JSON.stringify(adopted)} although the predecessor recorded no decision`,
      );
    }
    if (attachSucceeded) {
      throw fail(
        FAIL.ADOPTION_WITHOUT_DECISION,
        `a fresh daemon attached session ${fixture.sessionId}, which no live process holds`,
      );
    }
    if (writeRefusal === null) {
      throw fail(FAIL.SERVE_PROOF_MISSING, `a fresh daemon accepted a write to ${fixture.sessionId} without a PTY`);
    }

    return {
      sessionId: fixture.sessionId,
      childPid: fixture.pid,
      predecessorPid: predecessor.pid,
      predecessorExit: injected,
      candidatePid: successor.pid,
      candidateExit: successorExit,
      decision: 'absent',
      noDecision,
      processStreams: await captureProcessStreams(ctx, 'c-direct', { predecessor, candidate: successor }),
      socketRefusal: refusedBecause,
      directSessionLost: true,
      serveProofBeforeDeath: servedBeforeDeath,
      attachRefusal,
      writeRefusal,
      relayVerbs: relay.lines.slice(),
      note: 'the kill happens inside the relay decision, so the predecessor is dead strictly before the withheld commitHandover line',
    };
  },
);

/**
 * Plan rev5 (c), keeper-owned half: the same predecessor death, but the session that must survive is
 * owned by the keeper rather than by the daemon.
 *
 * Both halves run in one case on purpose, because the contrast is the claim: the daemon's own child is
 * lost, and the keeper's child is untouched. The keeper is driven on its real wire (spawn, ownership
 * discovery, head, attach), which is what makes "discoverable and recoverable" observable rather than
 * asserted.
 */
defineCase(
  'c-keeper-owned-session-survives-predecessor-death',
  'predecessor killed at CommitHandover: its direct session is lost, while the keeper-owned session stays discoverable and serveable',
  ['daemon', 'keeper'],
  async (ctx) => {
    const keeper = ctx.keeper;
    // The premise is a *direct*-owned fixture, and it is not free: the runner already started this
    // case's daemon while the keeper was up, so its startup pass enabled keeper ownership and every
    // subsequent spawn is keeper-routed. A "direct" fixture created under it is in fact keeper-owned,
    // whose child correctly survives the predecessor's SIGKILL - run12 reported exactly that ("pid
    // 73222 was still running when the bound expired"), because the case was asserting a survival
    // property that its own premise had already voided. `FERRYX_KEEPER_OWNERSHIP=disabled` pins the
    // routing for a fresh predecessor (`keeper_startup.rs::OwnershipPolicy`). The predecessor is
    // replaced *before* the keeper fixture exists, so the fresh daemon's startup discovery has no
    // keeper session to adopt and `prepareHandover` still genuinely does not offer one.
    await ctx.daemon.process.stop();
    ctx.daemon.close();
    ctx.daemon = await ctx.startDaemon({ role: 'c-direct-predecessor', ownership: 'disabled' });

    const keeperFixture = await spawnKeeperFixture(ctx);
    const firstServe = await proveKeeperServing(ctx, keeperFixture.sessionId);

    const predecessor = ctx.daemon.process;
    const directFixture = await spawnFixture(ctx, ctx.daemon);
    // Measured, not assumed: the premise only holds if the keeper's own registry does not list this
    // session. A keeper-routed spawn would put it there and the survival assertion below would be
    // asking the keeper to lose something it owns.
    const directRouted = (await keeper.ownership()).filter((entry) => entry.sessionId === directFixture.sessionId);
    await ctx.evidence.record({
      step: 'c-direct-fixture-routing',
      directSessionId: directFixture.sessionId,
      keeperListed: directRouted.length,
    });
    if (directRouted.length > 0) {
      throw fail(
        FAIL.PROTOCOL,
        `${directFixture.sessionId} is keeper-owned under a predecessor started with ${KEEPER_OWNERSHIP_ENV}=disabled, so there is no direct-owned session whose loss this case could measure`,
      );
    }
    const prepared = await ctx.daemon.prepareHandover();
    if (!prepared.activeSessions.includes(directFixture.sessionId)) {
      throw fail(
        FAIL.PROTOCOL,
        `prepareHandover offered ${JSON.stringify(prepared.activeSessions)} without the direct session ${directFixture.sessionId}`,
      );
    }
    // The keeper's session is not the daemon's to offer: it is a separate owner, so the handover
    // neither moves it nor can lose it.
    if (prepared.activeSessions.includes(keeperFixture.sessionId)) {
      throw fail(
        FAIL.PROTOCOL,
        `prepareHandover offered the keeper-owned session ${keeperFixture.sessionId}, which this daemon does not own`,
      );
    }

    let successor = null;
    let killed = false;
    let resolveCommit = null;
    const commitSeen = new Promise((resolveSeen) => {
      resolveCommit = resolveSeen;
    });
    await ctx.startRelay({
      upstreamPath: prepared.legacySocketPath,
      decide: async (request) => {
        if (request?.type !== 'commitHandover') return 'forward';
        if (killed) return 'drop';
        killed = true;
        await ctx.evidence.record({
          step: 'inject',
          variant: 'c-keeper-owned-session-survives-predecessor-death',
          at: 'commitHandover',
          successorPid: successor.pid,
          predecessorPid: predecessor.pid,
        });
        const outcome = await predecessor.kill('SIGKILL');
        resolveCommit(outcome);
        return 'drop';
      },
    });

    successor = await ctx.startSuccessor(ctx.paths.relayPath);
    const injected = await bounded(commitSeen, 'the successor reached the commit boundary', 180_000);
    if (injected.signal !== 'SIGKILL') {
      throw fail(FAIL.PROTOCOL, `the injected death reported ${JSON.stringify(injected)}, not SIGKILL`);
    }
    const successorExit = await successor.waitForExit(120_000);
    if (successorExit.signal !== null || successorExit.code === 0) {
      throw fail(
        FAIL.DECISION_UNEXPECTED,
        `the candidate left through ${JSON.stringify(successorExit)}; it must exit non-zero after losing its predecessor before the decision`,
      );
    }
    // Same fact as the direct half, checked on the keeper half too: the predecessor's death wrote no
    // decision and left no staging file, so nothing on disk can be read as authorising an adoption.
    const noDecision = await assertNoDecision(
      ctx.paths.runtimeDir,
      'predecessor died before the decision (keeper case)',
      prepared.legacySocketPath,
    );

    // The contrast: the daemon's own session goes with its owner.
    await requireGone(
      directFixture.pid,
      'the direct-owned PTY child',
      FAIL.DIRECT_SESSION_NOT_LOST,
      'the predecessor died before the decision, so its directly-owned PTY has no owner left',
      45_000,
    );

    // Losing the child is half the claim; the other half is that no successor takes it. A daemon that
    // adopted the dead PTY would be serving a corpse, so this asks the request gates directly rather
    // than inferring "not adopted" from the process being gone.
    const recovery = await ctx.startDaemon({ role: 'recovery' });
    const recovered = await recovery.liveSessions();
    const adoptedDirect = recovered.filter(
      (entry) => entry.backendId === directFixture.sessionId || Number(entry.localChildPid) === directFixture.pid,
    );
    let directAttachRefusal = null;
    let directAttachSucceeded = false;
    try {
      const attachment = await recovery.attach(directFixture.sessionId, null);
      directAttachSucceeded = true;
      attachment.stream.close();
    } catch (error) {
      if (!(error instanceof DaemonRefusal)) throw error;
      directAttachRefusal = error.code ?? error.text;
    }
    let directWriteRefusal = null;
    try {
      await recovery.write(directFixture.sessionId, Buffer.from('x', 'utf8'));
    } catch (error) {
      if (!(error instanceof DaemonRefusal)) throw error;
      directWriteRefusal = error.code ?? error.text;
    }
    await ctx.evidence.record({
      step: 'c-keeper-direct-session-gates',
      directSessionId: directFixture.sessionId,
      directChildPid: directFixture.pid,
      listed: recovered.length,
      adoptedDirect: adoptedDirect.length,
      directAttachRefusal,
      directWriteRefusal,
      directAttachSucceeded,
    });
    if (adoptedDirect.length > 0) {
      throw fail(
        FAIL.ADOPTION_WITHOUT_DECISION,
        `a fresh daemon lists ${JSON.stringify(adoptedDirect)} although the predecessor recorded no decision and its PTY is dead`,
      );
    }
    if (directAttachSucceeded) {
      throw fail(
        FAIL.ADOPTION_WITHOUT_DECISION,
        `a fresh daemon attached ${directFixture.sessionId}, whose PTY no live process holds`,
      );
    }
    if (directWriteRefusal === null) {
      throw fail(FAIL.SERVE_PROOF_MISSING, `a fresh daemon accepted a write to ${directFixture.sessionId} without a PTY`);
    }
    // Recorded, not asserted: whether a fresh daemon also lists the keeper-owned session is a fact
    // about the adoption path this build may or may not have. If it adopts, the keeper half's
    // "recoverable" claim only gets stronger, and `c-keeper-session-adopted-by-daemon` is the case
    // that owns that verdict - failing here would contradict it.
    const adoptedKeeper = recovered.filter((entry) => entry.backendId === keeperFixture.sessionId);
    await ctx.evidence.record({
      step: 'c-keeper-recovery-daemon-view',
      keeperSessionId: keeperFixture.sessionId,
      adoptedKeeper: adoptedKeeper.length,
      entries: adoptedKeeper,
    });

    // The keeper is a separate process and must be untouched by the daemon's death.
    if (!processAlive(keeperFixture.pid)) {
      throw fail(FAIL.LEAKED_PROCESS, `the keeper's child ${keeperFixture.pid} died with the daemon`);
    }
    if (ctx.keeperProcess && ctx.keeperProcess.pid === predecessor.pid) {
      throw fail(FAIL.PROTOCOL, 'the keeper and the predecessor are the same process, so this case proves nothing');
    }
    if (keeper.closed) {
      throw fail(FAIL.LEAKED_PROCESS, `the keeper connection closed with the predecessor: ${String(keeper.closed)}`);
    }

    // Authoritative discovery: the keeper's registry is the answer, and it still names the session
    // with the identity it spawned.
    const owned = (await keeper.ownership()).filter((entry) => entry.sessionId === keeperFixture.sessionId);
    if (owned.length !== 1) {
      throw fail(
        FAIL.SERVE_PROOF_MISSING,
        `the keeper reports ${owned.length} entries for ${keeperFixture.sessionId} after the daemon died, expected exactly one`,
      );
    }
    if (owned[0].pid !== keeperFixture.pid) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `the keeper reports pid ${owned[0].pid} for ${keeperFixture.sessionId}, it spawned ${keeperFixture.pid}`,
      );
    }
    const identity = assertProcessIdentity(
      await processTable(),
      keeperFixture.pid,
      keeperFixture.startTime,
      `keeper session ${keeperFixture.sessionId} after the daemon died`,
      'us',
    );
    if (identity.pid !== keeperFixture.identity.pid) {
      throw fail(FAIL.IDENTITY_MISMATCH, `the keeper session identity is ${JSON.stringify(identity)}`);
    }

    // Recoverable: a round trip through the keeper's own attach and replay path, after the death.
    const secondServe = await proveKeeperServing(ctx, keeperFixture.sessionId);
    if (secondServe.headAfter <= firstServe.headAfter) {
      throw fail(
        FAIL.SERVE_PROOF_MISSING,
        `the keeper's replay head ${secondServe.headAfter} did not advance past ${firstServe.headAfter} after the daemon died`,
      );
    }

    return {
      keeperSessionId: keeperFixture.sessionId,
      keeperChildPid: keeperFixture.pid,
      keeperPid: ctx.keeperProcess?.pid ?? null,
      directSessionId: directFixture.sessionId,
      directChildPid: directFixture.pid,
      predecessorPid: predecessor.pid,
      predecessorExit: injected,
      candidateExit: successorExit,
      decision: 'absent',
      noDecision,
      directRecovery: {
        listed: recovered.length,
        adoptedDirect: adoptedDirect.length,
        adoptedKeeper: adoptedKeeper.length,
        attachRefusal: directAttachRefusal,
        writeRefusal: directWriteRefusal,
      },
      processStreams: await captureProcessStreams(ctx, 'c-keeper', {
        predecessor,
        candidate: successor,
        keeper: ctx.keeperProcess,
      }),
      directSessionLost: true,
      keeperOwnership: owned[0],
      keeperOsIdentity: identity,
      keeperServeHeads: [firstServe.headAfter, secondServe.headAfter],
      note: 'the keeper-owned session is discovered and served again after its daemon died, while the daemon-owned session is gone',
    };
  },
);

/**
 * Plan rev5 (c), the recovery seam itself: can a daemon adopt a session the keeper owns?
 *
 * Measured, not assumed. The daemon spawns a session and the case asks the keeper whether it owns it.
 * If it does not, there is no keeper-owned session for any daemon to adopt, and the plan's recovery
 * requirement has no implementation to exercise - which is a typed gap, never a pass.
 */
defineCase(
  'c-keeper-session-adopted-by-daemon',
  'a daemon adopts a keeper-owned session across a predecessor death, or the missing adoption path is reported as a typed gap',
  ['daemon', 'keeper'],
  async (ctx) => {
    const keeper = ctx.keeper;
    const fixture = await spawnFixture(ctx, ctx.daemon);
    const owned = (await keeper.ownership()).filter((entry) => entry.sessionId === fixture.sessionId);
    await ctx.evidence.record({
      step: 'keeper-adoption-measured',
      sessionId: fixture.sessionId,
      childPid: fixture.pid,
      keeperOwnsSession: owned.length > 0,
      keeperOwnership: owned,
    });
    if (owned.length === 0) {
      const listed = (await keeper.ownership()).map((entry) => entry.sessionId);
      const startup = await keeperStartupEvidence(ctx);
      // Whether adoption was even reachable is computed from the binary this run was given, not
      // assumed: `ensure_keeper` only ever looks in a bundle payload.
      const bundle = bundleKeeperPayload(ctx.binaries.daemonBinary);
      const payloadPresent = bundle.payloadBinary ? await exists(bundle.payloadBinary) : false;
      await ctx.evidence.record({
        step: 'keeper-adoption-gap',
        sessionId: fixture.sessionId,
        childPid: fixture.pid,
        keeperOwnership: listed,
        bundle: { ...bundle, payloadPresent },
        ...startup,
      });
      throw gap(
        GAP.KEEPER_DAEMON_ADOPTION_ABSENT,
        `the daemon spawned ${fixture.sessionId} (pid ${fixture.pid}) through its own PTY manager and the keeper lists ${JSON.stringify(
          listed,
        )}. The daemon ran from ${bundle.daemonBinary} (bundle layout: ${bundle.bundleLayout ? 'Contents/MacOS' : 'not a bundle'}, payload ${bundle.payloadBinary ?? 'none'}, payload present: ${payloadPresent}), so the payload gate \`ensure_keeper\` -> \`bundle_keeper_dir(current_exe())\` was ${payloadPresent ? 'satisfied and the pass still did not activate keeper ownership' : 'closed, and that is the gap'}. The daemon's own startup line: ${startup.startupLine ?? 'none'}`,
      );
    }

    // The adoption path exists: a real predecessor death must leave the successor able to adopt.
    const predecessor = ctx.daemon.process;
    await predecessor.kill('SIGKILL');
    const recovery = await ctx.startDaemon({ role: 'recovery' });
    const listed = await recovery.liveSessions();
    const adopted = listed.filter((entry) => entry.backendId === fixture.sessionId);
    if (adopted.length !== 1) {
      throw fail(
        FAIL.ADOPTION_WITHOUT_DECISION,
        `the keeper owned ${fixture.sessionId} but a fresh daemon lists ${JSON.stringify(adopted)} for it`,
      );
    }
    if (Number(adopted[0].localChildPid) !== fixture.pid) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `the adopted entry serves pid ${adopted[0].localChildPid}, the keeper spawned ${fixture.pid}`,
      );
    }
    const served = await proveServing(ctx, fixture.sessionId, randomUUID());
    return {
      sessionId: fixture.sessionId,
      childPid: fixture.pid,
      predecessorPid: predecessor.pid,
      adopted: adopted[0],
      serveProofReplies: served.replyBytes,
    };
  },
);

/**
 * The isolation contract, exercised as a mutation matrix: every guard must be reachable and must fire
 * on the mutation that violates it. This case spawns nothing and creates nothing - it is pure path
 * arithmetic - so it runs even when no binary exists.
 *
 * The guards are ordered, so a weaker one can shadow a stronger one. Asserting the exact guard per
 * mutation is what pins that order: a reordering that lets `qa-label` answer for a production plist,
 * or `root-containment` answer for a production location, fails here.
 */
defineCase(
  'isolation-guard',
  'every isolation guard fires for its own mutation, and the unmutated layout passes',
  [],
  async (ctx) => {
    const uid = typeof process.getuid === 'function' ? process.getuid() : userInfo().uid;
    // Short on purpose, exactly like the run root: a `tmpdir()` base is already 48 bytes, which would
    // make the unmutated layout fail the socket budget instead of passing it.
    const base = planPaths(join('/tmp', `fx4g-${randomUUID().slice(0, 8)}`));
    const accepted = assertIsolation(structuredClone(base));

    const mutations = [
      {
        name: 'absolute',
        mutate: (paths) => {
          paths.runtimeDir = 'relative/rt';
        },
      },
      {
        name: 'production-location',
        mutate: (paths) => {
          paths.root = join('/tmp', `ferryx-${uid}`);
          paths.runtimeDir = join(paths.root, 'rt');
        },
      },
      {
        name: 'root-containment',
        mutate: (paths) => {
          paths.runtimeDir = join(tmpdir(), `fx4-outside-${randomUUID()}`);
        },
      },
      {
        name: 'runtime-legacy-overlap',
        mutate: (paths) => {
          paths.legacyRuntimeDir = join(paths.runtimeDir, 'legacy');
        },
      },
      {
        name: 'socket-budget',
        mutate: (paths) => {
          paths.runtimeDir = join(paths.root, 'x'.repeat(80));
        },
      },
      {
        name: 'production-label',
        mutate: (paths) => {
          paths.keeperPlistPath = join(paths.root, 'kpr', `${PRODUCTION_KEEPER_LABEL}.plist`);
        },
      },
      {
        name: 'inherited-home',
        mutate: (paths) => {
          // Once the keeper install pass runs, `write_keeper_launchd_plist` creates
          // `$HOME/Library/Logs/Ferryx`, so an inherited home is a write outside the fixture.
          paths.homeDir = homedir();
        },
      },
      {
        name: 'staged-bundle-outside-root',
        mutate: (paths) => {
          paths.appDir = join(tmpdir(), `fx4-app-${randomUUID()}`);
        },
      },
      {
        name: 'qa-label',
        mutate: (paths) => {
          paths.keeperPlistPath = join(paths.root, 'kpr', 'keeper.plist');
        },
      },
    ];

    const observed = [];
    for (const mutation of mutations) {
      const paths = structuredClone(base);
      mutation.mutate(paths);
      let thrown = null;
      try {
        assertIsolation(paths);
      } catch (error) {
        thrown = error;
      }
      if (!thrown) {
        throw fail(FAIL.SAFETY, `the ${mutation.name} mutation passed isolation, so that guard does not exist`);
      }
      if (!(thrown instanceof AcceptanceError) || thrown.code !== FAIL.SAFETY) {
        throw fail(FAIL.SAFETY, `the ${mutation.name} mutation failed with a non-safety error: ${String(thrown)}`);
      }
      if (thrown.extra?.guard !== mutation.name) {
        throw fail(
          FAIL.SAFETY,
          `the ${mutation.name} mutation was caught by ${JSON.stringify(thrown.extra?.guard ?? 'no guard')} instead, so the guard order is wrong`,
        );
      }
      observed.push({ mutation: mutation.name, guard: thrown.extra.guard, detail: thrown.detail });
    }

    // Every production location named by the isolation contract, not only the one the harness itself
    // would have used: a runtime under $HOME/.ferryx is as real as one under /tmp.
    const home = homedir();
    const productionRoots = [
      join('/tmp', `ferryx-${uid}`),
      join('/private/tmp', `ferryx-${uid}`),
      join('/tmp', `rorca-${uid}`),
      join('/private/tmp', `rorca-${uid}`),
      join(home, '.ferryx'),
    ];
    const productionObserved = [];
    for (const production of productionRoots) {
      const paths = structuredClone(base);
      paths.root = production;
      paths.runtimeDir = join(production, 'rt');
      let guard = null;
      try {
        assertIsolation(paths);
      } catch (error) {
        guard = error instanceof AcceptanceError ? error.extra?.guard ?? null : 'non-safety';
      }
      if (guard !== 'production-location') {
        throw fail(
          FAIL.SAFETY,
          `the production root ${production} was not refused as a production location: ${String(guard)}`,
        );
      }
      productionObserved.push({ production, guard });
    }

    return {
      acceptedLabel: accepted.label,
      acceptedSocketBudgets: accepted.sockets,
      mutations: observed,
      productionRoots: productionObserved,
      note: 'this case spawns no process and creates no path of its own: it is path arithmetic only',
    };
  },
);

// -------------------------------------------------------------------------------------------
// Runner
// -------------------------------------------------------------------------------------------

const USAGE = `Usage: node scripts/qa/phase4-handover-failure-injection.acceptance.mjs [options]

Runs the Phase 4 handover failure-injection acceptance cases against the real daemon (and, where the
plan requires it, the real keeper) binaries. Every run is isolated: a fresh short root under the OS
temp directory, its own FERRYX_RUNTIME_DIR and FERRYX_LEGACY_RUNTIME_DIR, its own keeper socket, its
own relay socket, and the QA launchd label. Production sessions, daemons, keepers and plists are
never read, started, signalled or written.

Cases map to plan rev 5 lines 248-263:
  (a) a-successor-dies-before-decision
  (b) b-successor-dies-after-decision
  (c) c-predecessor-dies-before-decision-direct
      c-keeper-owned-session-survives-predecessor-death
      c-keeper-session-adopted-by-daemon
isolation-guard is the harness's own contract check and spawns nothing. With no --case, the default
selection is the five gate cases, not the whole table: the gate runs and counts exactly those five.

Options:
  --daemon-binary <path>   ferryx daemon to run (default: FERRYX_DAEMON_BIN, then the src-tauri target)
  --keeper-binary <path>   ferryx-ptykeeper to run (default: FERRYX_PTY_KEEPER_BIN, then the crate target)
  --case <id>              run one case; repeatable
  --root <dir>             use this directory as the run root instead of a fresh /tmp/fx4-XXXXXX
  --list                   list case ids and exit
  --keep                   keep the root even when every case passes
  --json                   print the summary as JSON
  --allow-partial          run a selection that is not exactly the plan gate's five cases (debugging
                           only; the run then reports NOT ACCEPTED and never exits 0)
  -h, --help               print this text

Exit codes: 0 exactly the gate cases ran and all passed; 1 any case failed or was blocked (with
--allow-partial, also a selection that was not exactly the gate); 2 usage error, coverage drift or
an isolation refusal, all raised before anything is spawned.`;

function parseArgs(argv) {
  const options = {
    daemonBinary: process.env.FERRYX_DAEMON_BIN ?? null,
    keeperBinary: process.env.FERRYX_PTY_KEEPER_BIN ?? null,
    cases: [],
    root: null,
    list: false,
    keep: false,
    json: false,
    allowPartial: false,
    help: false,
  };
  const take = (name) => {
    const value = argv.shift();
    if (value === undefined || value.startsWith('--')) throw fail(FAIL.USAGE, `${name} needs a value`);
    return value;
  };
  while (argv.length > 0) {
    const flag = argv.shift();
    switch (flag) {
      case '--daemon-binary':
        options.daemonBinary = take(flag);
        break;
      case '--keeper-binary':
        options.keeperBinary = take(flag);
        break;
      case '--case':
        options.cases.push(take(flag));
        break;
      case '--root':
        options.root = take(flag);
        break;
      case '--list':
        options.list = true;
        break;
      case '--keep':
        options.keep = true;
        break;
      case '--json':
        options.json = true;
        break;
      case '--allow-partial':
        options.allowPartial = true;
        break;
      case '-h':
      case '--help':
        options.help = true;
        break;
      default:
        throw fail(FAIL.USAGE, `unknown argument ${JSON.stringify(flag)}`);
    }
  }
  return options;
}

/**
 * The selected cases, in table order.
 *
 * With no `--case`, the default is the gate set rather than the whole table: the documented bare
 * command has to be the thing the gate runs, and `isolation-guard` is a contract self-check that
 * spawns nothing, so including it by default would put six outcomes in front of a gate that requires
 * five. It stays selectable by name for a cheap pre-flight.
 */
function selectCases(options) {
  if (options.cases.length === 0) return CASES.filter((entry) => GATE_REQUIRED_CASES.includes(entry.id));
  const selected = [];
  for (const id of options.cases) {
    const found = CASES.find((entry) => entry.id === id);
    if (!found) throw fail(FAIL.USAGE, `unknown case ${JSON.stringify(id)}; run with --list to see the ids`);
    if (!selected.includes(found)) selected.push(found);
  }
  return selected;
}

/**
 * The cases the plan's gate needs, which of them this selection leaves out, and which it adds.
 *
 * A selection is not a gate run just because every case in it passed: `--case isolation-guard` alone
 * spawns nothing, and reporting that as an accepted Phase 4 gate would be a false pass over cases
 * that never ran. Coverage is therefore computed, and a selection that is not exactly the gate is
 * refused unless the operator opts out explicitly.
 *
 * Both directions matter, and for the same reason: a count the gate can read. A missing case is a
 * claim about work nobody did. An extra case is a count `combined-release-gate.mjs` rejects
 * (`counts.total !== requiredCases.length`, codes `GATE_CASE_COUNT_MISMATCH` and `GATE_CASE_FAILED`),
 * so a run carrying one cannot be a gate result even if every case in it passed. `isolation-guard`
 * is the only such case today; it is selectable by name, is never in the gate list, and is not in
 * the default selection.
 */
function auditSelection(selected) {
  const ids = new Set(selected.map((entry) => entry.id));
  const missing = GATE_REQUIRED_CASES.filter((id) => !ids.has(id));
  const extra = selected.map((entry) => entry.id).filter((id) => !GATE_REQUIRED_CASES.includes(id));
  return {
    required: [...GATE_REQUIRED_CASES],
    missing,
    extra,
    complete: missing.length === 0 && extra.length === 0,
  };
}

/**
 * The case table and the coverage contract must describe the same run.
 *
 * `coverage()` is hand-maintained prose, and a clause naming a case that no longer exists would let a
 * plan requirement read as covered while nothing executes it. Both directions are checked, plus the
 * gate list, so this can only fail closed.
 */
function auditCoverage() {
  const known = new Set(CASES.map((entry) => entry.id));
  const declared = coverage();
  const problems = [];
  const reached = new Set();
  for (const [clause, entry] of Object.entries(declared)) {
    if (!Array.isArray(entry.cases) || entry.cases.length === 0) {
      problems.push(`coverage clause ${clause} names no case`);
    }
    for (const id of entry.cases ?? []) {
      if (!known.has(id)) problems.push(`coverage clause ${clause} names ${id}, which is not a case`);
      else reached.add(id);
    }
    if (!Array.isArray(entry.asserts) || entry.asserts.length === 0) {
      problems.push(`coverage clause ${clause} asserts nothing`);
    }
  }
  // Every case must be reached by some clause, or it would run without any reported contract.
  for (const id of known) {
    if (!reached.has(id)) problems.push(`case ${id} is not named by any coverage clause`);
  }
  for (const id of GATE_REQUIRED_CASES) {
    if (!known.has(id)) problems.push(`gate-required case ${id} is not in the case table`);
    else if (!reached.has(id)) problems.push(`gate-required case ${id} is not reached by any coverage clause`);
  }
  if (problems.length > 0) {
    throw fail(FAIL.COVERAGE_DRIFT, problems.join('; '));
  }
  return { cases: [...known].sort(), gateRequired: [...GATE_REQUIRED_CASES] };
}

/** Whether `path` is a file this user can execute. A binary that cannot exec is the same gap as none. */
async function isExecutable(path) {
  try {
    const info = await stat(path);
    return info.isFile() && (info.mode & 0o111) !== 0;
  } catch {
    return false;
  }
}

/**
 * The first candidate that exists and is executable, or `null`. An explicitly supplied binary is
 * checked strictly and an unusable one is a usage error: silently falling back to a different binary
 * would run the cases against something the operator did not ask for.
 */
async function resolveBinary(explicit, candidates, label) {
  if (explicit) {
    const resolved = resolve(explicit);
    if (!(await isExecutable(resolved))) {
      throw fail(FAIL.USAGE, `${label} ${resolved} is not an executable file`);
    }
    return resolved;
  }
  for (const candidate of candidates) {
    if (await isExecutable(candidate)) return candidate;
  }
  return null;
}

async function resolveBinaries(options) {
  const daemonBinary = await resolveBinary(
    options.daemonBinary,
    [
      join(REPO_ROOT, 'src-tauri', 'target', 'release', 'ferryx'),
      join(REPO_ROOT, 'src-tauri', 'target', 'debug', 'ferryx'),
    ],
    '--daemon-binary',
  );
  const keeperBinary = await resolveBinary(
    options.keeperBinary,
    [
      join(REPO_ROOT, 'crates', 'ferryx-ptykeeper', 'target', 'release', 'ferryx-ptykeeper'),
      join(REPO_ROOT, 'crates', 'ferryx-ptykeeper', 'target', 'debug', 'ferryx-ptykeeper'),
    ],
    '--keeper-binary',
  );
  return { daemonBinary, keeperBinary };
}

/**
 * Prerequisites are facts about this host, resolved once. A `requires` entry that did not hold becomes
 * each dependent case's `blocked` outcome, so the report names which seam stopped which case instead
 * of collapsing the whole run.
 */
async function buildPrerequisites(binaries) {
  const platformSupported = process.platform === 'darwin' || process.platform === 'linux';
  const unix = typeof process.getuid === 'function';
  // Every daemon case proves identities against the OS, so `ps` is a prerequisite of all of them.
  const helper = (await exists('/bin/ps')) ? null : GAP.HELPER_ABSENT;
  const platformGap = !platformSupported || !unix ? GAP.PLATFORM_UNSUPPORTED : null;
  return {
    daemon: platformGap ?? (!binaries.daemonBinary ? GAP.DAEMON_BINARY_ABSENT : helper),
    keeper: platformGap ?? (!binaries.keeperBinary ? GAP.KEEPER_BINARY_ABSENT : helper),
  };
}

/**
 * One case, in its own root: a case that kills a daemon, leaves a decision record behind, or wedges a
 * relay must not be able to influence the next one. Isolation is re-asserted per case rather than
 * trusted from the run, because the paths differ.
 */
async function runCase(entry, index, options, root, evidence, binaries, prerequisites) {
  const startedAt = new Date().toISOString();
  const missing = entry.requires
    .map((name) => ({ name, reason: prerequisites[name] }))
    .filter((item) => item.reason);
  if (missing.length > 0) {
    return {
      id: entry.id,
      title: entry.title,
      outcome: 'blocked',
      gap: missing[0].reason,
      missing,
      durationMs: 0,
      startedAt,
    };
  }

  const paths = planPaths(join(root, 'cases', String(index)));
  // Nothing is created for a case before its paths pass: this is the last point where a mistake can
  // still be refused instead of materialised.
  assertIsolation(paths);
  // All frames land in one run-level stream, so a reader sees the cases interleaved in time order.
  paths.evidenceDir = root;
  for (const dir of [
    paths.runtimeDir,
    paths.legacyRuntimeDir,
    paths.dataDir,
    paths.sessionDir,
    paths.keeperDir,
    paths.relayDir,
    paths.appDir,
    paths.homeDir,
  ]) {
    await mkdir(dir, { recursive: true, mode: 0o700 });
  }
  await chmod(paths.relayDir, 0o700);

  // The daemon runs from a staged bundle, not from the `cargo` output. `binaries.daemonBinary` keeps
  // naming the build the operator supplied; every spawn below uses the staged executable.
  const caseBinaries = { ...binaries, daemonBinary: (await stageCaseDaemon(paths, binaries)) ?? binaries.daemonBinary };

  const ctx = new Context(options, paths, evidence, caseBinaries);
  const started = Date.now();
  let outcome;
  try {
    if (entry.requires.includes('keeper')) await ctx.startKeeper();
    // The daemon starts last: one that started first would find no keeper socket and the case would
    // measure a wiring failure it did not mean to test.
    if (entry.requires.includes('daemon')) ctx.daemon = await ctx.startDaemon({ role: 'initial' });
    const result = await entry.run(ctx);
    outcome = { id: entry.id, title: entry.title, outcome: 'pass', result: result ?? null };
  } catch (error) {
    if (error instanceof AcceptanceError) {
      outcome = {
        id: entry.id,
        title: entry.title,
        outcome: error.kind === 'GAP' ? 'blocked' : 'fail',
        gap: error.kind === 'GAP' ? error.code : undefined,
        failure: error.kind === 'FAIL' ? error.code : undefined,
        detail: error.detail,
        extra: error.extra,
      };
    } else {
      // An unexpected throw is a harness defect, not a product verdict, so it is labelled as one.
      outcome = {
        id: entry.id,
        title: entry.title,
        outcome: 'fail',
        failure: 'HARNESS_ERROR',
        detail: String(error?.stack ?? error),
      };
    }
    // The processes are still alive here, so their streams are read before teardown reaps them: a
    // failing case is exactly the one whose candidate stderr explains why, and a case that recorded
    // only its own verdict would have thrown that away.
    outcome.processStreams = await captureProcessStreams(
      ctx,
      `failed-${entry.id}`,
      // Keyed by position, not by `kind`: two daemons can share a kind, and a collapsed key would
      // silently drop the one stream that mattered.
      Object.fromEntries(ctx.children.map((child, position) => [`${position}-${child.kind}`, child])),
    ).catch(() => null);
  } finally {
    // The evidence is written before the teardown, so a teardown failure cannot erase the record of
    // what happened.
    outcome.durationMs = Date.now() - started;
    outcome.startedAt = startedAt;
    outcome.root = paths.root;
    await evidence.writeCase(entry.id, outcome).catch(() => null);
    await ctx.reapKeeperSessions().catch(() => null);
    await ctx.shutdown().catch(() => null);
    // A killed process leaves socket files behind by design, so the case root is removed instead of
    // accumulating them for the rest of the run.
    await rm(paths.root, { recursive: true, force: true }).catch(() => null);
  }
  return outcome;
}

function summarize(outcomes, gate) {
  const count = (name) => outcomes.filter((row) => row.outcome === name).length;
  // A selection that is not exactly the gate is never accepted, even when every case in it passed.
  const everySelectedPassed = outcomes.length > 0 && count('pass') === outcomes.length;
  return {
    total: outcomes.length,
    passed: count('pass'),
    failed: count('fail'),
    blocked: count('blocked'),
    accepted: everySelectedPassed && gate.complete,
    everySelectedPassed,
    gate,
  };
}

function render(outcomes, summary, binaries) {
  const lines = [];
  for (const row of outcomes) {
    const label = row.outcome === 'pass' ? 'PASS   ' : row.outcome === 'fail' ? 'FAIL   ' : 'BLOCKED';
    lines.push(`${label} ${row.id}`);
    if (row.outcome !== 'pass') {
      lines.push(`        ${row.failure ?? row.gap ?? 'unknown'}`);
      if (row.detail) lines.push(`        ${row.detail}`);
    } else if (row.result?.note) {
      lines.push(`        ${row.result.note}`);
    }
    if (row.durationMs) lines.push(`        ${row.durationMs}ms`);
  }
  lines.push('');
  lines.push(`${summary.passed} passed, ${summary.failed} failed, ${summary.blocked} blocked of ${summary.total} cases`);
  // "Accepted" is a claim about the handover failure-injection gate, and that gate is only meaningful
  // if a real daemon actually ran. Saying it for a self-check-only selection would be a false pass.
  const exercised = outcomes.some((row) => row.outcome === 'pass' && row.id !== 'isolation-guard');
  if (summary.gate?.missing?.length > 0) {
    lines.push(`NOT ACCEPTED: the selection omits ${summary.gate.missing.join(', ')}`);
  } else if (summary.gate?.extra?.length > 0) {
    // Not a defect in the cases: the gate plans exactly five ids and rejects any other count, so an
    // extra case makes the outcome a sweep rather than a gate result.
    lines.push(`NOT ACCEPTED: the selection adds ${summary.gate.extra.join(', ')}, which the gate does not run`);
  } else if (!summary.accepted) {
    lines.push('NOT ACCEPTED: the Phase 4 gate does not hold');
  } else if (exercised) {
    lines.push('ACCEPTED: every gate-required case passed against the supplied binaries');
  } else {
    lines.push('SELF-CHECK ONLY: the harness contract holds, but no handover case ran');
  }
  lines.push(`binaries: daemon ${binaries.daemonBinary ?? 'missing'}, keeper ${binaries.keeperBinary ?? 'missing'}`);
  return lines.join('\n');
}

/**
 * Plan rev 5 lines 248-263 -> the cases that cover each clause, and what each case actually proves.
 *
 * The `asserts` lists are the contract this run reports, so a clause cannot look covered by a case
 * that only names it. `typedGap` is a limitation the run reports rather than a pass.
 */
function coverage() {
  return {
    'a-successor-dies-before-decision': {
      cases: ['a-successor-dies-before-decision'],
      asserts: [
        'the killed candidate exits non-zero without becoming an owner',
        'no decision record and no staging file exist after the death',
        'the abort decision (AbortHandover) settles Active with rollbackReason "predecessor abort decision"',
        'the abort record names the fixture session exactly once and declares successorPid unused by construction',
        'the daemon log gains exactly one handover.decision_accounting_unpopulated record, for field successorPid, committed=false',
        'the predecessor still serves the session and keeps the PTY child alive after the abort',
        'a retried handover commits to a fresh candidate: exit 0, decision Retired, no rollbackReason',
        'the commit record names the fixture session and declares successorPid unused, with one matching warning committed=true',
        'the aborted record is unchanged by the later commit',
        'the successor serves the same PTY child pid and start time through the canonical socket',
      ],
    },
    'b-successor-dies-after-decision': {
      cases: ['b-successor-dies-after-decision'],
      asserts: [
        'the commit decision is written before the successor is killed (no injection: the kill follows the durable record)',
        'the decision settles Retired with no rollbackReason',
        'the commit record names the fixture session and declares successorPid unused, with one matching warning committed=true',
        'the successor really owns the session before the crash: same child pid and start time',
        'the PTY child is gone once the retired predecessor and the dead successor both no longer hold it',
        'the canonical socket refuses connections, and a fresh daemon in the same runtime adopts nothing',
        'a fresh daemon refuses both attach and write for the dead session',
      ],
    },
    'c-predecessor-dies-before-decision': {
      cases: [
        'c-predecessor-dies-before-decision-direct',
        'c-keeper-owned-session-survives-predecessor-death',
        'c-keeper-session-adopted-by-daemon',
      ],
      asserts: [
        'direct half: the predecessor really serves the session before the kill (attach, then a marker only a live child can print)',
        'direct half: the candidate exit is non-zero, and no decision record or staging file exists (the scan is proved non-vacuous by naming the prepared legacy socket it must list)',
        'direct half: the canonical socket refuses; the direct-owned PTY child is gone',
        'direct half: a fresh daemon adopts nothing, and refuses attach and write for the dead session',
        'keeper half: the direct-owned PTY child is gone while the keeper-owned child is still alive',
        'keeper half: a fresh daemon does not list the dead direct session and refuses attach and write for it (whether it also lists the keeper-owned session is recorded, not asserted: that verdict belongs to c-keeper-session-adopted-by-daemon)',
        'keeper half: the keeper still lists the session once, with the pid and OS start time it spawned',
        'keeper half: a post-death attach replays the written marker and advances the head past the pre-death head',
        'adoption half: a daemon whose binary carries a bundle keeper payload routes the fixture to the keeper, and a fresh daemon after the predecessor death lists exactly that session with the keeper-spawned pid and serves it',
      ],
      typedGap:
        'c-keeper-session-adopted-by-daemon: keeper ownership only activates after keeper_startup::ensure_keeper derives a payload from current_exe() via bundle_keeper_dir(), which needs Contents/MacOS/<exe> and Contents/Resources/keeper. A cargo target binary has neither, so this case reports the computed bundle answer and blocks instead of passing; the release gate supplies the shipped bundle daemon and reaches it',
    },
    'harness-contract': {
      cases: ['isolation-guard'],
      asserts: [
        'every path the run touches is inside the isolated root',
        'each named isolation guard fires for its own mutation, in the pinned order',
        'the gate-required case list, the case table and this contract agree (checked before any case runs)',
      ],
    },
  };
}

/** A usage failure reads as its reason; the typed code is redundant noise in front of `USAGE`. */
const usageText = (error) => error?.detail ?? String(error?.message ?? error);

export async function main(argv = process.argv.slice(2)) {
  let options;
  try {
    options = parseArgs([...argv]);
  } catch (error) {
    process.stderr.write(`${usageText(error)}\n\n${USAGE}\n`);
    return 2;
  }
  if (options.help) {
    process.stdout.write(`${USAGE}\n`);
    return 0;
  }
  if (options.list) {
    for (const entry of CASES) {
      const gate = GATE_REQUIRED_CASES.includes(entry.id) ? 'gate' : 'aux';
      process.stdout.write(
        `${entry.id} [${gate}]\n    ${entry.title}\n    requires: ${entry.requires.join(', ') || '(nothing)'}\n`,
      );
    }
    return 0;
  }

  let selected;
  let binaries;
  let root;
  let ownsRoot;
  let gate;
  let contract;
  try {
    // The contract is checked before a selection is judged by it, so drift is reported as drift
    // rather than as a missing case.
    contract = auditCoverage();
    selected = selectCases(options);
    gate = auditSelection(selected);
    if (!gate.complete && !options.allowPartial) {
      // Two different mistakes, named separately: a case the gate needs and this run would not
      // execute, and a case it does not require that would make the outcome count wrong.
      const problems = [];
      if (gate.missing.length > 0) problems.push(`this selection omits ${gate.missing.join(', ')}`);
      if (gate.extra.length > 0) {
        problems.push(`this selection adds ${gate.extra.join(', ')}, which the gate does not run`);
      }
      throw fail(
        FAIL.PARTIAL_SELECTION,
        `${problems.join(' and ')}; pass exactly the ${GATE_REQUIRED_CASES.length} gate cases, or --allow-partial for a debugging run that can never be accepted`,
      );
    }
    binaries = await resolveBinaries(options);
    // The root is short on purpose: `/tmp/fx4-XXXXXX` leaves room for `handover-<uuid>.sock` inside
    // the 103-byte `sun_path` budget, which `mkdtemp(tmpdir())` does not.
    ownsRoot = !options.root;
    root = options.root ? resolve(options.root) : await mkdtemp('/tmp/fx4-');
  } catch (error) {
    process.stderr.write(`${usageText(error)}\n\n${USAGE}\n`);
    return 2;
  }

  const paths = planPaths(root);
  // Isolation is asserted before a single process is spawned, from the paths that will be used.
  let isolation;
  try {
    isolation = assertIsolation(paths);
  } catch (error) {
    process.stderr.write(`${usageText(error)}\n\n${USAGE}\n`);
    return 2;
  }
  await mkdir(root, { recursive: true, mode: 0o700 });

  const evidence = new Evidence(root);
  await evidence.init();
  await evidence.record({
    run: 'phase4-handover-failure-injection',
    step: 'started',
    argv: [...argv],
    platform: process.platform,
    node: process.version,
    root,
    paths,
    isolation,
    launchd: plannedLaunchdIdentity(paths),
    contract,
    gate,
    coverage: coverage(),
  });

  const prerequisites = await buildPrerequisites(binaries);
  await evidence.record({ step: 'preflight', binaries, prerequisites });

  const outcomes = [];
  for (const [index, entry] of selected.entries()) {
    const outcome = await runCase(entry, index, options, root, evidence, binaries, prerequisites);
    outcomes.push(outcome);
    // Streamed as it happens: a case that wedges should be visible before the run finishes.
    if (!options.json) {
      const label = outcome.outcome === 'pass' ? 'PASS   ' : outcome.outcome === 'fail' ? 'FAIL   ' : 'BLOCKED';
      process.stdout.write(`${label} ${outcome.id}\n`);
    }
  }

  const summary = summarize(outcomes, gate);
  const report = {
    run: 'phase4-handover-failure-injection',
    root,
    isolation,
    binaries,
    prerequisites,
    contract,
    gate,
    coverage: coverage(),
    summary,
    outcomes,
  };
  await evidence.writeCase('summary', report);
  await evidence.record({ step: 'finished', summary });

  if (options.json) {
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  } else {
    process.stdout.write(`\n${render(outcomes, summary, binaries)}\n`);
    process.stdout.write(`\nevidence: ${root}\n`);
  }

  // Only a root this run created is removed: `--root` names a directory that already belonged to
  // the operator, and deleting it would be a destructive act nobody asked for.
  if (summary.accepted && !options.keep && ownsRoot) {
    await rm(root, { recursive: true, force: true }).catch(() => null);
  }
  return summary.accepted ? 0 : 1;
}

// Importing this file must never start anything: the CLI runs only when it is the entry point.
if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  main()
    .then((code) => {
      process.exitCode = code;
    })
    .catch((error) => {
      process.stderr.write(`${error?.stack ?? error}\n`);
      process.exitCode = 1;
    });
}

