#!/usr/bin/env node
/**
 * Phase 3 acceptance: isolated PTY crash survival.
 *
 * Source of truth: `.omo/plans/local-pty-crash-survival-20261116.md` (rev 5), Phase 3.
 *
 * This harness only drives processes it started itself, under its own runtime root, and only over the
 * real wire protocols:
 *
 *   keeper  `crates/ferryx-ptykeeper` - 60-byte `FXKP` header, JSON payload, base64 bytes, read from
 *           `<FERRYX_RUNTIME_DIR>/keeper.sock` (see `crates/ferryx-ptykeeper/src/protocol.rs`).
 *   daemon  NDJSON on `<FERRYX_RUNTIME_DIR>/daemon.sock`, `Handshake` version 6, then
 *           `spawn` / `write` / `attach` / `describeLiveSessions` (see `src-tauri/src/daemon/protocol.rs`).
 *
 * Nothing is mocked: no fake PTY, no stubbed socket, no synthetic survival claim. When a seam the plan
 * requires does not exist in the tree, the case that needs it reports the typed gap code and the run
 * is NOT ACCEPTED. It never reports a pass on a weaker path.
 *
 * Isolation contract, asserted before anything is spawned:
 *   FERRYX_RUNTIME_DIR         owned temp dir, never `/tmp/ferryx-<uid>`, never under `$HOME/.ferryx`
 *   FERRYX_LEGACY_RUNTIME_DIR  owned temp dir, distinct from and not nested with the runtime dir
 *   FERRYX_DATA_DIR            owned temp dir
 *   FERRYX_SESSION_DIR         owned temp dir
 *   FERRYX_KEEPER_DIR          owned temp dir (keeper generation installs)
 *   launchd label              `com.ferryx.qa.ptykeeper`, never `com.ferryx.ptykeeper`
 *   keeper plist path          inside the runtime root, via FERRYX_PTY_KEEPER_PLIST_PATH
 * Every child gets a scrubbed environment: no inherited `FERRYX_*` except the ones set here.
 *
 * Usage:
 *   node scripts/qa/phase3-pty-crash-survival.acceptance.mjs --list
 *   node scripts/qa/phase3-pty-crash-survival.acceptance.mjs \
 *     --keeper-binary crates/ferryx-ptykeeper/target/release/ferryx-ptykeeper \
 *     --daemon-binary src-tauri/target/release/ferryx \
 *     [--case <id>]... [--sessions 60] [--generations <binary>[,<binary>]] [--keep] [--json]
 *
 * Exit codes: 0 every selected case passed, 1 at least one case failed or was blocked, 2 usage error,
 * 3 the run could not tear down every process or socket it recorded. Code 3 exists because the
 * harness once printed its verdict and then hung for hours on a live handle: a run that cannot prove
 * it released what it spawned is a harness fault, and reporting it as a case result would hide that.
 */

import { spawn } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { once } from 'node:events';
import { appendFile, chmod, copyFile, mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises';
import { homedir, tmpdir, userInfo } from 'node:os';
import net from 'node:net';
import { basename, dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { readSourceFacts, SourceFactError } from './lib/phase3-source-facts.mjs';
// The bundle layout, the keeper constants and the staging call all come from the module the release
// packaging uses, so a harness and a shipped bundle cannot disagree about where the payload lives.
import {
  DAEMON_BINARY_NAME,
  KEEPER_BINARY_NAME,
  isBundledDaemon,
  keeperPayloadDirFor,
  stageAppBundle,
} from '../lib/keeper-bundle.mjs';

/** The checkout this file lives in, so the default binary locations are this tree's targets. */
const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

// ---------------------------------------------------------------------------------------------
// Constants pinned to real interfaces. Each names the Rust item it mirrors; drift is a harness bug.
// ---------------------------------------------------------------------------------------------

/** `protocol.rs`: MAGIC. */
const KEEPER_MAGIC = Buffer.from('FXKP', 'ascii');
/** `protocol.rs`: HEADER_BYTES. */
const KEEPER_HEADER_BYTES = 60;
/** `protocol.rs`: FRAMING_VERSION. */
const KEEPER_FRAMING_VERSION = 1;
/** `protocol.rs`: MAX_FRAME_PAYLOAD_BYTES. */
const KEEPER_MAX_PAYLOAD_BYTES = 1024 * 1024;
/** `protocol.rs`: MAX_CONNECTIONS, the acceptor's concurrent-peer cap. */
const KEEPER_MAX_CONNECTIONS = 8;
/** `protocol.rs`: NO_REQUEST_ID. */
const KEEPER_NO_REQUEST_ID = '-'.repeat(36);
/** `crash_supervisor.rs`: KEEPER_SOCKET_NAME. */
const KEEPER_SOCKET_NAME = 'keeper.sock';

// The payload layout `keeper_startup::bundle_keeper_dir` derives from `current_exe()`: it takes the
// executable's parent, requires that directory to be named `MacOS`, and looks for
// `../Resources/keeper/<BINARY_NAME>`. There is no environment override, so a daemon that is not
// laid out this way gets `PayloadAbsent` and `start_production` aborts before it can install the
// keeper runtime.
//
// The names are imported from `scripts/lib/keeper-bundle.mjs` instead of restated here: the release
// packaging stages the same layout, and a second copy of it drifts at exactly the point where the
// harness believes it staged a bundle the daemon can read and the daemon disagrees.

// The echo child's readiness handshake: the child names this token in its own argv so the spawn
// path can tell an echo fixture from any other child, and writes the tty mode it set to the file
// `FERRYX_QA_TTY_MARKER` names. The directory lives inside the isolated root (see `planPaths`), so a
// misdirected write cannot escape the fixture.
const RAW_TTY_SENTINEL = 'FERRYX_QA_ECHO_CHILD';
const RAW_TTY_MARKER_ENV = 'FERRYX_QA_TTY_MARKER';
/** Basename prefix of one marker per session, so a case running several children cannot be misled. */
const RAW_TTY_MARKER = 'tty-mode';
/** Set from `planPaths` before any case runs; `KeeperClient.spawn` needs it to place the marker. */
let rawTtyDir = null;
/** `session_service.rs::handle_spawn_requested`: the only shape a `requestedSessionId` may take. */
const CANONICAL_UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
/** `endpoint.rs`: the macOS `sun_path` budget, NUL excluded. */
const MAX_SOCKET_PATH_BYTES = 103;
/** `replay.rs`: REPLAY_SESSION_BYTES. */
const REPLAY_SESSION_BYTES = 8 * 1024 * 1024;
/** `budget.rs`: BUDGET_AGGREGATE_BYTES. */
const BUDGET_AGGREGATE_BYTES = 768 * 1024 * 1024;
/**
 * `budget.rs`: REPLAY_AGGREGATE_BYTES, the sub-cap that binds first.
 *
 * A session's whole replay allocation is charged at spawn, so this cap is also a session ceiling
 * (`cap / SESSION_RESERVATION_BYTES`). It is mirrored here because the fixture guards below have to
 * know which cap refuses first: a run sized against the 768 MiB aggregate alone would be counting on
 * room the replay sub-cap does not have, which is exactly how the 71-pane soak was refused at 65.
 */
const REPLAY_AGGREGATE_BYTES = 640 * 1024 * 1024;
/** `keeper.rs`: the per-session reservation (60 sessions = 480 MiB of 768 MiB). */
const SESSION_RESERVATION_BYTES = 8 * 1024 * 1024;
/** `keeper.rs`: MAX_WRITE_BYTES. */
const KEEPER_MAX_WRITE_BYTES = 64 * 1024;
/**
 * `protocol.rs`: ATTACH_INLINE_MAX_BYTES.
 *
 * Pinned for drift only, never asserted as reachable: `keeper.rs::replay_page` copies at most
 * `REPLY_PAGE_BYTES` (512 KiB), which equals this cap, so an `ATTACH` can never produce the
 * `FRAME_TOO_LARGE` refusal `server.rs` defines behind it. A case that expected that refusal would
 * be asserting a branch this build cannot reach.
 */
const KEEPER_ATTACH_INLINE_MAX_BYTES = 512 * 1024;
/** `checkpoint.rs`: MAX_CHECKPOINT_BYTES, the per-checkpoint cap charged before allocation. */
const KEEPER_MAX_CHECKPOINT_BYTES = 32 * 1024 * 1024;
/** `endpoint.rs`: DIR_MODE / SOCKET_MODE, the modes of the endpoint the keeper owns. */
const KEEPER_DIR_MODE = 0o700;
const KEEPER_SOCKET_MODE = 0o600;
/** `src-tauri/src/daemon/protocol.rs`: DAEMON_PROTOCOL_VERSION with default features. */
const DAEMON_PROTOCOL_VERSION = 6;
/** Plan Phase 3: "history beyond 512 KiB is present". */
const NO_GAP_HISTORY_BYTES = 512 * 1024;
/** `keeper_launchd.rs`: QA_PLIST_LABEL. */
const QA_KEEPER_LABEL = 'com.ferryx.qa.ptykeeper';
/** The label this harness must never write, load, or unload. */
const PRODUCTION_KEEPER_LABEL = 'com.ferryx.ptykeeper';
/**
 * `keeper_startup.rs`: `KEEPER_OWNERSHIP_POLICY_ENV`, the daemon-start routing override. It is a
 * policy for *new* sessions only -- the same startup pass attaches every keeper-held session before it
 * applies the flag -- so starting a daemon with it can never strand a surviving shell.
 */
const KEEPER_OWNERSHIP_ENV = 'FERRYX_KEEPER_OWNERSHIP';
/**
 * `keeper_startup.rs::OwnershipPolicy::parse`, exactly: every spelling the daemon accepts, lowercased
 * the way it lowercases. Absent or empty means `unchanged`; any other value is a typo the daemon logs
 * and then ignores, which is why `startDaemon` refuses one rather than forwarding it -- a case that
 * asked for a disable would otherwise run with routing it never had.
 */
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

/** `protocol.rs`: Kind discriminants. Never renumbered. */
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
  CheckpointInstall: 0x0080,
  CheckpointChunk: 0x0081,
  CheckpointCommit: 0x0082,
  CheckpointAck: 0x0083,
  CheckpointQuery: 0x0084,
  CheckpointFetch: 0x0085,
  CheckpointFetchChunk: 0x0086,
  CheckpointFetchEnd: 0x0087,
  Detach: 0x0090,
  Error: 0x00f0,
  Shutdown: 0x00f1,
});
const KIND_NAME = new Map(Object.entries(KIND).map(([name, value]) => [value, name]));

/**
 * Typed gaps: a seam the plan needs that the tree does not have. A gap is never a pass - the run
 * counts it as not accepted and names it in the summary.
 */
const GAP = Object.freeze({
  KEEPER_BINARY_ABSENT: 'no keeper binary was found, so survival cannot be exercised at all',
  DAEMON_BINARY_ABSENT: 'no daemon binary was found, so daemon-level reattach cannot be exercised',
  KEEPER_START_FAILED: 'the keeper binary exists but never bound its socket, so no case could run',
  RUNTIME_DIR_UNISOLATED: 'FERRYX_RUNTIME_DIR is absent or names a production location',
  LEGACY_RUNTIME_DIR_UNISOLATED: 'FERRYX_LEGACY_RUNTIME_DIR is absent or names a production location',
  RUNTIME_LEGACY_OVERLAP: 'the runtime and legacy roots overlap, so isolation is not provable',
  KEEPER_SOCKET_PATH_TOO_LONG: 'the isolated runtime path exceeds the unix socket path limit',
  PRODUCTION_LABEL_REFUSED: 'a production launchd label or plist path would have been touched',
  KEEPER_INTEGRATION_ABSENT:
    'where keeper startup can run, the daemon spawned a session and the keeper registry still has no entry for it, so no spawn reached the keeper',
  KEEPER_ROUTING_PLATFORM_GATED:
    'keeper ownership cannot be enabled on this host: `daemon/mod.rs` declares `keeper_startup` only for macOS and `daemon/server.rs:6710` gates the `start_production` call the same way, so this daemon binary contains no keeper start pass and nothing can flip the new-session routing flag',
  KEEPER_ROUTING_PAYLOAD_ABSENT:
    'the daemon cannot install the keeper runtime from where it runs: `bundle_keeper_dir` requires `<exe>/../MacOS` and looks for `../Resources/keeper`, and answers `PayloadAbsent` otherwise, so `start_production` never installed the runtime',
  OWNERSHIP_FLAG_ABSENT:
    'keeper ownership is not drivable from the daemon: the new-session routing flag, or the startup pass that sets it, is missing',
  ROLLBACK_REFUSAL_ABSENT: 'no rollback verb exists, so the typed refusal carrying the keeper-owned count cannot be observed',
  LIVE_PUSH_ABSENT: 'an attached consumer received no keeper-initiated OUTPUT, so live continuation would need a poll',
  IDENTITY_START_TIME_UNPROVEN: 'no child start time was reported, so identity would be pid-only',
  HANDOVER_KEEPER_UNPROVEN: 'the graceful handover path does not name keeper-owned sessions',
  HANDOVER_STEPS_ABSENT:
    'the tree does not expose every step of a graceful handover (prepare / transfer / commit / --handover-from), so no replacement can be driven',
  KEEPER_GENERATIONS_MISSING: 'fewer than two distinct keeper generations were supplied',
  KEEPER_GENERATIONS_IDENTICAL:
    'the two keeper builds supplied are the same bytes, and a keeper generation identity is the version compiled into it (`Keeper::new(env!("CARGO_PKG_VERSION"))`), so two copies of one build are one generation and coexistence across generations cannot be measured',
  DAEMON_GENERATIONS_MISSING:
    'fewer than two daemon binaries were supplied, so an app replacement has no successor build to run',
  DAEMON_GENERATIONS_INDISTINCT:
    'the supplied daemon binaries speak the same protocol version, so the "old replaced by new" pair is not real',
  CLI_BINARY_ABSENT:
    'no ferryx-cli binary was found, so the rollback refusal cannot be produced by the CLI that ships it',
  PLATFORM_UNSUPPORTED: 'the keeper transport is unix only',
  HELPER_ABSENT: 'an OS helper the case needs (`ps`) is unavailable',
});

const FAIL = Object.freeze({
  USAGE: 'a required argument is missing or malformed',
  DEADLINE: 'a bounded wait expired before its condition held',
  PROTOCOL: 'a wire frame did not match the protocol this harness speaks',
  REFUSED: 'the peer answered with a typed refusal the case did not expect',
  IDENTITY_MISMATCH: 'a process identity changed across the recovery boundary',
  REFUSAL_CODE: 'a refusal carried the wrong typed code, so it refused for a different reason',
  BYTE_ACCOUNTING: 'the recovered byte stream is not the byte stream that was produced',
  DUPLICATE_OUTPUT: 'a marker was delivered more than once',
  MISSING_OUTPUT: 'a marker that must survive the boundary is absent',
  GAP_IN_NO_GAP_CASE: 'a replay gap was reported where the downtime fits the replay cap',
  NO_GAP_IN_OVERFLOW_CASE: 'no replay gap was reported after the replay cap overflowed',
  EPOCH_NOT_FENCED: 'a stale epoch was still allowed to read or write',
  OVERSIZED_FIXTURE: 'the fixture exceeds the caps this case is written against',
  LIMIT_NOT_TYPED: 'a documented cap (connections, checkpoint size) did not refuse with its typed code',
  ATOMICITY_VIOLATED: 'a partial upload, or a superseded value, was visible where only the committed one may be',
  SAFETY: 'an operation would have left the isolation contract',
  SOURCE_DRIFT:
    'a literal in this harness no longer matches the tree it drives, so every verdict below would be about the wrong protocol',
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

function fail(code, detail, extra) {
  return new AcceptanceError('FAIL', code, detail, extra);
}

function gap(code, detail) {
  return new AcceptanceError('GAP', code, detail ?? GAP[code]);
}

// ---------------------------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------------------------

const sha256 = (buffer) => createHash('sha256').update(buffer).digest('hex');
const b64 = (buffer) => Buffer.from(buffer).toString('base64');
const unb64 = (text) => Buffer.from(typeof text === 'string' ? text : '', 'base64');

/** Bounded poll. No fixed sleep decides an outcome; only a deadline does. */
async function until(label, timeoutMs, probe, intervalMs) {
  const deadline = Date.now() + timeoutMs;
  const step = intervalMs ?? 25;
  for (;;) {
    const value = await probe();
    if (value) return value;
    if (Date.now() >= deadline) throw fail(FAIL.DEADLINE, `${label}: condition never held within ${timeoutMs}ms`);
    await new Promise((tick) => setTimeout(tick, step));
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
 * Every socket this run opened, so exit is decided by what is actually still live.
 *
 * A connected socket that was never destroyed holds a libuv handle, and one such handle keeps the
 * process alive after `main()` resolves. That is what a "printed the report, then stayed up for
 * hours" hang is: the verdict is written, the event loop still has work, and the process it spawned
 * (`ferryx --daemon`) is never released. Tracking them makes the leak a named fact in evidence
 * rather than a guess about which path forgot a `close()`.
 */
const liveSockets = new Map();
// Every child process this run started, keyed by its `Supervised` wrapper. Registered in the
// constructor, which is the one place every spawn passes through -- daemons, keepers, generations,
// staged CLI runs alike -- so a new case cannot forget to register. `Context` keeps named slots for
// the processes a case reasons about, but those slots are reassigned when a successor starts; this
// map is the record of what is actually still running.
const liveChildren = new Map();

/** Registers a socket by identity, so a run cannot finish with one still open. */
function trackSocket(socket, label) {
  liveSockets.set(socket, label);
  // `close` is the event that actually means the handle is gone. `error` can arrive while the kernel
  // still has the socket open, so treating it as the end would hide a live handle instead of finding
  // it -- and an `error`-then-`close` socket is exactly the case that used to leak.
  socket.once('close', () => liveSockets.delete(socket));
  return socket;
}

/** The sockets still open, by label. Empty is the only state from which exit is honest. */
function openSockets() {
  return [...liveSockets.values()];
}

/**
 * The end-of-run backstop: stop every process this run started and close every socket it opened.
 *
 * Called from a `finally`, so it runs on the pass path, on the fail path, on a `gap`/`blocked` verdict
 * and on an unexpected throw. It stops only what the registry recorded, which is the harness's own
 * children: nothing here reaches for a pid it did not spawn.
 *
 * Newest first, because a replacement daemon can hold a listener its predecessor also held, and a
 * keeper generation can hold a PTY master the older one still names.
 */
async function teardownRun(evidence) {
  const children = [...liveChildren.keys()].reverse();
  const sockets = [...liveSockets.keys()];
  // `close` is subscribed BEFORE `destroy()`, because the event can fire before a listener attached
  // afterwards, and that listener would then wait out its whole timeout for an event already gone.
  const closed = sockets.map((socket) => once(socket, 'close').catch(() => null));
  const stopped = [];
  for (const supervised of children) {
    try {
      await supervised.stop();
      stopped.push({ kind: supervised.kind, pid: supervised.pid ?? null });
    } catch (error) {
      stopped.push({ kind: supervised.kind, pid: supervised.pid ?? null, error: String(error?.message ?? error) });
    }
  }
  // Children first, then sockets: that is the order of a real shutdown, where the peer going away
  // gives the socket EOF. Sockets still open at this point belong to peers that did not close, so they
  // are destroyed rather than politely ended.
  for (const socket of sockets) {
    socket.destroy();
  }
  await Promise.all(closed.map((done) => bounded(done, 'socket close after destroy', 2_000).catch(() => null)));
  const leakedChildren = [...liveChildren.entries()].map(([supervised, kind]) => ({ kind, pid: supervised.pid ?? null }));
  const leakedSockets = openSockets();
  // Recorded on every run, including a run that started nothing. This frame is the evidence that the
  // harness accounted for every process it started, so a clean exit is a checked fact rather than an
  // assumption about which code path ran.
  await evidence?.record({ step: 'teardown', stopped, leaked: leakedChildren, openSockets: leakedSockets });
  return { stopped, leakedChildren, leakedSockets };
}

/**
 * What `finishRun` may still touch after `main()` returns: the evidence sink and the fixture root.
 *
 * The root is deleted only after teardown, not before. `main()` used to remove it on an accepted run
 * before returning, which would destroy the evidence directory of a run that then turned out to have
 * leaked -- the one time that record matters most.
 */
let runEvidence = null;
let runRoot = null;
/** `--keep` preserves the fixture root on a clean run; a leak preserves it regardless. */
let runKeepRoot = false;

/**
 * The end-of-run gate: tear down, then decide the exit code from what is actually left.
 *
 * Every path through `main()` reaches here -- a verdict, a thrown harness error, a usage refusal --
 * because it is called from both the fulfilment and the rejection of the `main()` promise. The exit
 * code it returns is a statement about the machine, not about the report: a case result never
 * upgrades a run that left a process or a socket behind.
 */
async function finishRun(code) {
  const { leakedChildren, leakedSockets } = await teardownRun(runEvidence).catch((error) => ({
    leakedChildren: [{ kind: 'teardown', pid: null, error: String(error?.message ?? error) }],
    leakedSockets: [],
  }));
  const leaked = leakedChildren.length > 0 || leakedSockets.length > 0;
  // Deleted only after teardown, only on an accepted run, and never when something leaked: the
  // evidence directory is the record of what survived, and a failing run's root is its evidence.
  if (runRoot && !runKeepRoot && !leaked && code === 0) {
    await rm(runRoot, { recursive: true, force: true }).catch(() => null);
  }
  if (!leaked) return code;
  // Loud and unmissable: this is the defect the run is supposed to make impossible, so it must never
  // be reported as a tidy verdict.
  process.stderr.write(
    `TEARDOWN LEAK: ${leakedChildren.length} process(es) and ${leakedSockets.length} socket(s) survived teardown: ` +
      `${JSON.stringify({ children: leakedChildren, sockets: leakedSockets })}\n`,
  );
  await runEvidence?.record({ step: 'teardown-leak', children: leakedChildren, sockets: leakedSockets }).catch(() => null);
  return 3;
}
/** True when `candidate` is strictly inside `root`. */
function isUnder(root, candidate) {
  if (!isAbsolute(root) || !isAbsolute(candidate)) return false;
  const delta = relative(root, candidate);
  return delta !== '' && delta !== '..' && !delta.startsWith(`..${'/'}`) && !isAbsolute(delta);
}

function overlaps(a, b) {
  return a === b || isUnder(a, b) || isUnder(b, a);
}

// ---------------------------------------------------------------------------------------------
// Isolation
// ---------------------------------------------------------------------------------------------

/**
 * Evaluated on resolved paths only, and not relaxable by any flag: an unisolated run would put the
 * user's real keeper or daemon within reach, which is the single thing this harness must never do.
 */
function assertIsolation(paths) {
  const uid = typeof process.getuid === 'function' ? process.getuid() : userInfo().uid;
  const home = homedir();
  const production = [`/tmp/ferryx-${uid}`, `/private/tmp/ferryx-${uid}`, join(home, '.ferryx')];

  const requireOwned = (label, dir, code) => {
    if (!dir || !isAbsolute(dir)) {
      throw fail(FAIL.SAFETY, `${label} must be an absolute path, got ${JSON.stringify(dir)}`, { code });
    }
    if (!isUnder(paths.root, dir)) {
      throw fail(FAIL.SAFETY, `${label} ${dir} is not under the owned root ${paths.root}`, { code });
    }
    for (const banned of production) {
      if (dir === banned || isUnder(banned, dir)) {
        throw fail(FAIL.SAFETY, `${label} ${dir} is a production location`, { code });
      }
    }
  };

  requireOwned('FERRYX_RUNTIME_DIR', paths.runtimeDir, GAP.RUNTIME_DIR_UNISOLATED);
  requireOwned('FERRYX_LEGACY_RUNTIME_DIR', paths.legacyRuntimeDir, GAP.LEGACY_RUNTIME_DIR_UNISOLATED);
  requireOwned('FERRYX_DATA_DIR', paths.dataDir, GAP.RUNTIME_DIR_UNISOLATED);
  requireOwned('FERRYX_SESSION_DIR', paths.sessionDir, GAP.RUNTIME_DIR_UNISOLATED);
  requireOwned('FERRYX_KEEPER_DIR', paths.keeperDir, GAP.RUNTIME_DIR_UNISOLATED);
  // Not an environment variable any child reads: the marker path reaches an echo child through its own
  // spawn envelope. Asserted here because a misdirected marker would be a write outside the fixture.
  requireOwned('tty marker dir', paths.rawTtyDir, GAP.RUNTIME_DIR_UNISOLATED);
  // Once the keeper install pass runs for real, `write_keeper_launchd_plist` creates
  // `$HOME/Library/Logs/Ferryx`; the daemon also reads `HOME` for nothing else this fixture wants.
  requireOwned('HOME', paths.homeDir, GAP.RUNTIME_DIR_UNISOLATED);
  // Staged bundles are copies of the binaries under test. A bundle outside the root would run a
  // daemon whose keeper generation directory is outside the fixture too.
  requireOwned('staged bundle dir', paths.bundleDir, GAP.RUNTIME_DIR_UNISOLATED);

  // Mirrors `server::transport_qa_isolated_profile`: distinct, non-nested, absolute.
  if (overlaps(paths.runtimeDir, paths.legacyRuntimeDir)) {
    throw fail(
      FAIL.SAFETY,
      `${paths.runtimeDir} and ${paths.legacyRuntimeDir} overlap`,
      { code: GAP.RUNTIME_LEGACY_OVERLAP },
    );
  }

  const socketBytes = Buffer.byteLength(paths.keeperSocketPath);
  if (socketBytes > MAX_SOCKET_PATH_BYTES) {
    throw fail(
      FAIL.SAFETY,
      `${paths.keeperSocketPath} is ${socketBytes} bytes, over the ${MAX_SOCKET_PATH_BYTES} byte unix socket limit`,
      { code: GAP.KEEPER_SOCKET_PATH_TOO_LONG },
    );
  }
  if (paths.keeperPlistPath && basename(paths.keeperPlistPath) !== `${QA_KEEPER_LABEL}.plist`) {
    throw fail(
      FAIL.SAFETY,
      `keeper plist ${paths.keeperPlistPath} does not carry the QA label`,
      { code: GAP.PRODUCTION_LABEL_REFUSED },
    );
  }
  if (paths.keeperPlistPath && paths.keeperPlistPath.includes(PRODUCTION_KEEPER_LABEL)) {
    throw fail(
      FAIL.SAFETY,
      `keeper plist ${paths.keeperPlistPath} names the production label`,
      { code: GAP.PRODUCTION_LABEL_REFUSED },
    );
  }
  return { label: QA_KEEPER_LABEL, plist: paths.keeperPlistPath ?? null };
}

/**
 * `keeper_launchd::label_for`: an isolated or debug runtime gets the QA label. A run of this harness
 * is isolated by construction, so any other label would be a defect. Recorded, never applied: this
 * harness never invokes `launchctl`.
 */
function plannedLaunchdIdentity(paths) {
  return {
    label: QA_KEEPER_LABEL,
    plistPath: paths.keeperPlistPath,
    applied: false,
    note: 'label and plist path are asserted only; launchctl is never invoked by this harness',
  };
}

/** Every child sees exactly the environment this harness builds, never an inherited production one. */
/**
 * The durable ownership table the daemon and the CLI both read, or `null` when it does not exist yet.
 *
 * `OwnershipStore::for_runtime()` resolves to `<FERRYX_DATA_DIR>/pty-ownership.json`. It is read here
 * only as *evidence*: no case fabricates a table, because a table the harness authored would prove
 * nothing about whether the daemon routed a spawn to the keeper. The only write the harness makes to
 * this path is the per-case reset that deletes what a previous case left (`resetOwnershipTable`),
 * which removes state rather than asserting it.
 */
async function readOwnershipTable(dataDir) {
  try {
    return JSON.parse(await readFile(join(dataDir, 'pty-ownership.json'), 'utf8'));
  } catch (error) {
    if (error?.code === 'ENOENT') return null;
    throw fail(FAIL.PROTOCOL, `the ownership table in ${dataDir} is unreadable as JSON: ${String(error?.message ?? error)}`);
  }
}

/**
 * Restores the durable ownership state a case may assume it starts from: no table at all.
 *
 * `planPaths(root).dataDir` is one directory per RUN, not per case (`planPaths` -> `dataDir`), and
 * `pty-ownership.json` lives in it, so every durable thing a case writes outlives the case. That is
 * not a property of the product under test, it is a side effect of the fixture layout: a case that
 * ends with routing pinned off (`FERRYX_KEEPER_OWNERSHIP=disabled`, `ownership-disable-attaches-
 * keeper-sessions`) or whose last daemon never flipped the flag leaves `keeperOwnershipEnabled`
 * behind, and the next case's daemon starts under `Unchanged` -- reading a decision the previous
 * case made. A case that needs a keeper-owned session then fails on the ordering of the run, which is
 * exactly what `mixed-generation-replacement-and-crash` reported ("not keeper-owned").
 *
 * Removing the file is the whole reset: `OwnershipStore::load()` treats a missing file as
 * `OwnershipTable::default()`, i.e. no sessions recorded and routing not yet decided -- the state a
 * fresh install has, so a daemon that enables keeper ownership has to do it at its own start pass.
 * Nothing here fabricates a table: every assertion after it still reads what the daemon itself wrote.
 * The `pty-ownership.tmp-<pid>-<ms>` sibling `OwnershipStore::save` stages through is removed too, so
 * a SIGKILLed daemon cannot leave a half-written table for `readdir`-based checks to trip over.
 */
async function resetOwnershipTable(paths) {
  const tablePath = join(paths.dataDir, 'pty-ownership.json');
  // The reset is a write, so it obeys the same isolation contract as every other write: only inside
  // the fixture root, never the user's table. `isUnder` is the same predicate `assertIsolation` uses.
  if (!isUnder(paths.root, tablePath)) {
    throw fail(FAIL.SAFETY, `the ownership table ${tablePath} is outside the fixture root, so it must not be removed`);
  }
  const entries = await readdir(paths.dataDir).catch((error) => {
    if (error?.code === 'ENOENT') return [];
    throw fail(FAIL.SAFETY, `${paths.dataDir} is unreadable (${error?.code ?? error}), so the ownership reset cannot be proven`);
  });
  const removed = entries.filter((name) => name === 'pty-ownership.json' || name.startsWith('pty-ownership.tmp-'));
  for (const name of removed) await rm(join(paths.dataDir, name), { force: true });
  return { tablePath, removed };
}

/**
 * A staged generation directory under this fixture's runtime root, holding a runnable copy of a real
 * daemon build.
 *
 * `plan_rollback` looks for `<runtime_root>/<version>-<8 hex>/` and inside it either a bare `ferryx`
 * or a macOS helper bundle (`runtime_stage::HELPER_BUNDLE_NAME`). It only checks that the executable
 * is a file, so the copy is what a staged generation genuinely is: real bytes, real path shape.
 *
 * The copy exists because `plan_rollback` resolves generations from `daemon_runtime_root()`, which is
 * `<FERRYX_DATA_DIR>/daemon-runtime` on an isolated runtime -- this fixture. Staging into the
 * user's root is exactly what must not happen, and staging here is what makes a *successful* rollback
 * testable without touching the machine.
 */
async function stageGeneration(paths, version, daemonBinary, suffix) {
  const runtimeRoot = join(paths.dataDir, 'daemon-runtime');
  const key = `${version}-${suffix}`;
  const dir = join(runtimeRoot, key);
  await mkdir(dir, { recursive: true });
  const staged = join(dir, process.platform === 'win32' ? 'ferryx.exe' : 'ferryx');
  await copyFile(daemonBinary, staged);
  await chmod(staged, 0o755);
  return { key, dir, staged, runtimeRoot };
}

/**
 * Runs `ferryx-cli service rollback <version> --json` against this fixture and returns its parsed
 * outcome.
 *
 * Every path the CLI can reach is redirected into the fixture root first, because the success path
 * writes: `apply_rollback` rewrites a plist under `launch_agents_dir` and creates `log_dir`. Those
 * come from `PLIST_PATH_ENV` (`FERRYX_DAEMON_PLIST_PATH`) and `HOME` respectively, so both are
 * overridden and asserted to be inside the fixture before the child is spawned. `HOME` is the one that
 * matters most: `launchd_log_dir_for(home)` is `$HOME/Library/Logs/Ferryx`, so leaving it unset for a
 * certain success would create directories in the user's home.
 */
async function runServiceRollback(ctx, targetVersion, extraEnv) {
  const agentsDir = join(ctx.paths.keeperDir, 'agents');
  const fakeHome = join(ctx.paths.root, 'home');
  const cliEnv = ctx.env(
    Object.assign(
      {
        HOME: fakeHome,
        FERRYX_DAEMON_SUPERVISOR_NAME: 'com.ferryx.qa.phase3-daemon',
        FERRYX_DAEMON_PLIST_PATH: agentsDir,
      },
      extraEnv ?? {},
    ),
  );
  // Isolation is asserted, not assumed: a child that could see a production path must never run.
  for (const key of ['HOME', 'FERRYX_DATA_DIR', 'FERRYX_RUNTIME_DIR', 'FERRYX_SESSION_DIR', 'FERRYX_DAEMON_PLIST_PATH']) {
    const value = cliEnv[key];
    if (!value || !resolve(value).startsWith(resolve(ctx.paths.root))) {
      throw fail(FAIL.SAFETY, `${key} is ${JSON.stringify(value)}, which is not inside the fixture root`);
    }
  }

  const argv = [ctx.binaries.cliBinary, 'service', 'rollback', targetVersion, '--json'];
  const child = spawn(argv[0], argv.slice(1), {
    cwd: ctx.paths.repo,
    env: cliEnv,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const supervised = new Supervised('cli-rollback', child, argv, cliEnv, ctx.evidence);
  let exit;
  try {
    exit = await supervised.waitForExit(60_000);
  } finally {
    await supervised.stop().catch(() => null);
  }
  if (supervised.spawnError) {
    throw gap(
      GAP.CLI_BINARY_ABSENT,
      `${GAP.CLI_BINARY_ABSENT}: could not execute ${ctx.binaries.cliBinary} (${supervised.spawnError.code ?? supervised.spawnError.message})`,
    );
  }
  await ctx.evidence.record({ step: 'service-rollback', argv, exitCode: exit.code, stdout: supervised.stdout.slice(-2_048), stderr: supervised.stderr.slice(-2_048) });
  return { argv, exit, supervised, cliEnv, agentsDir, fakeHome };
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

/**
 * Lays `daemonBinary` and `keeperBinary` out as the bundle shape the daemon derives its keeper
 * payload from, and returns the path of the bundled daemon to execute.
 *
 * This is not a mock: the daemon that runs is a byte copy of the real build, and the keeper the
 * daemon installs is a byte copy of the real keeper. What the layout changes is only *where* the
 * daemon looks for the payload -- `bundle_keeper_dir` reads `current_exe()` and has no environment
 * override, so a plain `cargo` binary can never activate keeper ownership no matter how it is
 * configured. The copy preserves the executable bit, which is what `run_keeper_install_pass` checks
 * with `is_file()` before installing the generation.
 *
 * The layout itself comes from `scripts/lib/keeper-bundle.mjs::stageAppBundle`, the same function the
 * release packaging uses, so this fixture cannot describe a bundle shape the shipped app does not
 * have. `keeper.json` is written beside the keeper from the bytes just copied: a presented manifest
 * whose digest disagrees with the binary is refused by `payload_digest`, and an absent one is
 * tolerated, so writing it from the copied bytes is the honest option.
 */
async function prepareKeeperBundle(paths, daemonBinary, keeperBinary) {
  const bundledDaemon = await stageDaemonBundle(paths, daemonBinary, keeperBinary);
  // `stageDaemonBundle` returns the source untouched only when the caller handed in a real bundle,
  // in which case its payload directory is the answer; otherwise it is the one just staged.
  const payloadDir = keeperPayloadDirFor(bundledDaemon);
  const bundledKeeper = payloadDir === null ? keeperBinary : join(payloadDir, KEEPER_BINARY_NAME);
  return { bundledDaemon, bundledKeeper, contents: dirname(dirname(bundledDaemon)) };
}

/**
 * The daemon executable this fixture should run, staged into a bundle if it is not one already.
 *
 * `ensure_keeper` -> `bundle_keeper_dir(current_exe())` reads the executable's own path and has no
 * environment override, so a `cargo` output can never activate keeper ownership: it answers
 * `PayloadAbsent`, `start_production` aborts, the routing flag is never written, and every session in
 * every case stays direct-owned no matter how the fixture is configured. Staging is therefore not a
 * convenience for the cases that ask for it -- it is the precondition of the routing this whole run
 * measures.
 *
 * A path that is already a bundle with its payload is returned untouched, so callers that stage
 * explicitly (`prepareKeeperBundle`) and callers that do not meet in the same place instead of
 * nesting one bundle inside another. The cache is keyed by the source's digest: two cases that use
 * the same build share one staged copy, and two *different* builds (`--daemon-generations`) never do.
 */
async function stageDaemonBundle(paths, daemonBinary, keeperBinary, cache = daemonBundleCache) {
  if (!daemonBinary) return daemonBinary;
  // Linux and Windows have no keeper at all (`daemon/mod.rs` gates the module to macOS), so staging
  // there would only change where the daemon runs from without enabling anything.
  if (process.platform !== 'darwin') return daemonBinary;
  if (isBundledDaemon({ daemonPath: daemonBinary })) return daemonBinary;
  if (!keeperBinary) return daemonBinary;
  const bytes = await readFile(daemonBinary);
  const key = sha256(bytes).slice(0, 16);
  const cached = cache.get(daemonBinary);
  if (cached && cached.key === key) return cached.staged;
  const appPath = join(paths.bundleDir, `Ferryx-${key}.app`);
  const staged = stageAppBundle({ appPath, daemonBinary, keeperBinary, sourceDir: paths.repo, triples: [] });
  cache.set(daemonBinary, { key, staged: staged.daemon, appPath, daemonSha256: staged.daemonSha256 });
  return staged.daemon;
}

/** Staged bundle per source path, so a run copies each build once. */
const daemonBundleCache = new Map();

// ---------------------------------------------------------------------------------------------
// Evidence: persistent, raw, appended - a crashed run still leaves its trace.
// ---------------------------------------------------------------------------------------------

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
    await appendFile(this.framesPath, `${JSON.stringify(Object.assign({ at: new Date().toISOString() }, record))}\n`);
  }

  async writeCase(id, value) {
    const path = join(this.casesDir, `${id}.json`);
    await writeFile(path, `${JSON.stringify(value, null, 2)}\n`);
    return path;
  }

  async writeRaw(name, buffer) {
    const path = join(this.dir, name);
    await writeFile(path, buffer);
    return path;
  }

  /**
   * Stores bytes verbatim beside the run, under their own subdirectory. A failing case is frequently
   * explained only by what a child process printed, and the case report is a parsed summary, so the
   * raw stream is kept with its sha256 instead of the few fields this harness happened to read.
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
   *
   * This is where the daemon's `tracing` output goes -- not to the stderr the process wrapper above
   * captures -- so a case that fails because the daemon did something is explained here or nowhere.
   * A missing log is `null`, never an error: the run may not have reached the point that writes it.
   */
  async daemonLog(dataDir) {
    const path = join(dataDir, 'logs', 'daemon.log');
    try {
      return { path, text: await readFile(path, 'utf8') };
    } catch (error) {
      if (error.code === 'ENOENT') return { path, text: null };
      throw error;
    }
  }
}

// ---------------------------------------------------------------------------------------------
// Keeper client: a faithful re-implementation of the 60-byte FXKP framing.
// ---------------------------------------------------------------------------------------------

class KeeperRefusal extends Error {
  constructor(code, message, offset) {
    super(`keeper refused ${code}: ${message}`);
    this.code = code;
    this.text = message;
    this.offset = offset;
  }
}

/**
 * `gap_payload` in `server.rs` emits `requestedAfterSequence` / `availableFromSequence` /
 * `retainedFromSequence` under `detail`, and the resume offset at the top level as `offset`.
 */
function replayGapPayload(payload, fallbackOffset) {
  const detail = payload?.detail ?? {};
  const availableFromSequence = Number(detail.availableFromSequence ?? payload?.offset ?? fallbackOffset ?? 0);
  return {
    endSequence: Number(payload?.offset ?? availableFromSequence),
    more: false,
    gap: {
      // `REPLAY` answers a lossy reset with the protocol's own typed event, whose payload carries
      // `code: "REPLAY_GAP"`. Attach carries the same offsets without a code, so a code is only ever
      // present on the replay path -- asserted there, never invented for the attach path.
      code: payload?.code ?? null,
      requestedAfterSequence: Number(detail.requestedAfterSequence ?? fallbackOffset ?? 0),
      availableFromSequence,
      retainedFromSequence: Number(detail.retainedFromSequence ?? availableFromSequence),
    },
  };
}

/**
 * The real `FXKP` connection. Replies are strictly one per request, so a pending waiter always belongs
 * to the request just written. The keeper can additionally originate `OUTPUT`, `REPLAY_GAP_EVENT` and
 * `SHUTDOWN`, which are collected separately so a case can assert either their presence or absence
 * without a request accidentally consuming them.
 */
class KeeperClient {
  constructor(socket, evidence) {
    this.socket = socket;
    this.evidence = evidence;
    this.buffer = Buffer.alloc(0);
    this.sequence = 0;
    /** Per correlation id: queued frames plus at most one waiting reader. */
    this.pending = new Map();
    /** Frames the keeper initiated (live output, a lossy reset, a shutdown notice). */
    this.unsolicited = [];
    /** Newest epoch this client presented per session; `KILL` must present the fenced one. */
    this.epochs = new Map();
    socket.on('data', (chunk) => this.#onData(chunk));
    // The keeper closing the socket mid-request is a product failure, not a harness bug, so it is
    // raised as a typed protocol failure. Otherwise every keeper crash would surface as HARNESS_ERROR
    // and read as our defect rather than a broken keeper.
    socket.on('close', () =>
      this.#failAll(
        fail(FAIL.PROTOCOL, 'the keeper closed the connection before answering'),
      ),
    );
    socket.on('error', (error) => this.#failAll(error));
  }

  static async connect(socketPath, evidence) {
    const socket = trackSocket(net.createConnection({ path: socketPath }), `keeper ${socketPath}`);
    try {
      await bounded(once(socket, 'connect'), `keeper connect ${socketPath}`, 10_000);
      socket.setNoDelay(true);
      return new KeeperClient(socket, evidence);
    } catch (error) {
      // A connect that throws or times out must not leave a live handle behind. Two effects follow a
      // leak, and both were observed in this harness: the handle keeps the event loop alive after the
      // report, so the run prints its verdict and never exits while still holding the daemon it
      // spawned; and it holds one of the keeper's `KEEPER_MAX_CONNECTIONS` slots, which every retry
      // loop that calls this consumes -- that is the `TOO_MANY_CONNECTIONS: this keeper serves at most
      // 8 connections at a time` failure another case reported instead of its own verdict.
      socket.destroy();
      throw error;
    }
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
        this.#failAll(fail(FAIL.PROTOCOL, `keeper framing version ${version}, this harness speaks ${KEEPER_FRAMING_VERSION}`));
        return;
      }
      const kind = this.buffer.readUInt16BE(6);
      const payloadLength = this.buffer.readUInt32BE(8);
      if (payloadLength > KEEPER_MAX_PAYLOAD_BYTES) {
        this.#failAll(fail(FAIL.PROTOCOL, `keeper announced ${payloadLength} payload bytes, cap is ${KEEPER_MAX_PAYLOAD_BYTES}`));
        return;
      }
      const total = KEEPER_HEADER_BYTES + payloadLength;
      if (this.buffer.length < total) return;
      const requestId = this.buffer.subarray(12, 48).toString('ascii');
      const frameSequence = Number(this.buffer.readBigUInt64BE(48));
      const fdCount = this.buffer.readUInt16BE(56);
      const flags = this.buffer.readUInt16BE(58);
      // This client passes no descriptors and the keeper sends none. A frame claiming descriptors
      // means the kernel dropped them, and the framing below would no longer be trustworthy.
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
      const frame = {
        kind,
        name: KIND_NAME.get(kind) ?? `0x${kind.toString(16)}`,
        requestId,
        frameSequence,
        payload,
      };
      // Replies are matched on the correlation id the keeper echoes back. Matching on arrival order
      // would misfile the first reply after an interleaved `OUTPUT` frame, which is a live-push
      // frame, not an answer.
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
      // Nothing is outstanding for this id. `output_run` and `SHUTDOWN` frames carry the all-dash id
      // by construction, so those are the live-push evidence; anything else means correlation is
      // lost and the reader must not keep guessing.
      const keeperOriginated =
        requestId === KEEPER_NO_REQUEST_ID || kind === KIND.Output || kind === KIND.Shutdown;
      if (keeperOriginated) {
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
    if (id.length !== 36 || !/^[\x21-\x7e]{36}$/.test(id)) {
      throw fail(FAIL.PROTOCOL, `request id ${JSON.stringify(id)} is not 36 printable ASCII characters`);
    }
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
   * The next frame belonging to one correlation id, from the per-request queue or the socket.
   *
   * A request may be answered by a run of several frames (`REPLAY` and a chunked attach), so the
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

  /** Forgets one correlation id once its request is fully answered. */
  #release(requestId) {
    const entry = this.pending.get(requestId);
    if (!entry) return;
    this.pending.delete(requestId);
    entry.waiter?.reject(fail(FAIL.PROTOCOL, `request ${requestId} was released while still awaiting a frame`));
    entry.waiter = null;
  }

  /** One request, one reply. An `ERROR` frame becomes a typed `KeeperRefusal`. */
  async call(kind, payload, expected, label, timeoutMs) {
    // A real correlation id, exactly 36 printable ASCII bytes: the keeper copies it into its reply,
    // so an interleaved `OUTPUT` frame can never be mistaken for the answer just requested.
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
      if (frame.requestId !== requestId) {
        throw fail(FAIL.PROTOCOL, `${label ?? KIND_NAME.get(kind)}: the refusal is correlated to ${frame.requestId}`);
      }
      throw new KeeperRefusal(frame.payload?.code, frame.payload?.message, frame.payload?.offset);
    }
    if (expected !== undefined && frame.kind !== expected) {
      throw fail(FAIL.PROTOCOL, `${label ?? KIND_NAME.get(kind)}: expected ${KIND_NAME.get(expected)}, got ${frame.name}`);
    }
    await this.evidence?.record({ wire: 'keeper', verb: KIND_NAME.get(kind), reply: frame.name, payload: frame.payload });
    return frame.payload ?? {};
  }

  /**
   * Waits for a frame the keeper sends on its own initiative and never for a reply. Used only to
   * measure that behaviour, so it deliberately does not send anything while it waits.
   */
  async nextFrame(label, timeoutMs) {
    const deadline = Date.now() + (timeoutMs ?? 5_000);
    for (;;) {
      if (this.unsolicited.length > 0) return this.unsolicited[0];
      if (this.closed) throw this.closed;
      const remaining = deadline - Date.now();
      if (remaining <= 0) throw fail(FAIL.DEADLINE, `${label}: no keeper-initiated frame within ${timeoutMs ?? 5_000}ms`);
      await new Promise((resolveWait) => setTimeout(resolveWait, Math.min(remaining, 50)));
    }
  }

  /** Drops the frame a measurement just observed, so the next measurement starts clean. */
  consumeFrame() {
    return this.unsolicited.shift() ?? null;
  }

  async hello() {
    const ack = await this.call(KIND.Hello, { protocolMajor: 1, protocolMinor: 0, keeperVersion: 'qa' }, KIND.HelloAck, 'keeper hello');
    if (Number(ack.protocolMajor) !== 1) {
      throw fail(FAIL.PROTOCOL, `keeper speaks protocol major ${ack.protocolMajor}, this harness speaks 1`);
    }
    return ack;
  }

  async spawn(options) {
    // `server.rs` reads an optional `requestId` and `keeper.rs::spawn` is idempotent on it, so a
    // retried spawn answers with the session that already exists instead of opening a second PTY.
    // The callers that need to retry pass the id from the first attempt; everyone else gets a fresh
    // one, which is exactly a first attempt.
    //
    // An echo fixture is asked to prove its tty mode before this returns. The marker path is passed
    // in the child's own environment rather than through each call site, so the ~18 spawn sites keep
    // their shape and no case can forget the wait. `SPAWN` is acknowledged at fork time, so without
    // it the first write below races the child's `stty` and lands in canonical mode.
    const isEchoChild = Array.isArray(options.argv)
      && options.argv.some((arg) => typeof arg === 'string' && arg.includes(RAW_TTY_SENTINEL));
    const env = Array.isArray(options.env) ? options.env.map(([key, value]) => [key, value]) : [];
    if (isEchoChild) {
      if (!rawTtyDir) throw fail(FAIL.PROTOCOL, 'the echo tty marker directory was never planned, so no write could be proven byte-exact');
      env.push([RAW_TTY_MARKER_ENV, rawTtyMarkerPath(options.sessionId)]);
    }
    const ack = await this.call(
      KIND.Spawn,
      {
        sessionId: options.sessionId,
        argv: options.argv,
        env,
        cwd: options.cwd,
        cols: options.cols ?? 80,
        rows: options.rows ?? 24,
        epoch: options.epoch ?? 1,
        requestId: options.requestId ?? randomUUID(),
      },
      KIND.SpawnAck,
      `keeper spawn ${options.sessionId}`,
      30_000,
    );
    if (isEchoChild) await awaitRawEchoChild(options.sessionId);
    return {
      sessionId: ack.sessionId,
      pid: Number(ack.pid),
      pgid: ack.pgid === null || ack.pgid === undefined ? null : Number(ack.pgid),
      processStartTime: ack.processStartTime === null || ack.processStartTime === undefined ? null : Number(ack.processStartTime),
      cols: Number(ack.cols),
      rows: Number(ack.rows),
      epoch: Number(ack.epoch),
      sequenceOffset: Number(ack.sequenceOffset ?? 0),
    };
  }

  async write(sessionId, epoch, bytes) {
    if (bytes.length > KEEPER_MAX_WRITE_BYTES) {
      throw fail(
        FAIL.OVERSIZED_FIXTURE,
        `write of ${bytes.length} bytes exceeds the keeper's ${KEEPER_MAX_WRITE_BYTES} byte write cap`,
      );
    }
    let total = 0;
    for (let offset = 0; offset < bytes.length; offset += KEEPER_MAX_WRITE_BYTES) {
      const slice = bytes.subarray(offset, offset + KEEPER_MAX_WRITE_BYTES);
      const ack = await this.call(KIND.Write, { sessionId, epoch, data: b64(slice) }, KIND.Write, `keeper write ${sessionId}`);
      total += Number(ack.written ?? 0);
    }
    this.epochs.set(sessionId, epoch);
    return total;
  }

  async attach(sessionId, epoch, afterSequence, options) {
    // `chunked: true` is opt-in on the keeper side: a peer that reads one frame per request would
    // desync on a run it never asked for, so the suffix is only streamed when asked for.
    const payload = { sessionId, epoch, chunked: options?.chunked !== false };
    // `liveOutput: true` is the other opt-in: `server.rs` arms the connection as the session's
    // output tap only when the peer asked for one, and echoes the decision in the ack. Without it no
    // `OUTPUT` frame is ever sent on this connection, so a case that expects live push must ask.
    if (options?.liveOutput) payload.liveOutput = true;
    if (afterSequence !== null && afterSequence !== undefined) payload.afterSequence = afterSequence;
    const requestId = randomUUID();
    const bytes = this.#encode(KIND.Attach, payload, requestId);
    const deadline = Date.now() + 120_000;
    const remaining = () => Math.max(1_000, deadline - Date.now());
    const next = () => this.#next(`keeper attach ${sessionId}`, remaining(), requestId);
    await this.#write(bytes, `keeper attach ${sessionId} write`);
    try {
      const ack = await next();
      if (ack.kind === KIND.Error) {
        throw new KeeperRefusal(ack.payload?.code, ack.payload?.message, ack.payload?.offset);
      }
      if (ack.kind !== KIND.AttachAck) {
        throw fail(FAIL.PROTOCOL, `keeper attach answered ${ack.name}`);
      }
      this.epochs.set(sessionId, Number(ack.payload.epoch));
      // A suffix above the inline cap arrives as `REPLAY_CHUNK`* then a final `REPLAY_END(more:false)`
      // on this same correlation id. Reading only the ack would silently truncate the history.
      let data = unb64(ack.payload.data);
      const reported = ack.payload.replayGap ?? null;
      // The run only follows when there is no gap: a refused range has nothing to stream, so waiting
      // for chunks here would sit on the deadline and report a false protocol failure.
      if (ack.payload.dataChunked && !reported) {
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
          throw fail(FAIL.PROTOCOL, `attach run sent ${frame.name}, expected REPLAY_CHUNK or REPLAY_END`);
        }
      }
      return {
        sessionId: ack.payload.sessionId,
        epoch: Number(ack.payload.epoch),
        sequenceOffset: Number(ack.payload.sequenceOffset ?? 0),
        checkpointId: ack.payload.checkpointId ?? null,
        // The keeper echoes the tap decision, so the case can assert the connection it got is the
        // one it asked for instead of assuming the flag was honoured.
        liveOutput: ack.payload.liveOutput === true,
        data,
        gap: reported ? replayGapPayload({ detail: reported }, 0).gap : null,
      };
    } finally {
      this.#release(requestId);
    }
  }

  /** `REPLAY` yields chunks then `REPLAY_END`, or a single `REPLAY_GAP_EVENT`. */
  /**
   * The whole retained suffix, not one page.
   *
   * `server::replay_run` pages at `RESPONSE_BYTES_PER_REQUEST` (512 KiB) and sets `more: true` when
   * there is another page. A caller that reads one page and stops would silently believe a 720 KiB log
   * was 512 KiB, so the continuation is followed here rather than at each call site.
   */
  async replayAll(sessionId, fromSequence, timeoutMs) {
    const chunks = [];
    let cursor = fromSequence;
    let gap = null;
    for (;;) {
      const page = await this.replay(sessionId, cursor, timeoutMs);
      if (page.gap) {
        gap = page.gap;
        break;
      }
      chunks.push(...page.chunks);
      if (!page.more) break;
      if (page.endSequence <= cursor) {
        throw fail(FAIL.PROTOCOL, `replay ${sessionId} reported more pages but did not advance past ${cursor}`);
      }
      cursor = page.endSequence;
    }
    return {
      chunks,
      endSequence: chunks.reduce((at, chunk) => at + chunk.data.length, fromSequence),
      more: false,
      gap,
      bytes: Buffer.concat(chunks.map((chunk) => chunk.data)),
    };
  }

  async replay(sessionId, fromSequence, timeoutMs) {
    const requestId = randomUUID();
    const bytes = this.#encode(KIND.Replay, { sessionId, fromSequence }, requestId);
    const deadline = Date.now() + (timeoutMs ?? 60_000);
    const remaining = () => Math.max(1_000, deadline - Date.now());
    const next = () => this.#next(`keeper replay ${sessionId}`, remaining(), requestId);
    await this.#write(bytes, `keeper replay ${sessionId} write`);
    try {
      let frame = await next();
      const chunks = [];
      for (;;) {
        if (frame.kind === KIND.ReplayChunk) {
          chunks.push({ sequenceOffset: Number(frame.payload.sequenceOffset ?? 0), data: unb64(frame.payload.data) });
          frame = await next();
          continue;
        }
        if (frame.kind === KIND.ReplayEnd) {
          return {
            chunks,
            endSequence: Number(frame.payload.sequenceOffset ?? 0),
            more: Boolean(frame.payload.more),
            gap: null,
          };
        }
        // A lossy reset is answered on this same correlation id: `REPLAY` returns the gap event as its
        // reply, not as an unsolicited frame.
        if (frame.kind === KIND.ReplayGapEvent) {
          return { chunks, ...replayGapPayload(frame.payload, frame.payload?.offset ?? 0) };
        }
        if (frame.kind === KIND.Error) {
          throw new KeeperRefusal(frame.payload?.code, frame.payload?.message, frame.payload?.offset);
        }
        throw fail(FAIL.PROTOCOL, `keeper replay: unexpected ${frame.name}`);
      }
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
      generation: entry.generation ?? null,
      pid: entry.pid === null || entry.pid === undefined ? null : Number(entry.pid),
      epoch: Number(entry.epoch ?? 0),
      sequenceOffset: Number(entry.sequenceOffset ?? 0),
      replayRetainedBytes: Number(entry.replayRetainedBytes ?? 0),
      replayResets: Number(entry.replayResets ?? 0),
      lastActivityMillis: Number(entry.lastActivityMillis ?? 0),
      cols: Number(entry.cols ?? 0),
      rows: Number(entry.rows ?? 0),
      owner: entry.owner ?? null,
      focused: Boolean(entry.focused),
    }));
  }

  async detach(sessionId, epoch) {
    await this.call(KIND.Detach, { sessionId, epoch }, KIND.Detach, `keeper detach ${sessionId}`);
  }

  async checkpointInstall(sessionId, epoch, checkpointId, sequenceOffset, bytes) {
    // `server.rs` runs `fenced_epoch` on `CHECKPOINT_INSTALL`, so an install without `epoch` is
    // answered with `BAD_FRAME`, not a stored checkpoint.
    const ack = await this.call(
      KIND.CheckpointInstall,
      { sessionId, epoch, checkpointId, sequenceOffset, totalBytes: bytes.length, data: b64(bytes) },
      KIND.CheckpointAck,
      `keeper checkpoint ${sessionId}`,
      30_000,
    );
    return {
      checkpointId: ack.checkpointId ?? null,
      sequenceOffset: Number(ack.sequenceOffset ?? 0),
      storedBytes: Number(ack.storedBytes ?? 0),
    };
  }

  async checkpointQuery(sessionId) {
    const ack = await this.call(KIND.CheckpointQuery, { sessionId }, KIND.CheckpointAck, `keeper checkpoint query ${sessionId}`);
    return {
      checkpointId: ack.checkpointId ?? null,
      sequenceOffset: Number(ack.sequenceOffset ?? 0),
      storedBytes: Number(ack.storedBytes ?? 0),
    };
  }

  /**
   * Opens a chunked checkpoint upload.
   *
   * `server.rs` treats `CHECKPOINT_INSTALL` *without* `data` as the `begin` of a chunked upload and
   * answers with its progress; `checkpoint.rs::begin` charges the whole declared size to the budget
   * before a single byte is accepted, so this is also the call that raises
   * `CHECKPOINT_BUDGET_EXHAUSTED` for an oversize declaration without allocating anything.
   */
  async checkpointBegin(sessionId, epoch, checkpointId, sequenceOffset, totalBytes) {
    const ack = await this.call(
      KIND.CheckpointInstall,
      { sessionId, epoch, checkpointId, sequenceOffset, totalBytes },
      KIND.CheckpointAck,
      `keeper checkpoint begin ${sessionId}`,
      30_000,
    );
    return {
      checkpointId: ack.checkpointId ?? checkpointId,
      sequenceOffset: Number(ack.sequenceOffset ?? 0),
      storedBytes: Number(ack.storedBytes ?? 0),
      totalBytes: Number(ack.totalBytes ?? 0),
      complete: ack.complete === true,
    };
  }

  /**
   * Appends one chunk to an open upload. `chunkIndex` must be the next index in order:
   * `checkpoint.rs::push_chunk` refuses an out-of-order index and any run that overruns the declared
   * total with `CHECKPOINT_CHUNK_MISMATCH`, never a silent truncation.
   */
  async checkpointChunk(sessionId, epoch, checkpointId, chunkIndex, bytes) {
    // The frame carries base64 of these bytes plus a small JSON envelope, all under the 1 MiB frame
    // cap. Refusing an oversize fixture here is a harness bug, not a keeper verdict.
    const encoded = b64(bytes);
    if (encoded.length + 512 > KEEPER_MAX_PAYLOAD_BYTES) {
      throw fail(
        FAIL.OVERSIZED_FIXTURE,
        `a ${bytes.length} byte checkpoint chunk encodes to ${encoded.length} bytes, over the ${KEEPER_MAX_PAYLOAD_BYTES} byte frame cap`,
      );
    }
    const ack = await this.call(
      KIND.CheckpointChunk,
      { sessionId, epoch, checkpointId, chunkIndex, data: encoded },
      KIND.CheckpointAck,
      `keeper checkpoint chunk ${sessionId}#${chunkIndex}`,
      30_000,
    );
    return {
      checkpointId: ack.checkpointId ?? checkpointId,
      sequenceOffset: Number(ack.sequenceOffset ?? 0),
      storedBytes: Number(ack.storedBytes ?? 0),
      totalBytes: Number(ack.totalBytes ?? 0),
      complete: ack.complete === true,
    };
  }

  /**
   * Commits a completed upload in one step, which is what makes installation atomic: until this
   * call lands, `CHECKPOINT_QUERY` keeps reporting the previous commit.
   *
   * There is no abort verb on the wire -- `server.rs::dispatch` handles Install/Chunk/Commit/Query/
   * Fetch and nothing else, `Keeper::abort_checkpoint` has no call site outside the store's own
   * tests -- so a partial upload is only ever assertable as "never becomes the committed
   * checkpoint", never as "was explicitly discarded".
   */
  async checkpointCommit(sessionId, epoch, checkpointId, sequenceOffset) {
    const ack = await this.call(
      KIND.CheckpointCommit,
      { sessionId, epoch, checkpointId, sequenceOffset },
      KIND.CheckpointAck,
      `keeper checkpoint commit ${sessionId}`,
      30_000,
    );
    return {
      checkpointId: ack.checkpointId ?? null,
      sequenceOffset: Number(ack.sequenceOffset ?? 0),
      storedBytes: Number(ack.storedBytes ?? 0),
    };
  }

  /**
   * Reads a committed checkpoint back, byte for byte.
   *
   * `CHECKPOINT_FETCH` pages: each page is `CHECKPOINT_FETCH_CHUNK`* then one `CHECKPOINT_FETCH_END`
   * carrying `nextByte` and `complete`, all on one correlation id. The cursor only advances by the
   * bytes actually received, so a page that arrives short is detected by the reassembly comparison
   * rather than silently shortening the history. `fetchedBytes` is what the caller compares against
   * what it installed.
   */
  async checkpointFetch(sessionId, epoch, checkpointId, sequenceOffset, timeoutMs = 60_000) {
    const chunks = [];
    let cursor = 0;
    let complete = false;
    for (;;) {
      const requestId = randomUUID();
      const bytes = this.#encode(
        KIND.CheckpointFetch,
        { sessionId, epoch, checkpointId, sequenceOffset, fromByte: cursor },
        requestId,
      );
      const deadline = Date.now() + timeoutMs;
      const next = () =>
        this.#next(`keeper checkpoint fetch ${sessionId}`, Math.max(1_000, deadline - Date.now()), requestId);
      await this.#write(bytes, `keeper checkpoint fetch ${sessionId} write`);
      try {
        for (;;) {
          const frame = await next();
          if (frame.kind === KIND.Error) {
            throw new KeeperRefusal(frame.payload?.code, frame.payload?.message, frame.payload?.offset);
          }
          if (frame.kind === KIND.CheckpointFetchChunk) {
            // A chunk carries the identity it belongs to, so a page from a different commit cannot be
            // spliced into this one without being noticed here.
            if (frame.payload?.checkpointId !== checkpointId) {
              throw fail(
                FAIL.PROTOCOL,
                `checkpoint fetch returned ${JSON.stringify(frame.payload?.checkpointId)}, not ${checkpointId}`,
              );
            }
            const fromByte = Number(frame.payload?.fromByte ?? 0);
            if (fromByte !== cursor) {
              throw fail(FAIL.BYTE_ACCOUNTING, `checkpoint chunk starts at ${fromByte}, not the cursor ${cursor}`);
            }
            const data = unb64(frame.payload?.data);
            chunks.push(data);
            cursor += data.length;
            continue;
          }
          if (frame.kind === KIND.CheckpointFetchEnd) {
            const nextByte = Number(frame.payload?.nextByte ?? 0);
            if (nextByte !== cursor) {
              throw fail(
                FAIL.BYTE_ACCOUNTING,
                `checkpoint fetch end advances to ${nextByte}, but ${cursor} bytes were received`,
              );
            }
            complete = frame.payload?.complete === true;
            break;
          }
          throw fail(FAIL.PROTOCOL, `checkpoint fetch sent ${frame.name}`);
        }
      } finally {
        this.#release(requestId);
      }
      if (complete) break;
    }
    return {
      checkpointId,
      sequenceOffset: Number(sequenceOffset),
      bytes: Buffer.concat(chunks),
      complete,
    };
  }

  async resize(sessionId, epoch, cols, rows) {
    await this.call(KIND.Resize, { sessionId, epoch, cols, rows }, KIND.Resize, `keeper resize ${sessionId}`);
  }

  /**
   * `server::fenced_epoch` requires an `epoch` on `KILL`, so a kill always presents one. Omitting it
   * would be answered with `BAD_FRAME`, not a kill.
   *
   * The epoch is normally read back from `OWNERSHIP_LIST`: `Keeper::ownership` passes
   * `epochs.newest(..)` into `Session::snapshot`, and that snapshot applies `with_epoch`, so the
   * registry reports the epoch actually in force. Tracking the newest epoch this client presented is
   * the fallback for a session the registry does not list, which is exact here because nothing else
   * talks to this keeper.
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

// ---------------------------------------------------------------------------------------------
// Daemon client: NDJSON on `daemon.sock`, protocol version 6.
// ---------------------------------------------------------------------------------------------

class DaemonRefusal extends Error {
  constructor(code, message) {
    super(`daemon refused ${code ?? 'error'}: ${message}`);
    this.code = code;
    this.text = message;
  }
}

/** Line-delimited JSON with an ordered frame queue: daemon replies and stream frames share one pipe. */
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
        let parsed;
        try {
          parsed = JSON.parse(line);
        } catch (error) {
          parsed = { type: 'unparseable', raw: line, error: String(error) };
        }
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
    // `pending` is an array of waiters, not a map: iterating it as `[key, entry]` pairs would
    // destructure each waiter object into nonsense and throw before the rejection was delivered.
    for (const entry of this.pending) {
      entry.reject?.(error);
    }
    this.pending.length = 0;
  }

  send(object) {
    this.socket.write(`${JSON.stringify(object)}\n`);
  }

  next(label, timeoutMs) {
    if (this.queue.length) return Promise.resolve(this.queue.shift());
    if (this.closed) return Promise.reject(this.closed);
    // `Infinity` is a real argument, not a missing one: it means "await the next frame for as long as
    // this stream is open". An attached stream is idle whenever the child is, and a reader that read
    // that silence as death would stop draining a live stream and turn the case's own wait into the
    // only thing that ever times out. The default stays bounded for request/response callers.
    const unbounded = timeoutMs === Infinity;
    const window = timeoutMs ?? 30_000;
    return new Promise((resolveFrame, rejectFrame) => {
      const timer = unbounded
        ? null
        : setTimeout(() => {
            const index = this.pending.indexOf(entry);
            if (index >= 0) this.pending.splice(index, 1);
            rejectFrame(fail(FAIL.DEADLINE, `${label}: no daemon frame within ${window}ms`));
          }, window);
      const entry = {
        resolve: (frame) => {
          if (timer) clearTimeout(timer);
          resolveFrame(frame);
        },
        reject: (error) => {
          if (timer) clearTimeout(timer);
          rejectFrame(error);
        },
      };
      this.pending.push(entry);
    });
  }

  close() {
    this.socket.destroy();
  }
}

class DaemonClient {
  constructor(channel, identity, evidence, protocolVersion) {
    this.channel = channel;
    this.identity = identity;
    this.evidence = evidence;
    // The version this client actually spoke. A daemon compares the presented version against its
    // own, so a mixed-version pair has to be addressed per daemon rather than with one global value.
    this.protocolVersion = protocolVersion ?? DAEMON_PROTOCOL_VERSION;
  }

  static async connect(socketPath, evidence, protocolVersion, negotiate) {
    const version = protocolVersion ?? DAEMON_PROTOCOL_VERSION;
    const socket = trackSocket(net.createConnection({ path: socketPath }), `daemon ${socketPath}`);
    try {
      return await DaemonClient.handshake(socket, socketPath, evidence, version, negotiate);
    } catch (error) {
      // Same rule as the keeper client: a failed connect or handshake destroys the socket it made.
      // The readiness loops call this once per attempt, so a leak here is one handle and one peer
      // slot per retry.
      socket.destroy();
      throw error;
    }
  }

  /** The handshake half of `connect`, split out so the socket has exactly one owner for teardown. */
  static async handshake(socket, socketPath, evidence, version, negotiate) {
    let replaced = false;
    try {
      await bounded(once(socket, 'connect'), `daemon connect ${socketPath}`, 10_000);
      let channel = new LineChannel(socket);
      channel.send({ type: 'handshake', version });
      let identity = await channel.next('daemon handshake');
      // A replacement runs a *different build*, which may speak a different protocol revision. The
      // daemon answers `protocolMismatch` with the version it wants, so asking again is the real
      // handshake a new client would perform -- not a bypass. Only attempted when the caller asked to
      // negotiate; every other case keeps its strict single-version check.
      if (negotiate && identity.type === 'protocolMismatch' && Number.isInteger(identity.expectedVersion)) {
        version = identity.expectedVersion;
        // The daemon closes the connection right after flushing the mismatch
        // (`server.rs:9472-9474`), so a second handshake written to this channel goes to a socket the
        // peer has already shut: the reply never arrives and the negotiation dies as a deadline
        // instead of a version answer. Mirror `proxy.rs:133-137` and redo the handshake on a fresh
        // connection to the same path, which is what a client of the daemon's version does from the
        // start.
        channel.close();
        replaced = true;
        socket = trackSocket(net.createConnection({ path: socketPath }), `daemon ${socketPath}`);
        await bounded(once(socket, 'connect'), `daemon connect ${socketPath} (version negotiation)`, 10_000);
        channel = new LineChannel(socket);
        channel.send({ type: 'handshake', version });
        identity = await channel.next('daemon handshake after version negotiation');
      }
      if (identity.type === 'protocolMismatch') {
        throw fail(
          FAIL.PROTOCOL,
          `daemon expects protocol ${identity.expectedVersion}, this client sent ${version}`,
        );
      }
      if (identity.type !== 'handshakeOk') {
        throw fail(FAIL.PROTOCOL, `daemon handshake answered ${identity.type}`);
      }
      await evidence?.record({ wire: 'daemon', verb: 'handshake', sentVersion: version, payload: identity });
      return new DaemonClient(channel, identity, evidence, version);
    } catch (error) {
      // `connect`'s catch destroys the socket it created; the reconnected one is this method's own.
      if (replaced) socket.destroy();
      throw error;
    }
  }

  async call(request, label, timeoutMs) {
    this.channel.send(request);
    const reply = await this.channel.next(label ?? `daemon ${request.type}`, timeoutMs);
    await this.evidence?.record({ wire: 'daemon', verb: request.type, reply: reply.type });
    if (reply.type === 'error') throw new DaemonRefusal(reply.code, reply.message);
    return reply;
  }

  async spawn(options) {
    // `session_service.rs::handle_spawn_requested` refuses any `requestedSessionId` that is not a
    // canonical lowercase UUID with `INVALID_ARGUMENT`, so a case that invents `mixed-direct-<hex>`
    // fails inside the harness with a daemon refusal that reads like a product bug. The shape is
    // asserted here, at the one place every daemon spawn passes through, so a new case cannot
    // reintroduce it silently.
    if (options.sessionId !== undefined && !CANONICAL_UUID.test(String(options.sessionId))) {
      throw fail(
        FAIL.USAGE,
        `daemon spawn sessionId ${JSON.stringify(options.sessionId)} is not a canonical lowercase UUID, `
          + 'which is the only shape the daemon accepts. A readable prefix has to be carried in the '
          + 'label and evidence, not in the id.',
      );
    }
    const reply = await this.call(
      {
        type: 'spawn',
        clientRequestId: `phase3-${randomUUID()}`,
        workspaceId: options.workspaceId,
        worktree: null,
        cwd: options.cwd,
        cols: options.cols ?? 80,
        rows: options.rows ?? 24,
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
   * `attach` converts this connection into a stream: one `attachOk` carrying the retained history,
   * then live `output` frames on the same socket. Bytes after the snapshot are proved to arrive
   * exactly once, either inline or via `replayGap.startSequence` continuation.
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
      history,
      segments: first.historySegments ?? first.segments ?? [],
      ptyCols: first.ptyCols ?? null,
      ptyRows: first.ptyRows ?? null,
      stream: this.channel,
    };
  }

  async closeSession(sessionId) {
    await this.call({ type: 'close', sessionId }, `daemon close ${sessionId}`);
  }

  /**
   * The predecessor's half of a graceful replacement: `server.rs` binds a one-shot legacy socket,
   * starts accepting on it, and answers with the path a successor must be started with. The
   * sessions listed here are what the successor is expected to adopt.
   */
  async prepareHandover() {
    const reply = await this.call({ type: 'prepareHandover' }, 'daemon prepareHandover', 60_000);
    if (reply.type === 'handoverRejected') {
      throw new DaemonRefusal('HANDOVER_REJECTED', reply.reason);
    }
    if (reply.type !== 'prepareHandoverOk') {
      throw fail(FAIL.PROTOCOL, `prepareHandover answered ${reply.type}`);
    }
    return {
      legacySocketPath: reply.legacySocketPath ?? null,
      activeSessions: Array.isArray(reply.activeSessions) ? reply.activeSessions : [],
    };
  }

  close() {
    this.channel.close();
  }
}

/**
 * Consumes an attached daemon stream: collects output and demultiplexes the frames that are not
 * bytes. Subscribing happens at the snapshot, before any input is written, so the markers asserted
 * later cannot have been missed in the window between attach and the first read.
 */
class AttachedSession {
  constructor(attachment, evidence) {
    this.attachment = attachment;
    this.evidence = evidence;
    this.chunks = [];
    this.other = [];
    this.pump = this.#run();
  }

  async #run() {
    for (;;) {
      let frame;
      try {
        // No deadline: the child is the clock here. A `writeToSession` caller supplies the bound, and
        // an idle stream between two writes must stay open rather than end this reader.
        frame = await this.attachment.stream.next(`daemon stream ${this.attachment.sessionId}`, Infinity);
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

  // No `write` here on purpose. An attached connection is OUTPUT-ONLY: `daemon/server.rs:10542-10547`
  // reads the client side of an attach stream only to notice a disconnect, and breaks the stream on
  // the first byte that is not EOF ("Unexpected client data on attach stream"). A `write` on this
  // object would therefore tear down the very stream the case is reading. Input travels as
  // `DaemonRequest::Write` over a separate control connection, which is what the real client does.
  // `AttachedSession` keeps no reference to a writable socket so the mistake cannot come back.

  async waitFor(label, timeoutMs, predicate) {
    // A stream that ended cannot start delivering again, so waiting out the deadline would report the
    // harness's own patience as the product's silence. The end reason is the fact worth having: it
    // says whether the daemon closed the stream, a frame failed to parse, or the socket died. The
    // predicate is tested first, so bytes that already arrived still count after a clean close.
    return until(label, timeoutMs, () => {
      if (predicate(this.bytes, this.chunks)) return this.bytes;
      if (this.ended) {
        throw fail(
          FAIL.MISSING_OUTPUT,
          `${label}: the attach stream ended before this condition held (${describeStreamEnd(this.ended)}), after ${this.chunks.length} output frame(s) and ${this.bytes.length} byte(s)`,
        );
      }
      return null;
    });
  }
}

/** The one-line reason an attach stream stopped, whether it was a typed failure or a socket error. */
function describeStreamEnd(error) {
  if (error instanceof AcceptanceError) return `${error.kind} ${error.code}: ${error.detail}`;
  const code = error?.code ? ` (${error.code})` : '';
  return `${String(error?.message ?? error)}${code}`;
}

/**
 * Folds a tty's local echo into the bytes the writer typed, so an echo assertion can match both a
 * transparent child and a real shell.
 *
 * Two rewrites happen between a write and its echo when the child is a login shell rather than
 * `echoChild()`'s raw `exec cat`:
 *   - the line editor repaints its line: zsh re-emits the first character, erases it with `\x08` and
 *     redraws the rest, so `PING:` arrives as `P\x08ING:`. Applying `\x08` as a destructive backspace
 *     rebuilds the intended text.
 *   - output post-processing (`ONLCR`) rewrites `\n` to `\r\n`, and zsh renders Enter as
 *     `\x1b[?2004l\r\r\n`. Dropping every `\r` leaves a single `\n`, so the marker and its line
 *     ending read the same as they do coming back from `cat`.
 *
 * Only CR and BS are folded: the assertion stays byte-exact about the payload itself.
 *
 * `tailBytes` folds only the end of a long buffer. The echo being waited for is always the most recent
 * output, and a case that attaches to a 720 KiB history would otherwise re-fold the whole stream on
 * every poll of `until`'s 25ms loop.
 */
function foldTtyEcho(buffer, tailBytes) {
  const start = tailBytes === undefined ? 0 : Math.max(0, buffer.length - tailBytes);
  const out = Buffer.alloc(buffer.length - start);
  let length = 0;
  for (let index = start; index < buffer.length; index += 1) {
    const byte = buffer[index];
    if (byte === 0x0d) continue;
    if (byte === 0x08) {
      if (length > 0) length -= 1;
      continue;
    }
    out[length] = byte;
    length += 1;
  }
  return out.subarray(0, length);
}

/**
 * Writes `bytes` into `sessionId` over a control connection, then waits for their return on `session`.
 *
 * The two connections are separate for a reason the daemon enforces, not one this harness invents:
 * an attach stream carries output only and is closed on any client byte (`daemon/server.rs:10542`),
 * so `DaemonRequest::Write` has to travel on a socket of its own. Every input site in this file goes
 * through here so there is one place that can be wrong, and it is the place that was wrong: six
 * cases used to write on the attach stream and the daemon silently dropped their read side.
 *
 * `expect` is the byte sequence that proves the write arrived, and it is what the wait is keyed on.
 * It defaults to `bytes`, minus the line terminator it ends with; a case whose child rewrites its input
 * passes the rewritten form.
 *
 * The terminator is dropped and both sides are folded through `foldTtyEcho` because only SOME children
 * here echo byte for byte. `echoChild()` runs `stty raw -echo -icanon -opost; exec cat` and does; a
 * session spawned through `ctx.daemon.spawn` with no argv is the configured LOGIN SHELL, and a tty in
 * its default mode does not: `opost` returns the `\n` as `\r\n`, and the line editor repaints the
 * first character before redrawing the line, so `PING:abc` comes back as `P\x08ING:abc`. Waiting for
 * the literal written bytes was therefore waiting for bytes the tty cannot emit, and it read as a lost
 * write. The frames prove otherwise: run14b's 15-byte keeper frame `09b65f8b` is exactly
 * `S\x08SEQ-DISABLED:`, and its 30-byte frame `6d44f101` is `H\x08HANDOVER:4619b22d\x1b[?2004l\r\r\n`
 * -- the marker echoed AND executed, one repaint away from what was asked for.
 */
async function writeToSession(ctx, sessionId, session, bytes, label, timeoutMs, expect) {
  const control = await DaemonClient.connect(ctx.daemonSocketPath, ctx.evidence, ctx.daemon?.protocolVersion);
  try {
    await control.write(sessionId, bytes);
  } finally {
    // The write is acknowledged before the connection is closed, so the echo is still delivered on
    // the attach socket: closing the control side does not disturb the reader.
    control.close();
  }
  const declared = expect ?? bytes;
  let end = declared.length;
  while (end > 0 && (declared[end - 1] === 0x0a || declared[end - 1] === 0x0d)) end -= 1;
  const wanted = foldTtyEcho(declared.subarray(0, end));
  try {
    return await session.waitFor(label, timeoutMs ?? 30_000, (buffer) => {
      // The echo is the newest output, so folding the tail is enough and keeps a large attach history
      // from being re-folded on every poll. The slack covers a repaint that straddles the window edge.
      return foldTtyEcho(buffer, wanted.length * 4 + 4096).includes(wanted);
    });
  } catch (error) {
    // Still silent, but alive: the stream did not end before the deadline, so the bytes went somewhere
    // else -- a write routed to a different daemon than the one serving this attach, or no pump from
    // the owner to the hub. Naming that in the failure is the difference between "no output" and a
    // located fault; the raw process streams are captured beside the case for the rest.
    if (error instanceof AcceptanceError && error.code === FAIL.DEADLINE) {
      throw fail(
        error.code,
        `${error.detail} (attach stream still open, ${session.chunks.length} frame(s) / ${session.bytes.length} byte(s) received, `
          + `${JSON.stringify(wanted.toString('latin1'))} not among them)`,
      );
    }
    throw error;
  }
}

/**
 * Waits until the `--handover-from` successor itself answers on the canonical socket, then rebinds
 * `ctx.daemon` to it.
 *
 * `--handover-from` does NOT bind the canonical socket: the successor adopts over the handover socket
 * and only serves the inherited listener once it commits (`daemon/server.rs:6336`, `:5920-5923`).
 * Until then `<runtimeDir>/daemon.sock` is still the predecessor's, so `startDaemon`'s readiness
 * handshake -- and every request issued straight after it -- can be answered by either process. The
 * attach and the write travel on separate connections, so without this wait a case can attach to the
 * draining predecessor and write to the successor: the session then receives the bytes, publishes
 * them where nobody is listening, and the case reports its own split-brain as "condition never held".
 *
 * The handshake carries the serving process's own pid (`server.rs:7274`), so the event is observable:
 * connected pid equals the successor's. `ctx.daemonProcess` is the successor because `startDaemon`
 * assigned it last, which is why the caller must invoke this immediately after that call.
 */
async function awaitSuccessorServing(ctx, label) {
  const successorPid = ctx.daemonProcess.pid;
  const version = ctx.daemon?.protocolVersion;
  let servedBy;
  try {
    servedBy = await until(
      label,
      60_000,
      async () => {
        // A successor that aborted its commit exits, and a deadline alone would hide which side went
        // wrong; its own stderr is the reason.
        if (ctx.daemonProcess.exited) {
          throw fail(
            FAIL.PROTOCOL,
            `the successor exited (${JSON.stringify(ctx.daemonProcess.exited)}) before serving ${ctx.daemonSocketPath}: ${ctx.daemonProcess.stderr.slice(-2_048)}`,
          );
        }
        try {
          // No evidence for the probe: it answers "who is listening right now", and every retry would
          // add another handshake frame to the trail without saying anything new.
          const probe = await DaemonClient.connect(ctx.daemonSocketPath, null, version);
          const pid = probe.identity?.pid ?? null;
          probe.close();
          return pid === successorPid ? pid : null;
        } catch (error) {
          // A refusal this harness authored is a verdict, not a retry. A plain connect failure
          // (ECONNREFUSED, a reset from the retiring predecessor) is the expected shape here.
          if (error instanceof AcceptanceError) throw error;
          return null;
        }
      },
      // No fixed sleep: each attempt is a real connect, which is itself the observation.
      250,
    );
  } catch (error) {
    await ctx.evidence
      .record({ step: 'successor-never-served', label, successorPid, error: String(error?.detail ?? error) })
      .catch(() => null);
    throw error;
  }
  await ctx.evidence.record({ step: 'successor-serving', label, successorPid, servedBy });
  // The client `startDaemon` returned was bound to whichever process answered then, and the retiring
  // predecessor aborts its own client connections at the commit point (`handover.rs:2040-2049`).
  // Reconnect so every later request -- including the attach and the write, which take separate
  // connections -- reaches the daemon that actually owns the adopted session.
  const successor = await DaemonClient.connect(ctx.daemonSocketPath, ctx.evidence, version);
  await successor
    .call({ type: 'registerWorkspace', workspaceId: ctx.workspaceId, repoRoot: ctx.paths.repo }, 'registerWorkspace')
    .catch(() => null);
  ctx.daemon.close();
  ctx.daemon = successor;
  return servedBy;
}

// ---------------------------------------------------------------------------------------------
// Process identity: the acceptance subject, so it is proved against the OS, not against a claim.
// ---------------------------------------------------------------------------------------------

/**
 * `ps -ax -o pid=,ppid=,lstart=` with `LC_ALL=C`. `lstart` is what macOS exposes for a process start
 * time, and it is compared at second granularity because that is the resolution the OS reports.
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
  try {
    await bounded(once(child, 'close'), 'ps -ax', 20_000);
  } catch (error) {
    // A `ps` that outlived its deadline is still a live child of this process. It used to be left
    // running, which is a second way for the harness to hold the event loop open after its report.
    child.kill('SIGKILL');
    throw error;
  }
  const rows = new Map();
  for (const line of stdout.split('\n')) {
    const match = /^\s*(\d+)\s+(\d+)\s+(.+?)\s*$/.exec(line);
    if (!match) continue;
    const startedAt = Date.parse(match[3]);
    rows.set(Number(match[1]), { ppid: Number(match[2]), lstart: match[3], startedAt });
  }
  return rows;
}

/**
 * `(pid, start time)` is the identity: a pid alone is reusable, so every identity assertion compares
 * the reported start time with the one the OS reports.
 *
 * The two reporters use different units and that is not a detail to paper over:
 *   - the keeper's `process_start_time` is epoch MICROseconds on both platforms -- macOS
 *     `pbi_start_tvsec * 1_000_000 + pbi_start_tvusec`, and on Linux `btime * 1_000_000 +
 *     ticks * 1_000_000 / CLK_TCK`, because `/proc/<pid>/stat` counts since boot and `btime` from
 *     `/proc/stat` is what turns that into an instant (`keeper.rs::process_start_time`). Reading the
 *     raw stat field would be an uptime, i.e. a start time in 1970, on a host up for days;
 *   - the daemon's `localChildStartTime` is `start_token / 1000`, i.e. MILLIseconds
 *     (`terminal/service.rs::keeper_start_time_ms`, `suspension/unix.rs`).
 *
 * So a comparison between them must convert, and the OS comparison below is made at second
 * granularity because that is `lstart`'s resolution.
 */
function startTimeToMicros(value, unit) {
  if (unit === 'ms') return Number(value) * 1000;
  return Number(value);
}

async function assertProcessIdentity(table, pid, startTime, label, unit = 'us') {
  if (!Number.isInteger(pid) || pid <= 0) {
    throw fail(FAIL.IDENTITY_MISMATCH, `${label}: pid ${JSON.stringify(pid)} is not a process`);
  }
  if (!Number.isFinite(startTime) || startTime <= 0) {
    throw gap(GAP.IDENTITY_START_TIME_UNPROVEN, `${label}: no start time was reported for pid ${pid}`);
  }
  const startTimeMicros = startTimeToMicros(startTime, unit);
  const row = table.get(pid);
  if (!row) throw fail(FAIL.IDENTITY_MISMATCH, `${label}: pid ${pid} is absent from the process table`);
  if (Number.isFinite(row.startedAt)) {
    const reportedSeconds = Math.floor(startTimeMicros / 1_000_000);
    const osSeconds = Math.floor(row.startedAt / 1000);
    if (Math.abs(reportedSeconds - osSeconds) > 1) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `${label}: pid ${pid} reports start ${reportedSeconds}s, the OS reports ${osSeconds}s (${row.lstart})`,
      );
    }
  }
  return { pid, startTimeMicros, lstart: row.lstart, ppid: row.ppid };
}

// ---------------------------------------------------------------------------------------------
// Child fixtures
// ---------------------------------------------------------------------------------------------

/**
 * `stty raw -echo -icanon -opost` is what makes byte accounting meaningful: without it the tty
 * rewrites `\n` as `\r\n`, turns the three ESCs of a screen fixture into `^[`, copies the payload
 * back as echo, and holds a newline-less run in the canonical line buffer -- so every byte
 * comparison below would be an assertion about line discipline instead of about the stream that
 * survived the crash. `exec cat` then makes output equal input exactly, so a sequence offset maps
 * 1:1 onto a payload index.
 *
 * The mode is recorded before `cat` starts and renamed into place, and the spawn path does not
 * resolve until that file has been read (`awaitRawEchoChild`). The wait is not decoration: `SPAWN`
 * is acknowledged as soon as the fork happens, so `stty` runs strictly later, and a write landing in
 * between is echoed or line-buffered by the child's INITIAL mode -- canonical input and echo, which
 * is exactly what inflated every offset below. This mirrors `echo_child` plus `await_raw_child` in
 * `crates/ferryx-ptykeeper/tests/checkpoint_resume_acceptance.rs`, which passes against the same
 * keeper for that reason.
 */
function echoChild() {
  return [
    '/bin/sh',
    '-c',
    // The flags are named after `raw` because what `raw` implies is not identical across platforms
    // (whether it drops `opost` is the difference that matters here), and `awaitRawEchoChild` checks
    // them by name. The leading `:` is the sentinel `KeeperClient.spawn` keys the readiness wait on.
    // The `[ -n ]` guard keeps an unset marker variable from writing `.partial` into the cwd: an
    // unconfigured child then records nothing and the wait below says so, rather than writing
    // outside the fixture.
    `: ${RAW_TTY_SENTINEL}; stty raw -echo -icanon -opost 2>/dev/null; `
      + `if [ -n "$${RAW_TTY_MARKER_ENV}" ]; then `
      + `stty -a > "$${RAW_TTY_MARKER_ENV}.partial" 2>/dev/null `
      + `&& mv "$${RAW_TTY_MARKER_ENV}.partial" "$${RAW_TTY_MARKER_ENV}" 2>/dev/null; fi; `
      + 'exec cat',
  ];
}

/**
 * Blocks until `sessionId`'s echo child has recorded its tty mode, and refuses a mode that is not raw.
 *
 * The signal is the rename, never a timer: the marker exists whole or not at all. Flags are compared
 * as whole tokens because `stty -a` prints them whitespace-separated (Linux one line per class,
 * macOS prefixes `iflags:`/`oflags:` and ends a line with `;`); `includes('-echo')` alone would also
 * match macOS's `-echoprt`/`-echonl` and certify a tty that still echoes.
 */
async function awaitRawEchoChild(sessionId) {
  const marker = rawTtyMarkerPath(sessionId);
  const mode = await until(`the echo child for ${sessionId} recorded its tty mode`, 45_000, async () => {
    try {
      return await readFile(marker, 'utf8');
    } catch {
      return null;
    }
  });
  const flags = mode.split(/\s+/).map((flag) => flag.replace(/;$/, ''));
  for (const required of ['-opost', '-icanon', '-echo']) {
    if (!flags.includes(required)) {
      throw fail(
        FAIL.PROTOCOL,
        `the echo child for ${sessionId} recorded a tty that is not raw (${required} is missing), so `
          + `its output would not be the bytes written and every offset below would count echo bytes `
          + `instead of payload. It recorded:\n${mode}`,
      );
    }
  }
  return marker;
}

/** One marker path per session, so a case running several echo children cannot be misled by one. */
function rawTtyMarkerPath(sessionId) {
  return join(rawTtyDir, `${RAW_TTY_MARKER}.${sessionId}`);
}

/**
 * Alternate screen, a long history run, then a byte-faithful echo.
 *
 * `downtimeDelayMs` makes the child produce its `DOWNTIME` marker on its own after a pause. That is
 * how the downtime bytes are genuinely produced with no daemon alive: the harness kills the daemon
 * while the child is waiting, so the bytes can only have come from the keeper's own pump. Writing the
 * marker from the harness instead would prove less.
 */
function altScreenChild(nonce, fillLines, downtimeDelayMs) {
  const script = [
    // Same tty discipline as `echoChild`, for the same reason: the harness types into this session
    // after startup, and a canonical tty would hold that input in the line buffer and echo it back.
    `: ${RAW_TTY_SENTINEL}; stty raw -echo -icanon -opost 2>/dev/null`,
    `if [ -n "$${RAW_TTY_MARKER_ENV}" ]; then stty -a > "$${RAW_TTY_MARKER_ENV}.partial" 2>/dev/null && mv "$${RAW_TTY_MARKER_ENV}.partial" "$${RAW_TTY_MARKER_ENV}" 2>/dev/null; fi`,
    "printf '\\033[?1049h'",
    `printf 'ALT-ENTER:%s\\n' "${nonce}"`,
    `i=0; while [ "$i" -lt ${fillLines} ]; do printf 'FILL%07d\\n' "$i"; i=$((i+1)); done`,
    `printf 'ALT-EXIT:%s\\n' "${nonce}"`,
    "printf '\\033[?1049l'",
    `printf 'READY:%s\\n' "${nonce}"`,
    `sleep ${Math.max(1, Math.round(downtimeDelayMs / 1000))}`,
    `printf 'DOWNTIME:%s\\n' "${nonce}"`,
    `printf 'DOWNTIME-DONE:%s\\n' "${nonce}"`,
    'exec cat',
  ].join('; ');
  return ['/bin/sh', '-c', script];
}

// ---------------------------------------------------------------------------------------------
// Preflight: what this tree can actually do.
// ---------------------------------------------------------------------------------------------

async function exists(path) {
  if (!path) return false;
  try {
    await stat(path);
    return true;
  } catch {
    return false;
  }
}

async function firstExisting(candidates) {
  for (const candidate of candidates) {
    if (await exists(candidate)) return candidate;
  }
  return null;
}

async function preflight(options, paths) {
  const keeperBinary =
    (await firstExisting([options.keeperBinary])) ??
    (await firstExisting([
      join(paths.repo, 'crates/ferryx-ptykeeper/target/release/ferryx-ptykeeper'),
      join(paths.repo, 'crates/ferryx-ptykeeper/target/debug/ferryx-ptykeeper'),
    ]));
  const daemonBinary =
    (await firstExisting([options.daemonBinary])) ??
    (await firstExisting([
      join(paths.repo, 'src-tauri/target/release/ferryx'),
      join(paths.repo, 'src-tauri/target/release/ferryx-cli'),
      join(paths.repo, 'src-tauri/target/debug/ferryx'),
      join(paths.repo, 'src-tauri/target/debug/ferryx-cli'),
    ]));
  // `ferryx-cli` is its own binary (`src-tauri/Cargo.toml` declares it separately from `ferryx`), and
  // `service rollback` exists only there. Accepting the daemon binary as a fallback would let a case
  // "run the CLI" against something that never parses `service`, so it is resolved on its own.
  const cliBinary =
    (await firstExisting([options.cliBinary])) ??
    (await firstExisting([
      join(paths.repo, 'src-tauri/target/release/ferryx-cli'),
      join(paths.repo, 'src-tauri/target/debug/ferryx-cli'),
    ]));

  const gaps = [];
  if (!keeperBinary) gaps.push(GAP.KEEPER_BINARY_ABSENT);
  if (!daemonBinary) gaps.push(GAP.DAEMON_BINARY_ABSENT);
  // The generation count is decided by `planKeeperGenerations`, which is the only place that knows
  // whether the supplied builds are genuinely distinct generations. Deciding it here from a count
  // would let two copies of one build pass as a cross-generation run.
  if (process.platform === 'win32') gaps.push(GAP.PLATFORM_UNSUPPORTED);
  if (!(await exists('/bin/ps'))) gaps.push(GAP.HELPER_ABSENT);

  return { keeperBinary, daemonBinary, cliBinary, gaps };
}

/**
 * Decides which keeper generations this run can honestly claim to have, before anything starts.
 *
 * The generation identity of a running keeper is the version compiled into it:
 * `main.rs` calls `Keeper::new(env!("CARGO_PKG_VERSION"), ...)` and that string is what
 * `OwnershipQuery` reports per session. Two copies of one build are therefore the SAME generation no
 * matter what they are named on disk, and a "coexistence across generations" case driven by two
 * copies of one build would be measuring one generation against itself while reporting two.
 *
 * So the count is taken from what the caller supplied, with one addition that is stated rather than
 * hidden: when a single keeper build was supplied, a second directory is staged from those same
 * bytes and the run records `identicalBytes: true`. That keeps the case runnable on a developer's
 * machine (where a second keeper build means a second full compile) while making the weaker evidence
 * visible in `preflight`, in the case result and in the note -- never silently equivalent to the
 * release gate, which blocks unless `--remote-generations` names two real builds.
 */
async function planKeeperGenerations({ options, paths, binaries, evidence }) {
  const supplied = options.generations.filter(Boolean);
  if (supplied.length >= 2) {
    const digests = [];
    for (const binary of supplied) digests.push(sha256(await readFile(binary)));
    const distinct = new Set(digests).size;
    if (distinct < 2) {
      // Same bytes twice: refused rather than reported as two generations, because the keeper's own
      // generation string would be identical in both processes.
      await evidence.record({ step: 'keeper-generations', supplied, digests, distinct, synthesized: false });
      return { generations: supplied, synthesized: false, identicalBytes: true, gap: GAP.KEEPER_GENERATIONS_IDENTICAL };
    }
    await evidence.record({ step: 'keeper-generations', supplied, digests, distinct, synthesized: false });
    return { generations: supplied, synthesized: false, identicalBytes: false, gap: null };
  }
  if (supplied.length === 0) {
    return { generations: [], synthesized: false, identicalBytes: false, gap: GAP.KEEPER_GENERATIONS_MISSING };
  }

  // One build: stage a second generation directory from the same bytes, and say so.
  const source = supplied[0];
  const bytes = await readFile(source);
  const digest = sha256(bytes);
  const dir = join(paths.bundleDir, 'keeper-generations', digest.slice(0, 16));
  const staged = join(dir, KEEPER_BINARY_NAME);
  await mkdir(dir, { recursive: true });
  await copyFile(source, staged);
  await chmod(staged, 0o755);
  await evidence.record({
    step: 'keeper-generations',
    supplied,
    synthesized: true,
    identicalBytes: true,
    sourceDigest: digest,
    staged,
    reason:
      'one keeper build was supplied, so the second generation is a copy of the same bytes; the keeper generation string compiled into both is identical, so this measures two keeper processes on one generation, not an upgrade between generations',
  });
  return { generations: [source, staged], synthesized: true, identicalBytes: true, gap: null };
}

// ---------------------------------------------------------------------------------------------
// Process supervision: only children of this process, only inside the owned root.
// ---------------------------------------------------------------------------------------------

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
    liveChildren.set(this, kind);
    // A spawn failure (missing file, no exec bit, wrong architecture) arrives asynchronously as an
    // 'error' event. Left unhandled it kills the process with a stack trace instead of naming the
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
      liveChildren.delete(this);
    });
    // A spawn that never produced a process (`ENOENT`, no exec bit) emits 'error' and then 'close',
    // not 'exit'. Without this the registry would keep a corpse and teardown would try to stop a pid
    // that never existed.
    child.on('close', () => {
      liveChildren.delete(this);
    });
  }

  get pid() {
    return this.child.pid ?? null;
  }

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

/**
 * Keeps what each real process said, byte for byte, for the case that produced it.
 *
 * A failing case in this harness almost always fails on a timeout -- "condition never held within
 * 30000ms" -- and that message alone cannot distinguish a product that never produced output from a
 * product that died, refused a frame, or re-attached at a fenced epoch. The daemon and keeper each
 * write that reason to their own stderr, so the raw streams are captured before teardown. Pass
 * cases stay lean: this is only called for an outcome that is not a pass.
 */
async function captureProcessStreams(ctx, label, processes) {
  const captured = {};
  for (const [key, process] of Object.entries(processes)) {
    if (!process) continue;
    const stdout = await ctx.evidence.captureRaw(join('processes', `${label}-${key}.stdout`), process.stdout ?? '');
    const stderr = await ctx.evidence.captureRaw(join('processes', `${label}-${key}.stderr`), process.stderr ?? '');
    captured[key] = {
      kind: process.kind ?? null,
      pid: process.pid ?? null,
      exit: process.exited ?? null,
      spawnError: process.spawnError ? String(process.spawnError.message ?? process.spawnError) : null,
      stdout,
      stderr,
    };
    await ctx.evidence.record({
      step: 'process-streams',
      label,
      role: key,
      kind: process.kind ?? null,
      pid: process.pid ?? null,
      exit: process.exited ?? null,
      stdoutBytes: stdout.bytes,
      stderrBytes: stderr.bytes,
      stderrTail: (process.stderr ?? '').slice(-2_048),
    });
  }
  return captured;
}

/**
 * The run context: the isolated paths, the real keeper, and whichever daemon generation is current.
 * `killDaemon` and `startDaemon` are the recovery boundary the plan's primary gate is built on.
 */
class Context {
  constructor(options, paths, evidence, binaries) {
    this.options = options;
    this.paths = paths;
    this.evidence = evidence;
    this.binaries = binaries;
    this.keeper = null;
    this.keeperProcess = null;
    // Every keeper this case started, oldest first, so a restart cannot orphan the previous process.
    this.keeperProcesses = [];
    this.daemon = null;
    this.daemonProcess = null;
    // Every daemon generation this case started, oldest first. `shutdown` stops these newest-first
    // before the current slot, so an inherited listener is released in the order it was taken.
    this.daemonProcesses = [];
    this.workspaceId = `phase3-workspace-${randomUUID()}`;
    this.daemonStarts = 0;
  }

  get keeperSocketPath() {
    return this.paths.keeperSocketPath;
  }

  /** The fixture root, so a case can prove a path it is about to use is inside it. */
  get rootDir() {
    return this.paths.root;
  }

  get daemonSocketPath() {
    return join(this.paths.runtimeDir, 'daemon.sock');
  }

  /**
   * Attaches over a connection of its own, returning the stream attachment.
   *
   * `attach` converts the connection that asked into an output-only stream, so attaching on the case's
   * control connection would leave every later `liveSessions`/`write`/`spawn` sending requests into a
   * socket the daemon has stopped reading requests from. Doing it here, once, is why no case has to
   * remember; the caller closes the returned `stream`, which closes this connection with it.
   */
  async attachSession(sessionId, afterSequence) {
    const client = await DaemonClient.connect(this.daemonSocketPath, this.evidence, this.daemon?.protocolVersion);
    try {
      return await client.attach(sessionId, afterSequence);
    } catch (error) {
      client.close();
      throw error;
    }
  }

  env(extra) {
    return childEnv(process.env, {
      FERRYX_RUNTIME_DIR: this.paths.runtimeDir,
      FERRYX_LEGACY_RUNTIME_DIR: this.paths.legacyRuntimeDir,
      FERRYX_DATA_DIR: this.paths.dataDir,
      FERRYX_SESSION_DIR: this.paths.sessionDir,
      FERRYX_KEEPER_DIR: this.paths.keeperDir,
      FERRYX_PTY_KEEPER_PLIST_PATH: this.paths.keeperPlistPath,
      // The keeper install pass writes `$HOME/Library/Logs/Ferryx` once it runs; an inherited real
      // `HOME` would put that outside the fixture.
      HOME: this.paths.homeDir,
      FERRYX_QA_HARNESS: 'phase3-pty-crash-survival',
      ...extra,
    });
  }

  /** Starts the real keeper binary and waits for its socket to accept a connection. */
  async startKeeper() {
    const argv = [this.binaries.keeperBinary, '--socket', this.paths.keeperSocketPath];
    const env = this.env({});
    const child = spawn(argv[0], argv.slice(1), {
      cwd: this.paths.repo,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    this.keeperProcess = new Supervised('keeper', child, argv, env, this.evidence);
    this.keeperProcesses.push(this.keeperProcess);
    await this.evidence.record({ process: 'keeper', step: 'started', pid: child.pid, argv, socket: this.paths.keeperSocketPath });
    this.keeper = await until('keeper socket accepts connections', 30_000, async () => {
      if (this.keeperProcess.spawnError) {
        throw gap(GAP.KEEPER_START_FAILED, `could not execute ${this.binaries.keeperBinary} (${this.keeperProcess.spawnError.code ?? this.keeperProcess.spawnError.message})`);
      }
      if (this.keeperProcess.exited) {
        throw fail(
          FAIL.PROTOCOL,
          `keeper exited before binding ${this.paths.keeperSocketPath}: ${this.keeperProcess.stderr.slice(-2_048)}`,
        );
      }
      try {
        const client = await KeeperClient.connect(this.paths.keeperSocketPath, this.evidence);
        try {
          await client.hello();
          return client;
        } catch {
          // The connection came up but the handshake did not answer. This attempt is over, so its
          // socket is destroyed here instead of being abandoned to the event loop and to the keeper's
          // connection cap.
          client.close();
          return null;
        }
      } catch {
        return null;
      }
    });
    return this.keeper;
  }

  /**
   * Starts the real daemon in the foreground, in this isolated runtime root, and waits for the socket
   * and its handshake. `role` is either `initial` or `successor`, so the evidence says which side of
   * the crash a daemon belonged to.
   */
  async startDaemon(options) {
    const role = options?.role ?? 'initial';
    this.daemonStarts += 1;
    // `binary` lets a case run a *different* build than the default one. That is what makes an app
    // replacement real: the successor must be a distinct executable, not the same bytes renamed.
    // Omitted, this keeps using the resolved default exactly as before.
    const requested = options?.binary ?? this.binaries.daemonBinary;
    // Whatever build the caller asked for, it runs from a bundle: `bundle_keeper_dir` reads the
    // executable's own path with no override, so the daemon can only install its keeper payload --
    // and only then write the routing flag -- from `Contents/MacOS/<exe>`. Doing it here rather than
    // in each case is what makes the whole run exercise the real start pass instead of reporting
    // `PayloadAbsent` once per case. An already-bundled path is returned untouched.
    const binary = await stageDaemonBundle(
      this.paths,
      requested,
      options?.keeperBinary ?? this.binaries.keeperBinary,
    );
    // The executable the current daemon generation runs. `keeperRoutingGap` reads the real layout of
    // this path to name why the keeper start pass could not install its payload, and `requested` is
    // kept beside it so the evidence can say which build it came from.
    this.daemonBinaryPath = binary;
    this.daemonRequestedBinaryPath = requested;
    const argv = [binary, '--daemon'];
    // The real graceful path: a successor is started with `--handover-from <predecessor socket>` and
    // adopts the predecessor's sessions over that socket. `cli.rs` reads this flag before it decides
    // whether to announce readiness, so it is the flag itself that selects the handover path -- no
    // other environment or file turns it on.
    if (options?.handoverFrom) argv.push('--handover-from', options.handoverFrom);
    // No `FERRYX_DAEMON_ROLE` exists in the tree; inventing one would look like configuration and be
    // silently ignored. The role only labels evidence.
    //
    // Ownership mode is an environment policy read at daemon start, not a CLI verb: nothing in the
    // tree accepts `--ownership`, and `FERRYX_KEEPER_OWNERSHIP` is the production surface (see
    // `keeper_startup.rs::OwnershipPolicy::from_env`). It is forwarded to the child, never applied in
    // this process, because the flag is a startup decision.
    //
    // A value the daemon does not accept is still refused here instead of being forwarded: the
    // daemon's own answer to a typo is to log it and keep the persisted flag, so a case that asked for
    // "disabled" would run with routing it never had and the evidence would look like a disable.
    //
    // Omitted, the variable is not set at all: an absent value *is* `unchanged` to the daemon, and
    // forwarding that spelling explicitly would change the environment every other case runs under
    // for no gain.
    let ownership = null;
    if (options?.ownership) {
      ownership = String(options.ownership).trim().toLowerCase();
      if (!OWNERSHIP_POLICY_VALUES.includes(ownership)) {
        throw fail(
          FAIL.USAGE,
          `${KEEPER_OWNERSHIP_ENV}=${JSON.stringify(options.ownership)} is not one the daemon parses; it accepts ${OWNERSHIP_POLICY_VALUES.join('|')}`,
        );
      }
    }
    const env = this.env(ownership ? { [KEEPER_OWNERSHIP_ENV]: ownership } : {});
    const child = spawn(argv[0], argv.slice(1), {
      cwd: this.paths.repo,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    this.daemonProcess = new Supervised(`daemon-${role}-${this.daemonStarts}`, child, argv, env, this.evidence);
    // A case that starts a second daemon (`graceful-app-replacement`, the rollback sequence) makes
    // this slot point at the successor, so the predecessor would be reachable only through whatever
    // the case remembered. Recorded here, in the one place a daemon is spawned.
    this.daemonProcesses.push(this.daemonProcess);
    await this.evidence.record({
      process: `daemon-${role}`,
      step: 'started',
      generation: this.daemonStarts,
      pid: child.pid,
      socket: this.daemonSocketPath,
      requestedBinary: requested,
      executable: binary,
      bundled: binary !== requested,
    });
    const client = await until(`${role} daemon socket accepts a handshake`, 60_000, async () => {
      if (this.daemonProcess.exited) {
        throw fail(
          FAIL.PROTOCOL,
          `${role} daemon exited (${JSON.stringify(this.daemonProcess.exited)}) before binding ${this.daemonSocketPath}; stderr: ${this.daemonProcess.stderr.slice(-2_048)}; stdout: ${this.daemonProcess.stdout.slice(-512)}`,
        );
      }
      try {
        const connected = await DaemonClient.connect(
          this.daemonSocketPath,
          this.evidence,
          options?.protocolVersion,
          options?.negotiateProtocol === true,
        );
        // The socket is shared and its path does not name a process: a predecessor that is still
        // serving (`mixed-direct`, retired by `prepareHandover` but not yet exited) answers a
        // connect to `daemon.sock` just as readily as the child just spawned here. Accepting that
        // answer makes readiness a lie -- the harness then drives the *old* generation and reports
        // its normal `Daemon handover is in progress` refusals as case failures, and a case whose
        // first request is `prepareHandover` retires the wrong daemon.
        //
        // So the handshake's own pid has to match the child: `HandshakeOk.pid` is
        // `std::process::id()` of the serving daemon (`protocol.rs:1594-1596`, read the same way by
        // `client.rs::endpoint_owner_pid` at `:339`). A `cmd.exec()` re-exec keeps its pid
        // (`server.rs:1103`), so a legitimate generation never trips this, and a mismatch means the
        // child is not yet bound: close, and let `until` retry. `null` is the retry signal, not a
        // verdict -- the deadline above still fails the case if the child never binds.
        //
        // A build from before this field existed cannot be attributed either way, so an unreadable
        // pid is treated as before (accept): `--daemon-generations` runs older releases, and refusing
        // them would fail a case over its own protocol revision rather than over the behaviour.
        const servedBy = Number(connected.identity?.pid);
        if (Number.isInteger(servedBy) && servedBy !== child.pid) {
          await this.evidence.record({
            process: `daemon-${role}`,
            step: 'handshake-predecessor',
            generation: this.daemonStarts,
            expectedPid: child.pid,
            servedByPid: servedBy,
          });
          connected.close();
          return null;
        }
        try {
          await connected.call({ type: 'registerWorkspace', workspaceId: this.workspaceId, repoRoot: this.paths.repo }, 'registerWorkspace').catch(() => null);
          return connected;
        } catch (error) {
          // The socket connected but this attempt is over: close it, or the retry loop below leaves one
          // live daemon connection per attempt.
          connected.close();
          throw error;
        }
      } catch (error) {
        if (error instanceof AcceptanceError) throw error;
        return null;
      }
    });
    // Readiness is the completed handshake, not a stdout banner: the handshake is the thing every
    // later request depends on, so it is the only signal that cannot be true while the socket is not.
    await this.evidence.record({
      process: `daemon-${role}`,
      step: 'ready',
      generation: this.daemonStarts,
      pid: child.pid,
      ownership: ownership ? `${KEEPER_OWNERSHIP_ENV}=${ownership}` : 'default',
      identity: client.identity,
      stdoutBanner: this.daemonProcess.stdout.includes('FERRYX_DAEMON_READY'),
    });
    return client;
  }

  /**
   * The crash. SIGKILL, not SIGTERM: the plan's origin is a SIGSEGV, and a graceful shutdown would
   * let the daemon flush state, which is precisely what must not be relied on.
   */
  async killDaemon(signal) {
    if (!this.daemonProcess) return null;
    const pid = this.daemonProcess.pid;
    const outcome = await this.daemonProcess.kill(signal ?? 'SIGKILL');
    this.daemon?.close();
    this.daemon = null;
    await this.evidence.record({ step: 'daemon-killed', pid, signal: signal ?? 'SIGKILL', exit: outcome });
    return outcome;
  }

  async shutdown() {
    this.daemon?.close();
    this.keeper?.close();
    // Newest first: a successor holds the inherited listener and the retired predecessor must not be
    // stopped before the one now serving is gone, or the case would tear down a live socket.
    for (const process of [...this.daemonProcesses].reverse()) {
      await process.stop().catch(() => null);
    }
    this.daemonProcesses = [];
    // Every keeper this case started, newest first. `startKeeper` reassigns `keeperProcess`, so a case
    // that restarts its keeper (`keeper-death-terminates-pty`) would otherwise leave the earlier
    // process running; `shutdown()` is the backstop that has to see it.
    for (const process of [...this.keeperProcesses].reverse()) {
      await process.stop().catch(() => null);
    }
    this.keeperProcesses = [];
    await this.daemonProcess?.stop().catch(() => null);
    await this.keeperProcess?.stop().catch(() => null);
  }

  /** Removes every session the run created, through the keeper's own kill verb. */
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
        // The registry reports the epoch actually in force (`Keeper::ownership` -> `epochs.newest`),
        // so a session this client never wrote to is still killable. Falling back to the tracked map
        // alone would fail every keeper-spawned session.
        await this.keeper.kill(entry.sessionId, entry.epoch);
        killed.push(entry.sessionId);
      } catch {
        await this.evidence.record({ step: 'reap-failed', sessionId: entry.sessionId, epoch: entry.epoch });
      }
    }
    return killed;
  }
}

// ---------------------------------------------------------------------------------------------
// Seam probes: what the tree does, measured against the real binaries instead of assumed.
// ---------------------------------------------------------------------------------------------

/**
 * The decisive seam. Plan Phase 3 accepts only if `kill -9` on the daemon leaves 60 sessions inside
 * the keeper. That is meaningful only if the daemon's PTYs are keeper-owned, so this spawns through
 * the daemon and then asks the keeper whether it holds that session. An empty keeper registry means
 * the daemon still opens masters directly, and every case that depends on it reports
 * `KEEPER_INTEGRATION_ABSENT` instead of passing on a weaker path.
 */
async function probeKeeperIntegration(ctx) {
  // Canonical UUID: the daemon refuses anything else before it looks at the request.
  const sessionId = randomUUID();
  let spawned = false;
  try {
    await ctx.daemon.spawn({ sessionId, workspaceId: ctx.workspaceId, cwd: ctx.paths.repo });
    spawned = true;
    const live = (await ctx.daemon.liveSessions()).find((entry) => entry.backendId === sessionId) ?? null;
    await ctx.evidence.record({
      probe: 'keeper-integration',
      step: 'spawned',
      sessionId,
      daemonChildPid: live?.localChildPid ?? null,
      daemonChildStartTime: live?.localChildStartTime ?? null,
    });
    const owned = await ctx.keeper.ownership();
    const match = owned.find((entry) => entry.sessionId === sessionId) ?? null;
    await ctx.evidence.record({
      probe: 'keeper-integration',
      step: 'ownership',
      sessionId,
      ownedSessionIds: owned.map((entry) => entry.sessionId),
    });
    if (match) {
      // Ownership alone is not routing: the keeper must also be able to serve that session's bytes.
      // `fence` demands a STRICTLY newer epoch than the one already in force, and `OWNERSHIP_LIST`
      // reports the fenced epoch (`Keeper::ownership` reads `epochs.newest`), so attaching at the
      // reported value would be refused with `EPOCH_MISMATCH`. Present the successor epoch instead.
      const attachment = await ctx.keeper.attach(sessionId, match.epoch + 1, 0);
      if (match.pid === null) {
        return { ok: false, code: GAP.IDENTITY_START_TIME_UNPROVEN, detail: GAP.IDENTITY_START_TIME_UNPROVEN };
      }
      return {
        ok: true,
        keeperPid: match.pid,
        keeperOwner: match.owner,
        daemonChildPid: live?.localChildPid ?? null,
        pidAgrees: live?.localChildPid === match.pid,
        attachBytes: attachment.data.length,
      };
    }
    // No entry in a keeper that is reachable and serving: the spawn did not go through it. Which
    // seam is missing decides what the honest gap is, so it is measured rather than asserted.
    return { ok: false, ...(await keeperRoutingGap(ctx)) };
  } catch (error) {
    if (error instanceof DaemonRefusal) {
      return {
        ok: false,
        code: GAP.KEEPER_INTEGRATION_ABSENT,
        detail: `the daemon refused the probe spawn with ${error.code}: ${error.text}`,
      };
    }
    throw error;
  } finally {
    if (spawned) {
      await ctx.daemon?.closeSession(sessionId).catch(() => null);
      await ctx.evidence.record({ probe: 'keeper-integration', step: 'closed', sessionId });
    }
  }
}

/**
 * The reason the daemon cannot install its keeper runtime on this host, or `null` when it can.
 *
 * Two independent gates, both measured instead of guessed: the start pass is compiled only for macOS
 * (`daemon/mod.rs` declares `keeper_startup` behind the cfg, and `daemon/server.rs` gates its one
 * production call site the same way), and even compiled it only resolves its payload when the
 * executable sits in a `Contents/MacOS` bundle (`bundle_keeper_dir`). A case that needs the routing
 * flag has to name whichever gate closed, or it reports a missing flag as if the daemon had ignored
 * one it could have written.
 */
async function keeperStartPassUnavailable(ctx) {
  let surfaces = null;
  try {
    surfaces = (await readSourceFacts(ctx.paths.repo)).surfaces;
  } catch (error) {
    const detail = error instanceof SourceFactError ? `${error.code}: ${error.detail}` : String(error?.message ?? error);
    return {
      code: GAP.KEEPER_INTEGRATION_ABSENT,
      detail: `${GAP.KEEPER_INTEGRATION_ABSENT} (the source surfaces could not be read to name the missing seam: ${detail})`,
    };
  }
  // A start pass that is not compiled cannot run, and nothing else flips the routing flag: the flag
  // is persisted truth written by `activate_new_sessions`, which only `start_production` calls. The
  // source says the whole module is macOS-only, so on any other host the daemon binary has no start
  // pass to refuse with -- it simply has no such code. On macOS this is false and the payload check
  // below is what decides.
  if (surfaces.keeperStartupIsMacosGated && process.platform !== 'darwin') {
    return {
      code: GAP.KEEPER_ROUTING_PLATFORM_GATED,
      detail: `${GAP.KEEPER_ROUTING_PLATFORM_GATED} (host ${process.platform}; keeperStartupIsMacosGated=${surfaces.keeperStartupIsMacosGated}, keeperStartupCallSiteIsMacosGated=${surfaces.keeperStartupCallSiteIsMacosGated})`,
    };
  }
  // Compiled, so the pass runs and may refuse on the payload. Measured from the executable this
  // daemon generation was actually started as, not from a compile-time assumption.
  const executable = ctx.daemonBinaryPath ?? ctx.binaries?.daemonBinary ?? null;
  const payloadDir = keeperPayloadDirFor(executable);
  if (executable && payloadDir === null) {
    return {
      code: GAP.KEEPER_ROUTING_PAYLOAD_ABSENT,
      detail: `${GAP.KEEPER_ROUTING_PAYLOAD_ABSENT} (the daemon runs from ${executable}, whose parent is ${basename(dirname(executable))}, not MacOS)`,
    };
  }
  // The parent directory is named `MacOS`, which is necessary but not sufficient: `bundle_keeper_dir`
  // also requires `../Resources/keeper/ferryx-ptykeeper` to exist. A staged bundle missing only the
  // payload would otherwise be reported as an integration failure -- a product verdict -- when the
  // truth is that the fixture never put the file where the daemon looks.
  if (payloadDir !== null && !(await exists(join(payloadDir, KEEPER_BINARY_NAME)))) {
    return {
      code: GAP.KEEPER_ROUTING_PAYLOAD_ABSENT,
      detail: `${GAP.KEEPER_ROUTING_PAYLOAD_ABSENT} (the daemon runs from ${executable}, but ${join(payloadDir, KEEPER_BINARY_NAME)} does not exist)`,
    };
  }
  return null;
}

/**
 * Which seam is missing when a live keeper is serving but a daemon-spawned session never appears in
 * its registry.
 *
 * The old answer here was a hardcoded sentence about `KeeperClient::spawn` having no call site, which
 * had stopped being true (the routing exists in `terminal/pty.rs` and `keeper_session.rs`) and named
 * the wrong thing to fix. When neither gate above is closed, the routing genuinely failed, and only
 * then is that reported.
 */
async function keeperRoutingGap(ctx) {
  return (
    (await keeperStartPassUnavailable(ctx)) ?? {
      code: GAP.KEEPER_INTEGRATION_ABSENT,
      detail:
        `${GAP.KEEPER_INTEGRATION_ABSENT} (the daemon runs from a bundle that carries its keeper payload ` +
        `(${ctx.daemonBinaryPath ?? 'unknown'}), so the start pass had everything it needed and still ` +
        `installed no keeper runtime; the gap is in the start pass, not in the fixture layout)`,
    }
  );
}

/**
 * Did the daemon generation that is serving now run a keeper start pass? Answered from the daemon's own
 * log, sliced from the offset recorded immediately before that generation was started.
 *
 * This is what separates the two reasons a spawn can come out direct-owned, which a bare
 * "not keeper-owned" assertion blurs into one verdict:
 *   - the start pass ran and the spawn still went direct: the daemon had everything it needed and did
 *     not route, which is a fact about the product;
 *   - no start pass ran for this build at all: the binary contains no keeper code, so no keeper-owned
 *     session can exist and the case's premise is not drivable with the generation the operator
 *     supplied. That is a fact about the fixture's build pair, and reporting it as a survival failure
 *     names the wrong side.
 * `daemon/server.rs` logs `keeper startup pass finished` before it starts listening, so by the time
 * `startDaemon` returns (it waits for a handshake) the line is already on disk. Slicing by offset keeps
 * an earlier generation's pass from answering for this one, which matters here because the log is a
 * single append-only file shared by every daemon the run starts.
 */
function keeperStartPassRan(generationLog) {
  return /keeper startup pass finished/.test(generationLog);
}

/**
 * Does an attached consumer receive output without asking?
 *
 * The measurement is deliberately request-free: after the attach, the harness writes a marker through
 * the same connection and then sends nothing at all. If the keeper originates a frame, it arrives on
 * an idle connection; if it does not, the wait simply runs out. Either way the marker is then read
 * back with `REPLAY`, which distinguishes "no live push" from "the bytes were lost".
 */
async function probeLivePush(ctx) {
  const sessionId = `push-${randomUUID().slice(0, 8)}`;
  const epoch = 1;
  try {
    await ctx.keeper.spawn({ sessionId, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch });
    // `spawn` already fences `epoch`, and `fence` demands a strictly newer value, so the attach
    // presents `epoch + 1`. Presenting `epoch` here would be answered with `EPOCH_MISMATCH`.
    // `liveOutput: true` arms this connection as the session's tap; without it `server.rs` returns
    // no tap at all and the probe would report a false absence.
    const attachment = await ctx.keeper.attach(sessionId, epoch + 1, 0, { liveOutput: true });
    if (!attachment.liveOutput) {
      return {
        push: false,
        code: GAP.LIVE_PUSH_ABSENT,
        detail: `${GAP.LIVE_PUSH_ABSENT}: the keeper refused to arm a tap for the attach`,
        head: null,
      };
    }
    const marker = Buffer.from(`PUSH:${sessionId}\n`);
    await ctx.keeper.write(sessionId, epoch + 1, marker);

    // No request is outstanding here: every reply has been consumed. A frame arriving now can only
    // be one the keeper decided to send, which is exactly the property being measured.
    //
    // Retried across fresh writes: `output_run` fires on the poll tick, so one 3s window can expire
    // simply because the marking write landed late. A single miss would block every dependent case
    // for the rest of the run, turning a timing wobble into a permanent verdict.
    let pushed = null;
    for (let attempt = 0; attempt < 3 && !pushed; attempt += 1) {
      if (attempt > 0) await ctx.keeper.write(sessionId, epoch + 1, Buffer.from(`PUSH:${sessionId}:${attempt}\n`));
      try {
        pushed = await ctx.keeper.nextFrame(`live push ${sessionId}`, 5_000);
      } catch (error) {
        if (!(error instanceof AcceptanceError) || error.code !== FAIL.DEADLINE) throw error;
      }
    }

    const head = await ctx.keeper.head(sessionId);
    const replay = await ctx.keeper.replay(sessionId, attachment.sequenceOffset);
    const replayed = Buffer.concat(replay.chunks.map((chunk) => chunk.data));
    const survived = replayed.includes(marker);
    await ctx.evidence.record({
      probe: 'live-push',
      sessionId,
      pushedFrame: pushed?.name ?? null,
      head,
      attachSequenceOffset: attachment.sequenceOffset,
      polledBytes: replayed.length,
      markerSurvived: survived,
    });
    if (!pushed) {
      return { push: false, code: GAP.LIVE_PUSH_ABSENT, detail: GAP.LIVE_PUSH_ABSENT, pollWorks: survived, head };
    }
    return { push: true, frame: pushed.name, pollWorks: survived, head };
  } finally {
    await ctx.keeper.kill(sessionId).catch(() => ctx.evidence.record({ probe: 'live-push', step: 'kill-failed', sessionId }));
  }
}

// ---------------------------------------------------------------------------------------------
// Cases
//
// `requires` names the probes a case cannot proceed without. The runner evaluates those probes once
// and turns a missing prerequisite into that case's recorded gap, so a missing seam is reported per
// case and never silently downgrades an assertion.
// ---------------------------------------------------------------------------------------------

const CASES = [];

function defineCase(id, title, requires, run) {
  CASES.push({ id, title, requires, run });
}

/**
 * Plan Phase 3, primary gate: 60 live sessions, `kill -9` on the daemon, then 60 reattach, each
 * asserting the original child pid AND start time. Identity, not "a shell exists".
 */
defineCase(
  'identity-60-session-sigkill',
  '60 live sessions survive a daemon SIGKILL with the original child pid and start time',
  ['keeper', 'daemon', 'keeperIntegration', 'ps'],
  async (ctx) => {
    const count = ctx.options.sessions;
    const reserved = count * SESSION_RESERVATION_BYTES;
    // Both caps, because the replay sub-cap binds first: `count * reservation` must fit the smaller
    // of the two, or the spawn that trips it is refused with `REPLAY_BUDGET_EXHAUSTED` and the case
    // reports a keeper failure for a fixture that was too large for the ledger it was pointed at.
    const bindingCap = Math.min(BUDGET_AGGREGATE_BYTES, REPLAY_AGGREGATE_BYTES);
    if (reserved > bindingCap) {
      throw fail(
        FAIL.OVERSIZED_FIXTURE,
        `${count} sessions reserve ${reserved} bytes, over the keeper's ${bindingCap} byte ${bindingCap === REPLAY_AGGREGATE_BYTES ? 'replay sub-cap' : 'aggregate cap'}`,
      );
    }

    const sessions = [];
    // Batched admission: 60 sessions against a 768 MiB budget is 480 MiB, but firing all 60 at once
    // races the budget ledger and produces a meaningless MEMORY_BUDGET_EXHAUSTED.
    for (let index = 0; index < count; index += 8) {
      const batch = [];
      const last = Math.min(index + 8, count);
      for (let offset = index; offset < last; offset += 1) {
        const sessionId = randomUUID();
        batch.push(
          ctx.daemon
            .spawn({ sessionId, workspaceId: ctx.workspaceId, cwd: ctx.paths.repo })
            .then(() => sessions.push({ sessionId, index: offset })),
        );
      }
      await Promise.all(batch);
    }
    if (sessions.length !== count) {
      throw fail(FAIL.PROTOCOL, `spawned ${sessions.length} of ${count} sessions`);
    }

    const live = await until(`all ${count} sessions reported live`, 90_000, async () => {
      const rows = await ctx.daemon.liveSessions();
      return rows.length >= count ? rows : null;
    });
    const byId = new Map(live.map((row) => [row.backendId, row]));
    // The table must be read *after* the sessions exist. A snapshot taken before the spawns cannot
    // contain pids the daemon had not created yet, so "pid absent from the process table" would be a
    // statement about this harness's ordering rather than about process identity. This is the
    // "before the crash" reading, which is exactly the boundary the case is about -- the later ones
    // (`tableDuringOutage`, `tableAfter`) are taken at their own moments.
    const before = await processTable();
    const identityBefore = [];
    for (const session of sessions) {
      const row = byId.get(session.sessionId);
      if (!row) throw fail(FAIL.MISSING_OUTPUT, `${session.sessionId} is absent from describeLiveSessions`);
      const proved = await assertProcessIdentity(
        before,
        row.localChildPid,
        row.localChildStartTime,
        `before ${session.sessionId}`,
        'ms',
      );
      identityBefore.push({ sessionId: session.sessionId, pid: proved.pid, startTimeMicros: proved.startTimeMicros, lstart: proved.lstart });
    }
    await ctx.evidence.record({
      case: 'identity-60-session-sigkill',
      step: 'identity-before-crash',
      count: identityBefore.length,
      identityBefore,
    });

    // The keeper must already hold every session, or "the keeper survived" proves nothing.
    const ownedBefore = await ctx.keeper.ownership();
    const ownedIds = new Set(ownedBefore.map((entry) => entry.sessionId));
    const unowned = sessions.filter((session) => !ownedIds.has(session.sessionId)).map((session) => session.sessionId);
    if (unowned.length) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `${unowned.length} of ${count} sessions are not keeper-owned before the crash: ${unowned.slice(0, 5).join(', ')}`,
      );
    }

    const killedPid = ctx.daemonProcess.pid;
    await ctx.killDaemon('SIGKILL');

    // The daemon is gone; the keeper and its children must not be.
    const ownedAfter = await ctx.keeper.ownership();
    if (ownedAfter.length < count) {
      throw fail(
        FAIL.MISSING_OUTPUT,
        `the keeper holds ${ownedAfter.length} sessions after the daemon died, expected at least ${count}`,
      );
    }
    const keeperById = new Map(ownedAfter.map((entry) => [entry.sessionId, entry]));

    // Identity during the outage: the children are still the same processes, before any successor
    // touches the registry.
    const tableDuringOutage = await processTable();
    for (const prior of identityBefore) {
      const keeperEntry = keeperById.get(prior.sessionId);
      if (!keeperEntry) throw fail(FAIL.MISSING_OUTPUT, `${prior.sessionId} vanished from the keeper after SIGKILL`);
      if (keeperEntry.pid !== prior.pid) {
        throw fail(FAIL.IDENTITY_MISMATCH, `${prior.sessionId}: keeper pid changed ${prior.pid} -> ${keeperEntry.pid}`);
      }
      if (!tableDuringOutage.get(prior.pid)) {
        throw fail(FAIL.IDENTITY_MISMATCH, `${prior.sessionId}: child pid ${prior.pid} is gone after SIGKILL`);
      }
      await assertProcessIdentity(tableDuringOutage, prior.pid, prior.startTimeMicros, `outage ${prior.sessionId}`);
    }

    ctx.daemon = await ctx.startDaemon({ role: 'successor' });
    const attachments = [];
    for (const session of sessions) {
      attachments.push({ sessionId: session.sessionId, attachment: await ctx.attachSession(session.sessionId, null) });
    }

    const after = await ctx.daemon.liveSessions();
    const afterById = new Map(after.map((row) => [row.backendId, row]));
    const tableAfter = await processTable();
    const identityAfter = [];
    for (const prior of identityBefore) {
      const row = afterById.get(prior.sessionId);
      if (!row) throw fail(FAIL.MISSING_OUTPUT, `the successor does not re-list ${prior.sessionId}`);
      if (row.localChildPid !== prior.pid) {
        throw fail(FAIL.IDENTITY_MISMATCH, `${prior.sessionId}: pid changed ${prior.pid} -> ${row.localChildPid} across the crash`);
      }
      if (row.localChildStartTime === null || row.localChildStartTime === undefined) {
        throw gap(GAP.IDENTITY_START_TIME_UNPROVEN, `${prior.sessionId}: the successor reports no localChildStartTime`);
      }
      // The daemon reports `keeper_start_time_ms`, i.e. the keeper's epoch microseconds divided by
      // 1000 (`terminal/service.rs:1119-1123`), so the unit is ms and the conversion below is what
      // makes the two sides comparable. The keeper value itself is epoch microseconds on Linux too:
      // `process_start_time` adds `btime` to the tick count so it is an instant, not an uptime.
      // Comparing the raw numbers would fail on every session; assuming the daemon reported
      // microseconds would fail by a factor of 1000.
      if (startTimeToMicros(row.localChildStartTime, 'ms') !== prior.startTimeMicros) {
        throw fail(
          FAIL.IDENTITY_MISMATCH,
          `${prior.sessionId}: start time changed ${prior.startTimeMicros}us -> ${row.localChildStartTime}ms across the crash`,
        );
      }
      const keeperEntry = keeperById.get(prior.sessionId);
      if (!keeperEntry || keeperEntry.pid !== row.localChildPid) {
        throw fail(
          FAIL.IDENTITY_MISMATCH,
          `${prior.sessionId}: keeper pid ${keeperEntry?.pid ?? 'absent'} disagrees with daemon pid ${row.localChildPid}`,
        );
      }
      const proved = await assertProcessIdentity(
        tableAfter,
        row.localChildPid,
        row.localChildStartTime,
        `after ${prior.sessionId}`,
        'ms',
      );
      const attachment = attachments.find((entry) => entry.sessionId === prior.sessionId).attachment;
      identityAfter.push({
        sessionId: prior.sessionId,
        pid: proved.pid,
        startTimeMicros: proved.startTimeMicros,
        lstart: proved.lstart,
        historyBytes: attachment.history.length,
      });
      await ctx.evidence.record({
        case: 'identity-60-session-sigkill',
        step: 'identity-after-crash',
        sessionId: prior.sessionId,
        pid: proved.pid,
        startTimeMicros: proved.startTimeMicros,
        lstart: proved.lstart,
        historyBytes: attachment.history.length,
      });
    }
    for (const { attachment } of attachments) attachment.stream.close();

    return {
      sessions: count,
      killedDaemonPid: killedPid,
      keeperHeldAfterCrash: ownedAfter.length,
      identityBefore,
      identityAfter,
      note: 'identity is pid plus OS start time, cross-checked between daemon and keeper and against ps',
    };
  },
);

/**
 * Plan Phase 3, no-gap case: downtime inside the replay window. History beyond 512 KiB survives,
 * alternate-screen state survives, downtime output is present exactly once, and interactive input
 * works after the automatic reattach.
 */
defineCase(
  'no-gap-history-altscreen-downtime-input',
  'no-gap recovery: history beyond 512 KiB, alternate screen, downtime output, interactive input',
  ['keeper', 'daemon', 'keeperIntegration'],
  async (ctx) => {
    const nonce = randomUUID();
    const sessionId = `nogap-${nonce.slice(0, 8)}`;
    // 12 bytes per FILL line: 60_000 lines is about 720 KiB, above the 512 KiB gate and below the
    // 8 MiB per-session replay cap, so the window cannot overflow by construction.
    const fillLines = 60_000;
    const historyFloor = fillLines * 12;
    if (historyFloor <= NO_GAP_HISTORY_BYTES) {
      throw fail(FAIL.OVERSIZED_FIXTURE, `fixture ${historyFloor} bytes does not exceed the ${NO_GAP_HISTORY_BYTES} byte gate`);
    }
    if (historyFloor > REPLAY_SESSION_BYTES) {
      throw fail(FAIL.OVERSIZED_FIXTURE, `fixture ${historyFloor} bytes would overflow the ${REPLAY_SESSION_BYTES} replay cap`);
    }

    // Spawned through the keeper, and the daemon then attaches to that same session: this case reads
    // the keeper's authoritative offsets and asserts the daemon-side recovery on the same bytes. The
    // daemon is never asked to spawn a second session with this id.
    const downtimeDelayMs = 12_000;
    const spawn = await ctx.keeper.spawn({
      sessionId,
      argv: altScreenChild(nonce, fillLines, downtimeDelayMs),
      env: [],
      cwd: ctx.paths.repo,
      cols: 200,
      rows: 40,
      epoch: 1,
    });
    if (spawn.processStartTime === null) {
      await ctx.evidence.record({ case: 'no-gap-history-altscreen-downtime-input', warning: 'keeper reported no processStartTime' });
    }

    const ready = Buffer.from(`READY:${nonce}\n`);
    await until('the history fixture reached READY', 180_000, async () => {
      const head = await ctx.keeper.head(sessionId);
      if (head <= historyFloor) return null;
      const replay = await ctx.keeper.replayAll(sessionId, 0);
      if (replay.gap) throw fail(FAIL.BYTE_ACCOUNTING, 'the replay window overflowed before the crash');
      return replay.bytes.includes(ready) ? true : null;
    });

    await ctx.killDaemon('SIGKILL');

    // Downtime output: the child emits its DOWNTIME marker on its own after the pause it started with,
    // and the daemon is dead by then. So those bytes exist only because the keeper kept pumping the
    // master with nobody attached -- which is exactly the property the plan is about.
    const downtime = Buffer.from(`DOWNTIME:${nonce}\n`);
    const done = Buffer.from(`DOWNTIME-DONE:${nonce}\n`);
    await until('the child produced its downtime output with no daemon running', 60_000, async () => {
      const replay = await ctx.keeper.replayAll(sessionId, 0);
      if (replay.gap) throw fail(FAIL.BYTE_ACCOUNTING, 'the replay window overflowed during downtime');
      return replay.bytes.includes(downtime) && replay.bytes.includes(done) ? true : null;
    });

    ctx.daemon = await ctx.startDaemon({ role: 'successor' });
    // The daemon's hub for a keeper session is registered by its startup pass and filled by the replay
    // pump (`terminal/service.rs:323-355` registers, `:358-372` publishes), and both happen after the
    // readiness handshake that `startDaemon` waits on. An attach issued in that window snapshots a hub
    // that exists but holds nothing yet -- and `subscribe_with_screen_mirror`
    // (`terminal/output_hub.rs:1840-1863`) only substitutes a screen seed when the buffer reports
    // retained loss, which a brand-new hub never does, so the payload is genuinely empty. That is not a
    // product defect -- the daemon never promised a populated buffer at the instant its socket accepts
    // -- so the case re-attaches until the daemon has pumped, and asserts the history from there. No
    // input is written in this loop, so nothing can slip past the subscription it ends on.
    let attachment;
    let session;
    await until(
      'the successor serves a non-empty attach for the reattached session',
      30_000,
      async () => {
        const attempt = await ctx.attachSession(sessionId, null);
        if (attempt.gap) {
          attempt.stream.close();
          throw fail(FAIL.GAP_IN_NO_GAP_CASE, `attach reported a gap inside the replay cap: ${JSON.stringify(attempt.gap)}`);
        }
        if (attempt.history.length === 0) {
          attempt.stream.close();
          return null;
        }
        attachment = attempt;
        // Subscribed at the snapshot, before any input is written: a marker cannot slip past unobserved.
        session = new AttachedSession(attempt, ctx.evidence);
        return true;
      },
      // Each attempt is a real attach, which is itself the observation.
      100,
    );

    const history = attachment.history;
    // The >512 KiB history assertion is made against the keeper's replay log, not the daemon's attach
    // payload: `output_hub::DEFAULT_BUFFER_CAPACITY` is itself 512 KiB, so demanding more than that
    // from `AttachOk.history` would be asserting something the daemon cannot physically deliver.
    const keeperLog = (await ctx.keeper.replayAll(sessionId, 0)).bytes;
    if (keeperLog.length <= NO_GAP_HISTORY_BYTES) {
      throw fail(
        FAIL.MISSING_OUTPUT,
        `the keeper retained ${keeperLog.length} bytes, not beyond the ${NO_GAP_HISTORY_BYTES} byte gate`,
      );
    }
    if (history.length === 0) {
      throw fail(FAIL.MISSING_OUTPUT, 'the daemon attach returned no history at all');
    }
    const recovered = keeperLog;
    const altEnter = history.indexOf(Buffer.from('\u001b[?1049h'));
    const altExit = history.indexOf(Buffer.from('\u001b[?1049l'));
    const altEnterLog = recovered.indexOf(Buffer.from('\u001b[?1049h'));
    const altExitLog = recovered.indexOf(Buffer.from('\u001b[?1049l'));
    if (altEnterLog < 0) throw fail(FAIL.MISSING_OUTPUT, 'the alternate-screen enter sequence is absent from the recovered log');
    if (altExitLog < 0) throw fail(FAIL.MISSING_OUTPUT, 'the alternate-screen exit sequence is absent from the recovered log');
    if (altEnterLog > altExitLog) throw fail(FAIL.BYTE_ACCOUNTING, 'the alternate-screen sequences are out of order');

    // Both surfaces are checked: the keeper's log must hold the whole run, and the daemon's attach must
    // hold the tail of it. Neither is allowed to be missing the markers it is expected to carry.
    for (const marker of [`ALT-ENTER:${nonce}`, `ALT-EXIT:${nonce}`, `READY:${nonce}`, `DOWNTIME:${nonce}`]) {
      if (!recovered.includes(Buffer.from(marker))) {
        throw fail(FAIL.MISSING_OUTPUT, `${marker} is absent from the keeper's recovered log`);
      }
    }
    for (const marker of [`READY:${nonce}`, `DOWNTIME:${nonce}`]) {
      if (!history.includes(Buffer.from(marker))) {
        throw fail(FAIL.MISSING_OUTPUT, `${marker} is absent from the daemon attach history`);
      }
    }
    const firstFill = recovered.indexOf(Buffer.from('FILL0000000'));
    const lastFill = recovered.indexOf(Buffer.from(`FILL${String(fillLines - 1).padStart(7, '0')}`));
    if (firstFill < 0 || lastFill < 0) throw fail(FAIL.MISSING_OUTPUT, 'the recovered log is not complete end to end');
    if (firstFill > lastFill) throw fail(FAIL.BYTE_ACCOUNTING, 'the history fill lines are out of order');

    // Exactly once: the child emits the marker a single time, so one occurrence is one delivery and two
    // would be a duplicate from the recovery path.
    const occurrences = recovered.toString('latin1').split(`DOWNTIME:${nonce}`).length - 1;
    if (occurrences !== 1) {
      throw fail(FAIL.DUPLICATE_OUTPUT, `DOWNTIME:${nonce} appears ${occurrences} times in the recovered log`);
    }

    // Interactive input after the reattach. The write goes over a control connection, not the attach
    // stream: the daemon closes an attach stream on any client byte (`server.rs:10542`), so a write
    // here would have ended the reader this case asserts on.
    const ping = Buffer.from(`PING:${nonce}\n`);
    await writeToSession(ctx, sessionId, session, ping, 'interactive echo after reattach', 30_000);

    // A complete escape sequence written after recovery must come back intact: a stuck or stale
    // parser state would swallow or mangle it.
    const colored = Buffer.from(`\u001b[32mGREEN:${nonce}\u001b[0m\n`);
    await writeToSession(
      ctx,
      sessionId,
      session,
      colored,
      'the post-recovery escape sequence renders',
      30_000,
      Buffer.from(`GREEN:${nonce}`),
    );
    await session.waitFor('the post-recovery escape sequence renders', 30_000, (bytes) =>
      bytes.includes(Buffer.from(`GREEN:${nonce}`)),
    );

    return {
      historyBytes: history.length,
      keeperLogBytes: keeperLog.length,
      historySha256: sha256(history),
      keeperLogSha256: sha256(keeperLog),
      altScreenEnterOffset: altEnterLog,
      altScreenExitOffset: altExitLog,
      altScreenSeenInDaemonHistory: altEnter >= 0 && altExit >= 0,
      downtimeMarker: `DOWNTIME:${nonce}`,
      liveBytesAfterReattach: session.bytes.length,
      keeperHeadAtAttach: attachment.endSequence,
      note: 'the >512 KiB gate is asserted on the keeper log; the daemon ring is 512 KiB by design',
    };
  },
);

/**
 * Plan Phase 3, overflow case: produce output past the replay cap, then reattach. `REPLAY_GAP` must be
 * reported and the session must be usable after a reset. The retained suffix is discarded whole, so
 * the resume offset may land mid escape sequence or mid character; this case asserts exactly that and
 * never asserts boundary alignment.
 */
defineCase(
  'overflow-replay-gap-reset',
  'overflow: typed REPLAY_GAP, the retained suffix discarded whole, the session usable after reset',
  ['keeper'],
  async (ctx) => {
    const nonce = randomUUID();
    const sessionId = `ovf-${nonce.slice(0, 8)}`;
    const epoch = 1;
    // `cat` with echo off is the deterministic volume source: it makes output equal input one byte for
    // one, so every offset below is exact. An interactive shell cannot be used here because the 8 MiB
    // of filler would have to be parsed by the shell as a command line, which is neither deterministic
    // nor what this case is about. Interactive usability after a reset is the no-gap case's job.
    const spawn = await ctx.keeper.spawn({ sessionId, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch });
    if (spawn.pid === null) throw fail(FAIL.PROTOCOL, 'the keeper spawned a session with no pid');

    // Volume, not a timeout: the echo child makes output equal input, so the offsets below are exact.
    const total = REPLAY_SESSION_BYTES + 256 * 1024;
    const filler = Buffer.alloc(KEEPER_MAX_WRITE_BYTES - 16, 'x');
    let written = 0;
    // The loop target is `total`, not `total` minus the tail: the tail writes below add only a handful
    // of bytes, so stopping short would leave the head under the cap and the case would wait for an
    // overflow it had not produced.
    while (written < total) {
      const batch = Buffer.concat([Buffer.from(`BLOCK${String(written).padStart(9, '0')}:`), filler]).subarray(
        0,
        KEEPER_MAX_WRITE_BYTES,
      );
      written += await ctx.keeper.write(sessionId, epoch, batch);
    }
    // The tail deliberately splits a CSI sequence and a multi-byte character, so the resume offset is
    // free to land inside either. Rev 5 promises no alignment, and this case does not want one.
    written += await ctx.keeper.write(sessionId, epoch, Buffer.from('\u001b[3', 'latin1'));
    written += await ctx.keeper.write(sessionId, epoch, Buffer.from('1m\u00e4\u00b8', 'latin1'));
    written += await ctx.keeper.write(sessionId, epoch, Buffer.from('\u00ad', 'latin1'));

    const head = await until('the output head passed the replay cap', 180_000, async () => {
      const value = await ctx.keeper.head(sessionId);
      // `written`, not `total`: the tail writes below add bytes past the loop target, and waiting only
      // for `total` would let the head be read while bytes this case wrote were still in flight, so the
      // gap snapshot below would describe a different instant than the attach it is compared against.
      return value >= written ? value : null;
    });

    const owned = (await ctx.keeper.ownership()).find((entry) => entry.sessionId === sessionId);
    if (!owned) throw fail(FAIL.PROTOCOL, `${sessionId} vanished from the keeper registry`);
    // `replayResets` and `replayRetainedBytes` are the keeper's own counters and are recorded here for
    // the evidence trail. `Keeper::ownership` reads them under the log's guard together with `head`,
    // so they describe one instant. The overflow is proved by the attach gap and the head below; the
    // counters are then asserted only for what they actually mean (see the reset check below), because
    // the fill loop above is what overflows the log -- not anything the case does afterwards.
    await ctx.evidence.record({
      case: 'overflow-replay-gap-reset',
      step: 'ownership-counters',
      sessionId,
      head,
      reportedReplayResets: owned.replayResets,
      reportedReplayRetainedBytes: owned.replayRetainedBytes,
    });

    const attachment = await ctx.keeper.attach(sessionId, epoch + 1, 0);
    if (!attachment.gap) throw fail(FAIL.NO_GAP_IN_OVERFLOW_CASE, 'attach after overflow reported no replay gap');
    // `head` was read before this attach, and the child's echo of the last writes can still land in
    // between, so the reset offset is compared as a lower bound rather than for equality: requiring
    // exact equality would fail on a correct keeper whenever one more echoed byte arrived first.
    if (attachment.gap.availableFromSequence < head) {
      throw fail(
        FAIL.BYTE_ACCOUNTING,
        `resume offset ${attachment.gap.availableFromSequence} is behind the head ${head} observed before the attach`,
      );
    }
    // The gap must be the ONE the replay log actually performed: the offset cannot sit beyond the head
    // the keeper reports right now, or the consumer would resume past bytes that exist and silently
    // skip them.
    const headAfterGap = await ctx.keeper.head(sessionId);
    if (attachment.gap.availableFromSequence > headAfterGap) {
      throw fail(
        FAIL.BYTE_ACCOUNTING,
        `resume offset ${attachment.gap.availableFromSequence} is past the head ${headAfterGap}, so bytes after it would be skipped`,
      );
    }
    if (attachment.data.length !== 0) {
      throw fail(
        FAIL.NO_GAP_IN_OVERFLOW_CASE,
        `the retained suffix was not discarded whole: ${attachment.data.length} bytes returned`,
      );
    }
    // The typed refusal itself, from the verb that emits it: `REPLAY` answers a lossy reset with
    // `REPLAY_GAP_EVENT` carrying `code: REPLAY_GAP`, so the code is asserted there rather than on the
    // attach payload, which carries the offsets alone.
    const typed = await ctx.keeper.replayAll(sessionId, 0);
    if (!typed.gap) throw fail(FAIL.NO_GAP_IN_OVERFLOW_CASE, 'replay from 0 after the overflow did not report a gap');
    if (typed.gap.code !== 'REPLAY_GAP') {
      throw fail(
        FAIL.PROTOCOL,
        `the overflow was reported as ${JSON.stringify(typed.gap.code)}, not the typed REPLAY_GAP code`,
      );
    }
    if (typed.gap.availableFromSequence !== attachment.gap.availableFromSequence) {
      throw fail(
        FAIL.BYTE_ACCOUNTING,
        `attach resumed at ${attachment.gap.availableFromSequence} but REPLAY reported ${typed.gap.availableFromSequence}`,
      );
    }
    // The reset is real, not just reported: the keeper counted it. The count is asserted where it can
    // hold. `ReplayLog::push` (`replay.rs:261-272`) increments it only when a push would exceed the
    // capacity, and the fill loop above writes `REPLAY_SESSION_BYTES + 256 KiB` in 65535-byte blocks
    // (`KEEPER_MAX_WRITE_BYTES` minus its 15-byte `BLOCK<n>:` prefix). 128 blocks leave only 128 bytes
    // free, so block 129 is the one push that cannot fit and it is the single overflow -- and it
    // happens *before* the `owned` snapshot above. `attach` and `REPLAY` never reset -- they only
    // consume the pending gap (`take_gap`) -- so demanding a *second* reset across them would be
    // unsatisfiable on a correct keeper, which is exactly what this case used to assert.
    const afterOverflow = (await ctx.keeper.ownership()).find((entry) => entry.sessionId === sessionId);
    if (!afterOverflow) throw fail(FAIL.PROTOCOL, `${sessionId} vanished from the keeper registry after the reset`);
    if (owned.replayResets < 1) {
      throw fail(
        FAIL.BYTE_ACCOUNTING,
        `the keeper reports ${owned.replayResets} resets after ${written} bytes into a ${REPLAY_SESSION_BYTES} byte log, so the overflow was reported but never counted`,
      );
    }
    // Monotonic, not increasing: a second reset would have to come from another overflowing push, and
    // none happened. A decrease would mean the counter describes a different session's log.
    if (afterOverflow.replayResets < owned.replayResets) {
      throw fail(
        FAIL.BYTE_ACCOUNTING,
        `the keeper's reset counter went backwards, ${owned.replayResets} -> ${afterOverflow.replayResets}, across an attach that must not reset`,
      );
    }
    if (afterOverflow.replayRetainedBytes > REPLAY_SESSION_BYTES) {
      throw fail(
        FAIL.BYTE_ACCOUNTING,
        `the keeper retains ${afterOverflow.replayRetainedBytes} bytes, above the ${REPLAY_SESSION_BYTES} byte cap it just enforced`,
      );
    }

    // Usable after the reset: later output must still flow and be retrievable from the resume offset.
    // A session that reports a gap but then goes silent is not usable, so this writes a fresh marker
    // and reads it back from the offset the gap named.
    const resumeAt = attachment.gap.availableFromSequence;
    const later = Buffer.from(`LATER:${nonce}\n`);
    await ctx.keeper.write(sessionId, epoch + 1, later);
    const resumed = await until('output written after the reset is retrievable from the resume offset', 30_000, async () => {
      const replay = await ctx.keeper.replayAll(sessionId, resumeAt);
      if (replay.gap) return null;
      return replay.bytes.includes(later) ? replay.bytes : null;
    });
    // Exactly once: one write is one delivery, so two occurrences would mean the reset path duplicated
    // bytes at the seam. Zero is already caught by the `until` above having failed.
    const occurrences = resumed.toString('latin1').split(`LATER:${nonce}`).length - 1;
    if (occurrences !== 1) {
      throw fail(
        FAIL.DUPLICATE_OUTPUT,
        `LATER:${nonce} appears ${occurrences} times after the reset, so the seam duplicated the write`,
      );
    }
    // No boundary assertion is made about the FIRST byte after the gap: the log's byte at the resume
    // offset is whatever survived the reset, and rev 5 promises no alignment.
    await ctx.evidence.record({
      case: 'overflow-replay-gap-reset',
      writtenBytes: written,
      head,
      reportedGap: attachment.gap,
      typedGapCode: typed.gap.code,
      replayResetsBefore: owned.replayResets,
      replayResetsAfter: afterOverflow.replayResets,
      resumedBytes: resumed.length,
      laterOccurrences: occurrences,
      resumedFirstBytesHex: resumed.subarray(0, 24).toString('hex'),
    });

    return {
      writtenBytes: written,
      head,
      gap: attachment.gap,
      typedGapCode: typed.gap.code,
      replayResetsBefore: owned.replayResets,
      replayResetsAfter: afterOverflow.replayResets,
      bytesRetainedAfterOverflow: attachment.data.length,
      laterOutputDeliveredExactlyOnce: true,
      boundaryAlignmentAsserted: false,
      note: 'rev 5 promises a lossy reset with no alignment, so nothing is asserted about the byte at the resume offset',
    };
  },
);

/**
 * Plan Phase 3, epoch fencing: a stalled predecessor must not read or write after its successor
 * attaches. `EpochFence::verify` gates reads and writes on the newest fenced epoch, so a stale
 * presentation is a typed `EPOCH_MISMATCH` and never a silent read.
 */
defineCase(
  'stale-epoch-fenced',
  'a stale epoch cannot read or write after its successor attaches',
  ['keeper'],
  async (ctx) => {
    const nonce = randomUUID();
    const sessionId = `fence-${nonce.slice(0, 8)}`;
    await ctx.keeper.spawn({ sessionId, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch: 1 });

    const marker = Buffer.from(`FENCE:${nonce}\n`);
    await ctx.keeper.write(sessionId, 1, marker);
    // The write reaches the PTY, and the child's echo arrives whenever the kernel and the reader get to
    // it, so this waits for the bytes rather than assuming they are already in the log.
    await until('the pre-successor marker reached the replay log', 20_000, async () => {
      const replay = await ctx.keeper.replayAll(sessionId, 0);
      return replay.bytes.includes(marker) ? true : null;
    });

    // The successor attaches at a strictly newer epoch: that is what fences the predecessor out.
    const successor = await ctx.keeper.attach(sessionId, 2, 0);
    if (successor.epoch !== 2) {
      throw fail(FAIL.EPOCH_NOT_FENCED, `the keeper fenced epoch ${successor.epoch}, not 2`);
    }

    const refusals = [];
    const expectRefusal = async (label, action) => {
      try {
        await action();
      } catch (error) {
        if (!(error instanceof KeeperRefusal)) throw error;
        if (error.code !== 'EPOCH_MISMATCH') {
          throw fail(FAIL.EPOCH_NOT_FENCED, `${label} was refused with ${error.code}, not EPOCH_MISMATCH`);
        }
        refusals.push({ label, code: error.code, message: error.text });
        return;
      }
      throw fail(FAIL.EPOCH_NOT_FENCED, `${label} was allowed at the stale epoch`);
    };

    // The registry must report the epoch it actually fenced at. `Keeper::ownership` passes
    // `epochs.newest(..)` into `Session::snapshot`, so a session that attached at 2 must read back as
    // 2 -- a registry that reported 0 would let a peer believe a stale epoch is current.
    const fencedState = (await ctx.keeper.ownership()).find((entry) => entry.sessionId === sessionId);
    if (!fencedState) throw fail(FAIL.PROTOCOL, `${sessionId} vanished from the keeper registry`);
    if (fencedState.epoch !== 2) {
      throw fail(
        FAIL.EPOCH_NOT_FENCED,
        `OWNERSHIP_LIST reports epoch ${fencedState.epoch} for a session fenced at 2, so the fence is not observable`,
      );
    }
    // The replay state is read under the same guard as the epoch, so a non-zero head here is the log
    // the successor is resuming from rather than a second, drifted read.
    if (fencedState.sequenceOffset === 0) {
      throw fail(FAIL.BYTE_ACCOUNTING, 'OWNERSHIP_LIST reports no output head, so the replay state is not surfaced');
    }
    if (fencedState.lastActivityMillis === 0) {
      throw fail(
        FAIL.MISSING_OUTPUT,
        'OWNERSHIP_LIST reports no last activity, so a reattach cannot tell a live session from an idle one',
      );
    }

    await expectRefusal('stale write', () => ctx.keeper.write(sessionId, 1, Buffer.from('STALE\n')));
    await expectRefusal('stale attach', () => ctx.keeper.attach(sessionId, 1, 0));
    await expectRefusal('stale detach', () => ctx.keeper.detach(sessionId, 1));
    await expectRefusal('stale kill', () => ctx.keeper.kill(sessionId, 1));

    // The successor is not merely tolerated: it still works.
    const ping = Buffer.from(`SUCCESSOR:${nonce}\n`);
    await ctx.keeper.write(sessionId, 2, ping);
    const replayed = await until('the successor can still write and read', 20_000, async () => {
      const replay = await ctx.keeper.replay(sessionId, successor.sequenceOffset);
      if (replay.gap) return null;
      const bytes = Buffer.concat(replay.chunks.map((chunk) => chunk.data));
      return bytes.includes(ping) ? bytes : null;
    });
    await ctx.evidence.record({
      case: 'stale-epoch-fenced',
      sessionId,
      refusals,
      successorBytes: replayed.length,
      registryEpoch: fencedState.epoch,
      registrySequenceOffset: fencedState.sequenceOffset,
      registryLastActivityMillis: fencedState.lastActivityMillis,
      registryReplayRetainedBytes: fencedState.replayRetainedBytes,
    });

    return {
      fencedAt: 2,
      refusals,
      registryEpoch: fencedState.epoch,
      successorStillWrites: true,
      note: 'the refusal is the typed EPOCH_MISMATCH code, not a timeout or a closed socket',
    };
  },
);

/**
 * Plan Phase 3, detach pump survival: output produced while nobody is attached must still be in the
 * replay log when the next consumer arrives. This is the property that makes a crash recoverable at
 * all -- if detaching stopped the pump, the downtime bytes would be gone regardless of the keeper.
 */
defineCase(
  'detach-output-survives',
  'the keeper keeps pumping while detached: output produced with no consumer replays to the next one',
  ['keeper'],
  async (ctx) => {
    const nonce = randomUUID();
    const sessionId = `detach-${nonce.slice(0, 8)}`;
    const epoch = 1;
    await ctx.keeper.spawn({ sessionId, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch });

    const attached = Buffer.from(`ATTACHED:${nonce}\n`);
    await ctx.keeper.write(sessionId, epoch, attached);
    // The attach snapshot is taken after the write, so the marker must already be in the log or the
    // snapshot would legitimately not contain it. Waiting here keeps that expectation honest.
    await until('the attached marker reached the replay log', 20_000, async () => {
      const replay = await ctx.keeper.replayAll(sessionId, 0);
      return replay.bytes.includes(attached) ? true : null;
    });
    const first = await ctx.keeper.attach(sessionId, epoch + 1, 0);
    if (!first.data.includes(attached)) {
      throw fail(FAIL.MISSING_OUTPUT, 'the first consumer did not see the attached marker');
    }

    // Detach, then produce output with no consumer at all. The keeper owns the master, so it must
    // keep reading it; a pump that stopped on detach would lose every one of these bytes.
    await ctx.keeper.detach(sessionId, epoch + 1);
    const detached = [];
    for (let index = 0; index < 8; index += 1) {
      const bytes = Buffer.from(`DETACHED:${nonce}:${index}\n`);
      // A detach clears the tap but not the fence: the newest epoch is still the detached one, so the
      // write presents it rather than inventing a newer one that `verify` would refuse.
      await ctx.keeper.write(sessionId, epoch + 1, bytes);
      detached.push(bytes);
    }

    const successor = await ctx.keeper.attach(sessionId, epoch + 3, first.sequenceOffset);
    if (successor.gap) throw fail(FAIL.BYTE_ACCOUNTING, `the detached run overflowed: ${JSON.stringify(successor.gap)}`);
    const recovered = successor.data;
    for (const [index, bytes] of detached.entries()) {
      const text = `DETACHED:${nonce}:${index}`;
      const occurrences = recovered.toString('latin1').split(text).length - 1;
      if (occurrences !== 1) {
        throw fail(
          occurrences === 0 ? FAIL.MISSING_OUTPUT : FAIL.DUPLICATE_OUTPUT,
          `${text} appears ${occurrences} times in the detached-run replay`,
        );
      }
    }

    // Counting markers is not enough: a suffix that starts one byte early re-delivers a byte without
    // duplicating any marker, and that is still a corrupted resume. The keeper's own log from the same
    // offset is the authoritative stream, so the attach payload is compared to it byte for byte.
    const authoritative = (await ctx.keeper.replayAll(sessionId, first.sequenceOffset)).bytes;
    if (!recovered.equals(authoritative)) {
      throw fail(
        FAIL.BYTE_ACCOUNTING,
        `the attach payload is not the keeper's log: ${recovered.length} bytes vs ${authoritative.length} authoritative`,
      );
    }

    await ctx.evidence.record({
      case: 'detach-output-survives',
      sessionId,
      firstAttachOffset: first.sequenceOffset,
      successorOffset: successor.sequenceOffset,
      recoveredBytes: recovered.length,
      recoveredSha256: sha256(recovered),
    });
    await ctx.evidence.writeRaw(`detach-${sessionId}.bin`, recovered);

    return {
      detachedMarkers: detached.length,
      recoveredBytes: recovered.length,
      note: 'the keeper read the master with no consumer attached, so the bytes were never lost',
    };
  },
);

/**
 * Plan Phase 3, exactly-once: the bytes between the attach snapshot and the first live frame must be
 * delivered once. Duplicates are the failure mode of every "replay then subscribe" implementation,
 * and they are invisible unless the marker count is asserted.
 */
defineCase(
  'attach-live-exactly-once',
  'a marker produced across the attach boundary is delivered exactly once, never zero and never twice',
  ['keeper', 'livePush'],
  async (ctx) => {
    const nonce = randomUUID();
    const sessionId = `once-${nonce.slice(0, 8)}`;
    const epoch = 1;
    await ctx.keeper.spawn({ sessionId, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch });

    // Enough history that the attach snapshot is non-trivial and its tail is real bytes.
    const prelude = Buffer.from(`.`.repeat(64 * 1024));
    await ctx.keeper.write(sessionId, epoch, prelude);
    // The prelude has to be IN the log before the head is read, or the resume offset below is computed
    // from a head of 0 and the case degenerates into "resume from the beginning".
    await until('the prelude reached the replay log', 20_000, async () => {
      const head = await ctx.keeper.head(sessionId);
      return head >= prelude.length ? head : null;
    });
    const beforeAttach = await ctx.keeper.head(sessionId);

    // Resume in the MIDDLE of the log, which is what a real successor does: it holds everything up to
    // where it last saw output and asks only for the rest. Resuming at 0 would never exercise a
    // mid-stream resume, which is where a duplicate or a dropped byte actually appears.
    const resumeFrom = Math.max(0, beforeAttach - 32 * 1024);
    // The tap is opt-in: this case asserts that live frames continue the snapshot, so the attach has
    // to ask for them. Without the flag the keeper arms no tap and no `OUTPUT` frame is ever sent.
    const attachment = await ctx.keeper.attach(sessionId, epoch + 1, resumeFrom, { liveOutput: true });
    if (!attachment.liveOutput) {
      throw gap(GAP.LIVE_PUSH_ABSENT, `${GAP.LIVE_PUSH_ABSENT}: the keeper refused to arm a tap for the attach`);
    }
    const observed = Buffer.concat([attachment.data]);
    if (observed.length === 0) {
      throw fail(FAIL.MISSING_OUTPUT, `the attach from ${resumeFrom} to ${beforeAttach} carried no snapshot bytes`);
    }

    // Produced at the boundary: part of it may land in the snapshot, part after it. Either way the
    // consumer must see it once, so the assertion is a count over the union of both sources.
    const boundary = Buffer.from(`BOUNDARY:${nonce}\n`);
    await ctx.keeper.write(sessionId, epoch + 1, boundary);

    const live = [];
    const deadline = Date.now() + 20_000;
    while (Date.now() < deadline) {
      let frame;
      try {
        frame = await ctx.keeper.nextFrame(`exactly-once ${sessionId}`, Math.max(250, deadline - Date.now()));
      } catch (error) {
        if (error instanceof AcceptanceError && error.code === FAIL.DEADLINE) break;
        throw error;
      }
      if (frame.kind === KIND.Output && frame.payload?.sessionId === sessionId) {
        live.push({ sequenceOffset: Number(frame.payload.sequenceOffset ?? 0), data: unb64(frame.payload.data) });
        ctx.keeper.consumeFrame();
        const seen = Buffer.concat([observed, ...live.map((entry) => entry.data)]);
        if (seen.includes(boundary)) break;
        continue;
      }
      ctx.keeper.consumeFrame();
    }
    let seen = Buffer.concat([observed, ...live.map((entry) => entry.data)]);
    if (!seen.includes(boundary)) {
      // Not a pass and not a silent skip: the marker is old enough that a correct pump must have
      // delivered it, so this is a real loss.
      throw fail(FAIL.MISSING_OUTPUT, `BOUNDARY:${nonce} never arrived after the attach snapshot`);
    }

    // The complete stream the consumer would have: snapshot plus live frames, de-duplicated by the
    // offset each frame starts at. Overlap between the two is the bug being tested for.
    const covered = attachment.sequenceOffset;
    const overlapsSnapshot = live.filter((entry) => entry.sequenceOffset < covered);
    const occurrencesInSnapshot = observed.toString('latin1').split(`BOUNDARY:${nonce}`).length - 1;
    const occurrencesInLive = live
      .map((entry) => entry.data.toString('latin1'))
      .join('')
      .split(`BOUNDARY:${nonce}`).length - 1;
    const headAfter = await ctx.keeper.head(sessionId);

    // A frame that starts before the snapshot re-delivers bytes the snapshot already carried. That is
    // the duplicate this case exists to catch, so it fails rather than being de-duplicated away.
    if (overlapsSnapshot.length > 0) {
      throw fail(
        FAIL.DUPLICATE_OUTPUT,
        `${overlapsSnapshot.length} live frames start before the snapshot offset ${covered} (first at ${overlapsSnapshot[0].sequenceOffset})`,
      );
    }
    const total = occurrencesInSnapshot + occurrencesInLive;
    if (total !== 1) {
      throw fail(FAIL.DUPLICATE_OUTPUT, `BOUNDARY:${nonce} was delivered ${total} times (snapshot ${occurrencesInSnapshot}, live ${occurrencesInLive})`);
    }
    const contiguous = live.every(
      (entry, index) => index === 0 || live[index - 1].sequenceOffset + live[index - 1].data.length === entry.sequenceOffset,
    );
    if (!contiguous) throw fail(FAIL.BYTE_ACCOUNTING, 'the live frames are not contiguous after the snapshot');
    if (live.length > 0 && live[0].sequenceOffset !== covered) {
      throw fail(FAIL.BYTE_ACCOUNTING, `the first live frame starts at ${live[0].sequenceOffset}, not at the snapshot ${covered}`);
    }

    // The stream the consumer assembles starts at the resume offset, not the snapshot head, so it is
    // compared against the keeper's log from `resumeFrom`. Offsets and marker counts can both look
    // right while a byte is shifted or duplicated, so the bytes themselves are compared.
    const assembled = Buffer.concat([observed, ...live.map((entry) => entry.data)]);
    const authoritativeFromResume = (await ctx.keeper.replayAll(sessionId, resumeFrom)).bytes;
    if (authoritativeFromResume.length < assembled.length) {
      throw fail(
        FAIL.BYTE_ACCOUNTING,
        `the keeper log holds ${authoritativeFromResume.length} bytes from ${resumeFrom}, fewer than the ${assembled.length} assembled`,
      );
    }
    if (!authoritativeFromResume.subarray(0, assembled.length).equals(assembled)) {
      throw fail(
        FAIL.BYTE_ACCOUNTING,
        `the assembled stream diverges from the keeper's log from ${resumeFrom} (${assembled.length} bytes compared)`,
      );
    }

    await ctx.evidence.record({
      case: 'attach-live-exactly-once',
      sessionId,
      headBeforeAttach: beforeAttach,
      snapshotOffset: attachment.sequenceOffset,
      snapshotBytes: attachment.data.length,
      liveFrames: live.length,
      liveBytes: live.reduce((sum, entry) => sum + entry.data.length, 0),
      boundaryInSnapshot: occurrencesInSnapshot,
      boundaryInLive: occurrencesInLive,
      headAfter,
    });

    return {
      snapshotOffset: attachment.sequenceOffset,
      snapshotBytes: attachment.data.length,
      liveFrames: live.length,
      boundaryDelivered: total,
      note: 'the boundary marker is counted across snapshot and live frames together, so zero and two both fail',
    };
  },
);

/**
 * Plan Phase 3, checkpoint across a reset: a checkpoint installed before the overflow must survive it
 * and be offered on the attach that reports the gap. Without it, a reset would throw away the screen
 * state the user was looking at, which is the whole reason the checkpoint exists.
 */
defineCase(
  'checkpoint-survives-overflow',
  'a checkpoint installed before the replay overflow still answers after it',
  ['keeper'],
  async (ctx) => {
    const nonce = randomUUID();
    const sessionId = `ckpt-${nonce.slice(0, 8)}`;
    const epoch = 1;
    await ctx.keeper.spawn({ sessionId, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch });

    const screen = Buffer.from(`\u001b[2J\u001b[HSCREEN:${nonce}\n`, 'latin1');
    await ctx.keeper.write(sessionId, epoch, screen);
    // The screen bytes only reach the replay log once the child has echoed them, so the checkpoint
    // offset is taken from a head that is KNOWN to include them: reading it immediately after the
    // write would install the checkpoint before the bytes it is meant to preserve, and the later
    // `sequenceOffset` assertion would then fail on a correct keeper.
    const at = await until('the screen bytes reached the replay log', 20_000, async () => {
      const head = await ctx.keeper.head(sessionId);
      return head >= screen.length ? head : null;
    });
    const installed = await ctx.keeper.checkpointInstall(sessionId, epoch, `ckpt-${nonce}`, at, screen);
    if (installed.storedBytes !== screen.length) {
      throw fail(FAIL.BYTE_ACCOUNTING, `the checkpoint stored ${installed.storedBytes} of ${screen.length} bytes`);
    }

    // Push the log past the replay cap, which is what discards the retained suffix.
    const total = REPLAY_SESSION_BYTES + 128 * 1024;
    const filler = Buffer.alloc(KEEPER_MAX_WRITE_BYTES - 16, 'y');
    let written = 0;
    while (written < total) {
      const batch = Buffer.concat([Buffer.from(`OVR${String(written).padStart(10, '0')}:`), filler]).subarray(
        0,
        KEEPER_MAX_WRITE_BYTES,
      );
      written += await ctx.keeper.write(sessionId, epoch, batch);
    }
    const head = await until('the log passed the replay cap', 180_000, async () => {
      const value = await ctx.keeper.head(sessionId);
      // The absolute head, not the bytes written since the checkpoint: installing the checkpoint
      // truncated the log to `at`, so `head` is `at` plus everything written after it. Comparing
      // against the loop target is what makes this "the log passed the cap" rather than "some bytes
      // arrived".
      return value >= total ? value : null;
    });

    const queried = await ctx.keeper.checkpointQuery(sessionId);
    if (queried.checkpointId !== `ckpt-${nonce}`) {
      throw fail(FAIL.MISSING_OUTPUT, `the checkpoint did not survive the overflow: ${JSON.stringify(queried)}`);
    }
    if (queried.storedBytes !== screen.length || queried.sequenceOffset !== at) {
      throw fail(
        FAIL.BYTE_ACCOUNTING,
        `the surviving checkpoint moved: ${queried.storedBytes} bytes at ${queried.sequenceOffset}, expected ${screen.length} at ${at}`,
      );
    }

    // The attach that reports the gap must also name the checkpoint, so a consumer can reset the
    // screen from it instead of starting blank.
    const attachment = await ctx.keeper.attach(sessionId, epoch + 1, 0);
    if (!attachment.gap) throw fail(FAIL.NO_GAP_IN_OVERFLOW_CASE, 'attach after overflow reported no gap');
    if (attachment.checkpointId !== `ckpt-${nonce}`) {
      throw fail(
        FAIL.MISSING_OUTPUT,
        `the gap attach reported checkpoint ${JSON.stringify(attachment.checkpointId)}, not the installed one`,
      );
    }

    await ctx.evidence.record({
      case: 'checkpoint-survives-overflow',
      sessionId,
      checkpointId: `ckpt-${nonce}`,
      sequenceOffset: at,
      storedBytes: screen.length,
      headAfterOverflow: head,
      gapOnAttach: attachment.gap,
      checkpointIdOnGapAttach: attachment.checkpointId,
    });

    return {
      checkpointId: `ckpt-${nonce}`,
      sequenceOffset: at,
      storedBytes: screen.length,
      headAfterOverflow: head,
      offeredOnGapAttach: attachment.checkpointId,
    };
  },
);

/**
 * Plan Phase 3, within-cap recovery is lossless: a checkpoint plus the replay suffix must reproduce
 * every byte the session ever produced, exactly once and in order.
 *
 * This is the counterpart to the overflow case. Above the cap the keeper is allowed to lose bytes and
 * says so with a typed gap; BELOW it there is no such licence, so the recovery is held to byte
 * equality. A checkpoint that dropped a byte, or a replay boundary that repeated one, would corrupt a
 * user's screen while every count still looked right, which is why the comparison is against the
 * bytes themselves and not against a total.
 *
 * The fixture mixes plain text, a multi-byte character and escape sequences, and the checkpoint is
 * installed at an offset that splits the escape sequence. Rev 5 promises no alignment at a resume
 * point, so the checkpoint is deliberately allowed to hold a partial sequence: what must survive is
 * the BYTES, not their interpretation.
 */
defineCase(
  'checkpoint-replay-byte-exact',
  'within the replay cap a checkpoint plus the suffix reproduces every byte exactly',
  ['keeper'],
  async (ctx) => {
    const nonce = randomUUID();
    const sessionId = `exact-${nonce.slice(0, 8)}`;
    const epoch = 1;
    await ctx.keeper.spawn({ sessionId, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch });

    // The screen the user was looking at. Latin1 keeps one byte per character so the offsets below are
    // the byte offsets, and the trailing partial CSI is completed by the bytes written after it.
    const screen = Buffer.from(`\u001b[2J\u001b[HSCREEN:${nonce}\nescape:\u001b[3`, 'latin1');
    await ctx.keeper.write(sessionId, epoch, screen);
    // The child has to echo the bytes before they are in the replay log, so the install offset is
    // taken from a head known to include them rather than read straight after the write.
    const at = await until('the screen bytes reached the replay log', 20_000, async () => {
      const head = await ctx.keeper.head(sessionId);
      return head >= screen.length ? head : null;
    });
    const checkpointId = `cp-${nonce}`;
    const installed = await ctx.keeper.checkpointInstall(sessionId, epoch, checkpointId, at, screen);
    if (installed.storedBytes !== screen.length) {
      throw fail(FAIL.BYTE_ACCOUNTING, `the checkpoint stored ${installed.storedBytes} of ${screen.length} bytes`);
    }
    // The checkpoint must be findable through the verb a restarted daemon actually asks, not only
    // through the ack of the install that created it.
    const queried = await ctx.keeper.checkpointQuery(sessionId);
    if (queried.checkpointId !== checkpointId) {
      throw fail(
        FAIL.MISSING_OUTPUT,
        `CHECKPOINT_QUERY reports ${JSON.stringify(queried.checkpointId)}, not the installed ${checkpointId}`,
      );
    }
    if (queried.sequenceOffset !== at || queried.storedBytes !== screen.length) {
      throw fail(
        FAIL.BYTE_ACCOUNTING,
        `CHECKPOINT_QUERY reports offset ${queried.sequenceOffset}/${queried.storedBytes} bytes, not ${at}/${screen.length}`,
      );
    }

    // Bytes written AFTER the checkpoint: these are what the replay suffix has to supply on top of it.
    // A completed escape sequence and a multi-byte character, so a broken boundary is visible.
    const suffix = Buffer.from(`1m\u4e2d\u6587TAIL:${nonce}\n`, 'utf8');
    const before = await ctx.keeper.head(sessionId);
    await ctx.keeper.write(sessionId, epoch, suffix);
    const headAfter = await until('the suffix reached the replay log', 20_000, async () => {
      const head = await ctx.keeper.head(sessionId);
      return head >= before + suffix.length ? head : null;
    });
    // Well inside the cap, so no gap is permitted from here on.
    if (headAfter > REPLAY_SESSION_BYTES) {
      throw fail(FAIL.OVERSIZED_FIXTURE, `this fixture reached ${headAfter} bytes, over the ${REPLAY_SESSION_BYTES} cap`);
    }

    // Fetch the checkpoint back and replay from its offset. The two together are the recovery a daemon
    // performs, so the concatenation is what must equal the original stream.
    const fetched = await ctx.keeper.checkpointFetch(sessionId, epoch, checkpointId, at);
    if (!fetched.complete) throw fail(FAIL.PROTOCOL, 'the checkpoint fetch ended without reporting completion');
    const replay = await ctx.keeper.replayAll(sessionId, at);
    if (replay.gap) {
      throw fail(
        FAIL.GAP_IN_NO_GAP_CASE,
        `replay from the checkpoint offset reported a gap inside the cap: ${JSON.stringify(replay.gap)}`,
      );
    }
    const recovered = Buffer.concat([fetched.bytes, replay.bytes]);
    const expected = Buffer.concat([screen, suffix]);

    if (fetched.bytes.length !== screen.length) {
      throw fail(
        FAIL.BYTE_ACCOUNTING,
        `the checkpoint returned ${fetched.bytes.length} of the ${screen.length} bytes installed`,
      );
    }
    if (!fetched.bytes.equals(screen)) {
      throw fail(FAIL.BYTE_ACCOUNTING, 'the checkpoint body differs from the bytes installed, so the screen would restore wrong');
    }
    if (!recovered.equals(expected)) {
      // Name where they diverge: a mismatch at the seam is a boundary bug, one elsewhere is a
      // dropped or duplicated byte, and the two need different fixes.
      const limit = Math.min(recovered.length, expected.length);
      let diverge = -1;
      for (let index = 0; index < limit; index += 1) {
        if (recovered[index] !== expected[index]) {
          diverge = index;
          break;
        }
      }
      if (diverge < 0) diverge = limit;
      throw fail(
        FAIL.BYTE_ACCOUNTING,
        `recovered ${recovered.length} bytes differ from the ${expected.length} written at offset ${diverge}` +
          ` (checkpoint length ${fetched.bytes.length}, replay offset ${replay.bytes.length ? `after ${at}` : 'empty'});` +
          ` seam is at ${screen.length}`,
      );
    }

    // The session keeps working after the recovery: a fresh marker written now must still flow.
    const later = Buffer.from(`AFTER:${nonce}\n`);
    await ctx.keeper.write(sessionId, epoch, later);
    const laterBytes = await until('the session still produces output after the recovery', 30_000, async () => {
      const page = await ctx.keeper.replayAll(sessionId, headAfter);
      if (page.gap) return null;
      return page.bytes.includes(later) ? page.bytes : null;
    });
    const laterOccurrences = laterBytes.toString('latin1').split(`AFTER:${nonce}`).length - 1;
    if (laterOccurrences !== 1) {
      throw fail(FAIL.DUPLICATE_OUTPUT, `AFTER:${nonce} appears ${laterOccurrences} times after the recovery`);
    }

    await ctx.evidence.record({
      case: 'checkpoint-replay-byte-exact',
      sessionId,
      checkpointId,
      sequenceOffset: at,
      checkpointBytes: fetched.bytes.length,
      replayBytes: replay.bytes.length,
      recoveredSha256: sha256(recovered),
      expectedSha256: sha256(expected),
      headAfter,
    });

    return {
      checkpointBytes: fetched.bytes.length,
      replayBytes: replay.bytes.length,
      totalBytes: recovered.length,
      sha256: sha256(recovered),
      seamInsideEscapeSequence: true,
      note: 'in-cap recovery is compared byte for byte; only an overflow may lose bytes, and then it must say so',
    };
  },
);

/**
 * Plan Phase 3, "Spawn deduplication": a `request_id` keyed spawn is idempotent -- a retried spawn
 * returns the existing session rather than creating a second PTY.
 *
 * This is what makes a daemon retry safe after a lost reply. If it failed here, every retry would
 * leak a live child and the identity the daemon recorded would name a process nobody is holding.
 * The retry is issued over a *second connection*, because that is what a retried RPC actually is:
 * the first connection may be gone with its reply.
 */
defineCase(
  'spawn-request-id-idempotent',
  'a retried spawn with the same request id returns the existing session and opens no second PTY',
  ['keeper'],
  async (ctx) => {
    const nonce = randomUUID();
    const sessionId = `idem-${nonce.slice(0, 8)}`;
    const requestId = randomUUID();
    const epoch = 1;
    const spawnOptions = { sessionId, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch, requestId };

    const first = await ctx.keeper.spawn(spawnOptions);
    if (!Number.isInteger(first.pid) || first.pid <= 0) {
      throw fail(FAIL.IDENTITY_MISMATCH, `the first spawn reported pid ${JSON.stringify(first.pid)}`);
    }
    if (first.processStartTime === null) {
      throw gap(GAP.IDENTITY_START_TIME_UNPROVEN);
    }

    // A retry over a fresh connection, with the same request id.
    const retryClient = await KeeperClient.connect(ctx.keeperSocketPath, ctx.evidence);
    let retried;
    try {
      await retryClient.hello();
      retried = await retryClient.spawn(spawnOptions);
    } finally {
      retryClient.close();
    }
    if (retried.pid !== first.pid) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `the retried spawn opened a second PTY: pid ${first.pid} -> ${retried.pid} for request ${requestId}`,
      );
    }
    if (retried.processStartTime !== first.processStartTime) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `the retried spawn reported start time ${retried.processStartTime}, not ${first.processStartTime}`,
      );
    }

    // A second spawn of the SAME session id under a DIFFERENT request id is the exclusive-admission
    // path: `existing_spawn` answers from the session already installed rather than opening another.
    const otherRequest = await ctx.keeper.spawn({ ...spawnOptions, requestId: randomUUID() });
    if (otherRequest.pid !== first.pid) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `a spawn for the existing session id opened a second PTY: pid ${first.pid} -> ${otherRequest.pid}`,
      );
    }

    // One registry entry, not two.
    const owned = (await ctx.keeper.ownership()).filter((entry) => entry.sessionId === sessionId);
    if (owned.length !== 1) {
      throw fail(FAIL.PROTOCOL, `${sessionId} appears ${owned.length} times in OWNERSHIP_LIST after three spawns`);
    }

    // The retries must not have forked the byte stream either: one write, one echo.
    const marker = Buffer.from(`IDEM:${nonce}\n`);
    await ctx.keeper.write(sessionId, first.epoch, marker);
    const bytes = await until('the single marker reached the replay log', 30_000, async () => {
      const replay = await ctx.keeper.replayAll(sessionId, 0);
      if (replay.gap) return null;
      return replay.bytes.includes(marker) ? replay.bytes : null;
    });
    const occurrences = bytes.toString('latin1').split(`IDEM:${nonce}`).length - 1;
    if (occurrences !== 1) {
      throw fail(
        FAIL.DUPLICATE_OUTPUT,
        `IDEM:${nonce} appears ${occurrences} times, so a retried spawn attached a second child to the same log`,
      );
    }

    await ctx.evidence.record({
      case: 'spawn-request-id-idempotent',
      sessionId,
      requestId,
      pid: first.pid,
      processStartTime: first.processStartTime,
      registryEntries: owned.length,
      markerOccurrences: occurrences,
    });

    return {
      pid: first.pid,
      processStartTime: first.processStartTime,
      registryEntries: owned.length,
      note: 'the retry ran on a second connection, so the identity comes from the keeper registry rather than the first reply',
    };
  },
);

/**
 * Plan Phase 3, chunked checkpoint transport: "a checkpoint larger than the 1 MiB frame cap is
 * chunked, and installation is atomic -- the keeper writes chunks under a temporary id, then commits
 * in one atomic step. A partial upload is discarded."
 *
 * The atomicity half is what this case measures, and it is measurable without an abort verb: while
 * the upload is incomplete, `CHECKPOINT_QUERY` must still report the PREVIOUS commit, and that
 * previous checkpoint must still fetch back byte for byte. If a partial upload were ever visible, a
 * successor restoring mid-upload would load a truncated terminal.
 */
defineCase(
  'chunked-checkpoint-atomic-commit',
  'a chunked upload stays invisible until its commit, then fetches back byte for byte',
  ['keeper'],
  async (ctx) => {
    const nonce = randomUUID();
    const sessionId = `chunk-${nonce.slice(0, 8)}`;
    const epoch = 1;
    await ctx.keeper.spawn({ sessionId, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch });

    // A first, small commit: this is the checkpoint that must survive every later refusal.
    const previousBytes = Buffer.from(`PREVIOUS:${nonce}\n`, 'latin1');
    const previousId = `cp-prev-${nonce}`;
    await ctx.keeper.write(sessionId, epoch, previousBytes);
    const previousAt = await until('the previous checkpoint offset is reachable', 20_000, async () => {
      const head = await ctx.keeper.head(sessionId);
      return head >= previousBytes.length ? head : null;
    });
    const installed = await ctx.keeper.checkpointInstall(sessionId, epoch, previousId, previousAt, previousBytes);
    if (installed.storedBytes !== previousBytes.length) {
      throw fail(FAIL.BYTE_ACCOUNTING, `the previous checkpoint stored ${installed.storedBytes} of ${previousBytes.length} bytes`);
    }
    const previousQuery = await ctx.keeper.checkpointQuery(sessionId);
    if (previousQuery.checkpointId !== previousId) {
      throw fail(FAIL.PROTOCOL, `CHECKPOINT_QUERY reports ${JSON.stringify(previousQuery.checkpointId)}, not ${previousId}`);
    }

    // The chunked upload. 2 MiB in 256 KiB chunks: base64 of one chunk is ~350 KiB, comfortably under
    // the 1 MiB frame cap, while the 2 MiB total could never ride one frame.
    const chunkBytes = 256 * 1024;
    const totalChunks = 8;
    const payload = Buffer.alloc(chunkBytes * totalChunks);
    for (let index = 0; index < payload.length; index += 1) payload[index] = index % 251;
    const uploadId = `cp-big-${nonce}`;
    // A chunked commit truncates replay below the committed offset, so the offset must be strictly
    // newer than the previous commit or `check_stale` refuses the upload before it starts.
    const uploadAt = previousAt + 1;

    const opened = await ctx.keeper.checkpointBegin(sessionId, epoch, uploadId, uploadAt, payload.length);
    if (opened.storedBytes !== 0) {
      throw fail(FAIL.PROTOCOL, `a fresh upload reported ${opened.storedBytes} bytes already stored`);
    }
    if (opened.totalBytes !== payload.length) {
      throw fail(FAIL.PROTOCOL, `the upload reports a declared size of ${opened.totalBytes}, not ${payload.length}`);
    }

    let progressed = 0;
    for (let index = 0; index < totalChunks; index += 1) {
      const chunk = payload.subarray(index * chunkBytes, (index + 1) * chunkBytes);
      const ack = await ctx.keeper.checkpointChunk(sessionId, epoch, uploadId, index, chunk);
      progressed += chunk.length;
      if (ack.storedBytes !== progressed) {
        throw fail(FAIL.BYTE_ACCOUNTING, `chunk ${index} left the upload at ${ack.storedBytes} bytes, expected ${progressed}`);
      }
      if (!ack.complete && index === totalChunks - 1) {
        throw fail(FAIL.PROTOCOL, 'the upload never reported completion after its last chunk');
      }

      // Atomicity, sampled mid-upload: the query verb must still name the PREVIOUS checkpoint. Any
      // other answer means a partially written upload is visible to a reattaching daemon.
      if (index === Math.floor(totalChunks / 2)) {
        const midQuery = await ctx.keeper.checkpointQuery(sessionId);
        if (midQuery.checkpointId !== previousId) {
          throw fail(
            FAIL.ATOMICITY_VIOLATED,
            `mid-upload CHECKPOINT_QUERY reports ${JSON.stringify(midQuery.checkpointId)}, not the committed ${previousId}`,
          );
        }
        if (midQuery.sequenceOffset !== previousAt) {
          throw fail(
            FAIL.ATOMICITY_VIOLATED,
            `mid-upload CHECKPOINT_QUERY reports offset ${midQuery.sequenceOffset}, not the committed ${previousAt}`,
          );
        }
        const midFetch = await ctx.keeper.checkpointFetch(sessionId, epoch, previousId, previousAt);
        if (!midFetch.complete || !midFetch.bytes.equals(previousBytes)) {
          throw fail(
            FAIL.ATOMICITY_VIOLATED,
            'the committed checkpoint does not fetch back unchanged while an upload is open',
          );
        }
      }
    }

    const committed = await ctx.keeper.checkpointCommit(sessionId, epoch, uploadId, uploadAt);
    if (committed.checkpointId !== uploadId) {
      throw fail(FAIL.PROTOCOL, `the commit answered with checkpoint ${JSON.stringify(committed.checkpointId)}, not ${uploadId}`);
    }
    if (committed.storedBytes !== payload.length) {
      throw fail(FAIL.BYTE_ACCOUNTING, `the commit stored ${committed.storedBytes} of ${payload.length} bytes`);
    }

    const afterQuery = await ctx.keeper.checkpointQuery(sessionId);
    if (afterQuery.checkpointId !== uploadId || afterQuery.sequenceOffset !== uploadAt) {
      throw fail(
        FAIL.PROTOCOL,
        `after the commit CHECKPOINT_QUERY reports ${JSON.stringify(afterQuery.checkpointId)}@${afterQuery.sequenceOffset}, not ${uploadId}@${uploadAt}`,
      );
    }

    // The whole 2 MiB must come back exactly: `CHECKPOINT_FETCH` pages at 512 KiB, so this also
    // exercises the continuation arithmetic over a multi-page read.
    const fetched = await ctx.keeper.checkpointFetch(sessionId, epoch, uploadId, uploadAt);
    if (!fetched.complete) throw fail(FAIL.PROTOCOL, 'the chunked checkpoint fetch ended without reporting completion');
    if (fetched.bytes.length !== payload.length) {
      throw fail(FAIL.BYTE_ACCOUNTING, `the chunked checkpoint returned ${fetched.bytes.length} of ${payload.length} bytes`);
    }
    if (!fetched.bytes.equals(payload)) {
      let diverge = -1;
      for (let index = 0; index < payload.length; index += 1) {
        if (fetched.bytes[index] !== payload[index]) {
          diverge = index;
          break;
        }
      }
      throw fail(FAIL.BYTE_ACCOUNTING, `the chunked checkpoint differs from the bytes uploaded at offset ${diverge}`);
    }

    await ctx.evidence.record({
      case: 'chunked-checkpoint-atomic-commit',
      sessionId,
      uploadId,
      previousId,
      uploadAt,
      totalBytes: payload.length,
      chunkBytes,
      chunkCount: totalChunks,
      sha256: sha256(payload),
    });

    return {
      sessionId,
      totalBytes: payload.length,
      chunkCount: totalChunks,
      sha256: sha256(payload),
      previousPreservedUntilCommit: true,
      note: 'the previous commit stayed queryable and fetchable through every intermediate chunk',
    };
  },
);

/**
 * Plan Phase 3, chunked transport: "A partial upload is discarded."
 *
 * There is no abort verb on the wire, so this drives the only partial-upload shape a peer can reach:
 * open an upload, send some chunks, never commit. What must hold is that the session keeps serving
 * its committed checkpoint and its live child -- an abandoned upload may hold budget until the
 * session is retired, but it must never become the recovery point.
 */
defineCase(
  'partial-checkpoint-upload-never-commits',
  'an abandoned chunked upload never becomes the committed checkpoint and leaves the session usable',
  ['keeper'],
  async (ctx) => {
    const nonce = randomUUID();
    const sessionId = `partial-${nonce.slice(0, 8)}`;
    const epoch = 1;
    await ctx.keeper.spawn({ sessionId, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch });

    // A committed checkpoint before the abandoned upload, so "the previous commit survives" and "no
    // commit exists at all" are distinguishable outcomes rather than the same empty answer.
    const committedId = `cp-good-${nonce}`;
    const committedBytes = Buffer.from(`GOOD:${nonce}\n`, 'latin1');
    await ctx.keeper.write(sessionId, epoch, committedBytes);
    const committedAt = await until('the committed checkpoint offset is reachable', 20_000, async () => {
      const head = await ctx.keeper.head(sessionId);
      return head >= committedBytes.length ? head : null;
    });
    await ctx.keeper.checkpointInstall(sessionId, epoch, committedId, committedAt, committedBytes);

    // An upload that is opened, partially filled, and abandoned.
    const abandonedId = `cp-abandoned-${nonce}`;
    const abandonedTotal = 256 * 1024 * 4;
    const abandonedAt = committedAt + 1;
    await ctx.keeper.checkpointBegin(sessionId, epoch, abandonedId, abandonedAt, abandonedTotal);
    await ctx.keeper.checkpointChunk(sessionId, epoch, abandonedId, 0, Buffer.alloc(256 * 1024, 0x61));

    const query = await ctx.keeper.checkpointQuery(sessionId);
    if (query.checkpointId !== committedId) {
      throw fail(
        FAIL.ATOMICITY_VIOLATED,
        `an uncommitted upload became the recovery point: CHECKPOINT_QUERY reports ${JSON.stringify(query.checkpointId)}, not ${committedId}`,
      );
    }
    if (query.storedBytes !== committedBytes.length) {
      throw fail(
        FAIL.ATOMICITY_VIOLATED,
        `CHECKPOINT_QUERY reports ${query.storedBytes} stored bytes, not the committed ${committedBytes.length}`,
      );
    }

    // Committing an incomplete upload must be refused rather than promoted.
    let refusal = null;
    try {
      await ctx.keeper.checkpointCommit(sessionId, epoch, abandonedId, abandonedAt);
    } catch (error) {
      if (!(error instanceof KeeperRefusal)) throw error;
      refusal = { code: error.code, message: error.text };
    }
    if (!refusal) {
      throw fail(FAIL.ATOMICITY_VIOLATED, 'an incomplete upload was committed, so a truncated checkpoint could be restored');
    }
    if (refusal.code !== 'CHECKPOINT_CHUNK_MISMATCH') {
      throw fail(
        FAIL.LIMIT_NOT_TYPED,
        `committing an incomplete upload was refused with ${refusal.code}, not CHECKPOINT_CHUNK_MISMATCH`,
      );
    }

    // The committed checkpoint still fetches, unchanged.
    const fetched = await ctx.keeper.checkpointFetch(sessionId, epoch, committedId, committedAt);
    if (!fetched.complete || !fetched.bytes.equals(committedBytes)) {
      throw fail(FAIL.ATOMICITY_VIOLATED, 'the committed checkpoint changed after an abandoned upload');
    }

    // And the session itself is untouched: it still echoes.
    //
    // Replayed from the committed offset, not from 0: installing a checkpoint truncates the replay log
    // below its offset (`checkpoint.rs::install` -> `ReplayLog::truncate_before`), so from 0 the
    // keeper correctly answers `REPLAY_GAP` forever and polling it could never see the marker. That
    // gap is the checkpoint working as designed, not the session dying.
    const marker = Buffer.from(`ALIVE:${nonce}\n`);
    await ctx.keeper.write(sessionId, epoch, marker);
    await until('the session still echoes after an abandoned upload', 30_000, async () => {
      const replay = await ctx.keeper.replayAll(sessionId, committedAt);
      if (replay.gap) return null;
      return replay.bytes.includes(marker) ? true : null;
    });

    await ctx.evidence.record({
      case: 'partial-checkpoint-upload-never-commits',
      sessionId,
      committedId,
      abandonedId,
      abandonedTotal,
      commitRefusal: refusal,
    });

    return {
      sessionId,
      committedId,
      commitRefusal: refusal.code,
      committedPreserved: true,
      note: 'no abort verb exists on the wire, so discard is asserted as "never becomes the commit", never as an explicit abort',
    };
  },
);

/**
 * Plan Phase 3: "Stale checkpoints are rejected by comparing the incoming offset against the live
 * session's newest committed offset (`STALE_CHECKPOINT`); the keeper never rewinds."
 *
 * A rewind is the failure that silently loses the user's screen: a successor restoring an older
 * offset would replay bytes the terminal has already consumed.
 */
defineCase(
  'stale-checkpoint-refused',
  'a checkpoint at or behind the committed offset is refused with STALE_CHECKPOINT and the newest commit survives',
  ['keeper'],
  async (ctx) => {
    const nonce = randomUUID();
    const sessionId = `stale-${nonce.slice(0, 8)}`;
    const epoch = 1;
    await ctx.keeper.spawn({ sessionId, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch });

    const body = Buffer.from(`FIRST:${nonce}\n`, 'latin1');
    await ctx.keeper.write(sessionId, epoch, body);
    const at = await until('the first checkpoint offset is reachable', 20_000, async () => {
      const head = await ctx.keeper.head(sessionId);
      return head >= body.length ? head : null;
    });
    const firstId = `cp-first-${nonce}`;
    await ctx.keeper.checkpointInstall(sessionId, epoch, firstId, at, body);

    // A newer commit, strictly further along.
    const newerBody = Buffer.from(`SECOND:${nonce}\n`, 'latin1');
    await ctx.keeper.write(sessionId, epoch, newerBody);
    const newerAt = await until('a newer offset exists for the second commit', 20_000, async () => {
      const head = await ctx.keeper.head(sessionId);
      return head > at ? head : null;
    });
    const newerId = `cp-newer-${nonce}`;
    await ctx.keeper.checkpointInstall(sessionId, epoch, newerId, newerAt, newerBody);

    // Equal to, and older than, the committed offset: both are a rewind.
    for (const [label, offset] of [['equal', newerAt], ['older', at]]) {
      let refusal = null;
      try {
        await ctx.keeper.checkpointInstall(sessionId, epoch, `cp-rewind-${label}-${nonce}`, offset, Buffer.from('X', 'latin1'));
      } catch (error) {
        if (!(error instanceof KeeperRefusal)) throw error;
        refusal = error;
      }
      if (!refusal) {
        throw fail(FAIL.ATOMICITY_VIOLATED, `an install at the ${label} offset ${offset} was accepted as a rewind`);
      }
      if (refusal.code !== 'STALE_CHECKPOINT') {
        throw fail(FAIL.LIMIT_NOT_TYPED, `an install at the ${label} offset was refused with ${refusal.code}, not STALE_CHECKPOINT`);
      }
      // `checkpoint.rs::check_stale` reports the committed offset, so the caller learns where the
      // live session actually is instead of guessing.
      if (Number(refusal.offset) !== newerAt) {
        throw fail(
          FAIL.LIMIT_NOT_TYPED,
          `the stale refusal reports offset ${JSON.stringify(refusal.offset)}, not the committed ${newerAt}`,
        );
      }
    }

    // A chunked `begin` at a stale offset is refused too, and charges nothing.
    let beginRefusal = null;
    try {
      await ctx.keeper.checkpointBegin(sessionId, epoch, `cp-begin-stale-${nonce}`, newerAt, 1024);
    } catch (error) {
      if (!(error instanceof KeeperRefusal)) throw error;
      beginRefusal = error;
    }
    if (!beginRefusal || beginRefusal.code !== 'STALE_CHECKPOINT') {
      throw fail(
        FAIL.LIMIT_NOT_TYPED,
        `a chunked begin at the committed offset was answered with ${beginRefusal?.code ?? 'success'}, not STALE_CHECKPOINT`,
      );
    }

    // The newest commit is the one that survives, and it still fetches byte for byte.
    const query = await ctx.keeper.checkpointQuery(sessionId);
    if (query.checkpointId !== newerId || query.sequenceOffset !== newerAt) {
      throw fail(
        FAIL.ATOMICITY_VIOLATED,
        `after the refusals CHECKPOINT_QUERY reports ${JSON.stringify(query.checkpointId)}@${query.sequenceOffset}, not ${newerId}@${newerAt}`,
      );
    }
    const fetched = await ctx.keeper.checkpointFetch(sessionId, epoch, newerId, newerAt);
    if (!fetched.complete || !fetched.bytes.equals(newerBody)) {
      throw fail(FAIL.ATOMICITY_VIOLATED, 'the newest committed checkpoint does not fetch back unchanged after a stale refusal');
    }

    await ctx.evidence.record({
      case: 'stale-checkpoint-refused',
      sessionId,
      firstId,
      newerId,
      firstAt: at,
      newerAt,
      beginRefusal: beginRefusal.code,
    });

    return {
      sessionId,
      newerAt,
      refusals: ['equal', 'older', 'begin'],
      note: 'every refusal is the typed STALE_CHECKPOINT code and carries the committed offset',
    };
  },
);

/**
 * Plan Phase 3: "When a new checkpoint would exceed the 768 MiB aggregate cap, the keeper preserves
 * the previous committed checkpoint and returns a typed `CHECKPOINT_BUDGET_EXHAUSTED`."
 *
 * This drives the per-checkpoint cap, which is the reachable half of that requirement: a declaration
 * over `MAX_CHECKPOINT_BYTES` is refused by `check_per_checkpoint_cap` *before* anything is
 * allocated, so proving it costs nothing and cannot be confused with an out-of-memory failure. The
 * aggregate 768 MiB path needs many live uploads to reach and is reported as unverified instead of
 * being claimed here.
 */
defineCase(
  'checkpoint-cap-refusal-preserves-commit',
  'an over-cap checkpoint declaration is refused typed and leaves the committed checkpoint intact',
  ['keeper'],
  async (ctx) => {
    const nonce = randomUUID();
    const sessionId = `cap-${nonce.slice(0, 8)}`;
    const epoch = 1;
    await ctx.keeper.spawn({ sessionId, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch });

    const body = Buffer.from(`KEEP:${nonce}\n`, 'latin1');
    await ctx.keeper.write(sessionId, epoch, body);
    const at = await until('the kept checkpoint offset is reachable', 20_000, async () => {
      const head = await ctx.keeper.head(sessionId);
      return head >= body.length ? head : null;
    });
    const keptId = `cp-kept-${nonce}`;
    await ctx.keeper.checkpointInstall(sessionId, epoch, keptId, at, body);

    // One byte over the per-checkpoint cap. `begin` refuses on the declaration alone, so no 32 MiB
    // buffer is ever allocated and the refusal cannot be a symptom of low memory.
    const declared = KEEPER_MAX_CHECKPOINT_BYTES + 1;
    let refusal = null;
    try {
      await ctx.keeper.checkpointBegin(sessionId, epoch, `cp-over-${nonce}`, at + 1, declared);
    } catch (error) {
      if (!(error instanceof KeeperRefusal)) throw error;
      refusal = error;
    }
    if (!refusal) {
      throw fail(FAIL.LIMIT_NOT_TYPED, `a declaration of ${declared} bytes was accepted`);
    }
    if (refusal.code !== 'CHECKPOINT_BUDGET_EXHAUSTED') {
      throw fail(
        FAIL.LIMIT_NOT_TYPED,
        `an over-cap declaration was refused with ${refusal.code}, not CHECKPOINT_BUDGET_EXHAUSTED`,
      );
    }

    // The refused declaration must not have disturbed what was already committed: the requirement is
    // that the previous checkpoint is PRESERVED, not merely that the call failed.
    const afterRefusal = await ctx.keeper.checkpointQuery(sessionId);
    if (afterRefusal.checkpointId !== keptId || afterRefusal.sequenceOffset !== at) {
      throw fail(
        FAIL.ATOMICITY_VIOLATED,
        `the over-cap refusal changed the commit to ${JSON.stringify(afterRefusal.checkpointId)}@${afterRefusal.sequenceOffset}, not ${keptId}@${at}`,
      );
    }
    const kept = await ctx.keeper.checkpointFetch(sessionId, epoch, keptId, at);
    if (!kept.complete || !kept.bytes.equals(body)) {
      throw fail(FAIL.ATOMICITY_VIOLATED, 'the checkpoint committed before the over-cap refusal no longer fetches unchanged');
    }

    // The store is not wedged by the refusal: a normal install at a newer offset still lands.
    const recovery = Buffer.from(`RECOVER:${nonce}\n`, 'latin1');
    const recovered = await ctx.keeper.checkpointInstall(sessionId, epoch, `cp-recover-${nonce}`, at + 1, recovery);
    if (recovered.storedBytes !== recovery.length) {
      throw fail(
        FAIL.PROTOCOL,
        `a ${recovery.length} byte install after the refusal stored ${recovered.storedBytes} bytes`,
      );
    }

    await ctx.evidence.record({
      case: 'checkpoint-cap-refusal-preserves-commit',
      sessionId,
      keptId,
      declared,
      refusal: { code: refusal.code, message: refusal.text },
      committedAfterRefusal: afterRefusal.checkpointId,
    });

    return {
      sessionId,
      perCheckpointCap: KEEPER_MAX_CHECKPOINT_BYTES,
      refusal: refusal.code,
      committedPreserved: true,
      note: 'the per-checkpoint cap is proven; the 768 MiB aggregate path is not reachable from one session and stays unverified',
    };
  },
);

/**
 * Plan Phase 3, authoritative discovery: "The keeper is the source of truth for which sessions
 * exist and who owns them. A daemon reconnect names expected sessions; the keeper returns
 * `UNKNOWN_SESSION` for absent ones."
 *
 * The second half matters as much as the first: a refusal must be scoped to the missing session and
 * must not take the connection, or the keeper, down with it. A daemon reconnecting after a crash
 * names every session it believed in, so one stale id must not silence the rest.
 */
defineCase(
  'unknown-session-typed',
  'every session-scoped verb answers UNKNOWN_SESSION for an absent session and the connection keeps serving',
  ['keeper'],
  async (ctx) => {
    const nonce = randomUUID();
    const absent = `absent-${nonce.slice(0, 8)}`;
    const live = `present-${nonce.slice(0, 8)}`;
    const epoch = 1;

    // A real session first, so the refusals below are shown not to disturb a live one.
    await ctx.keeper.spawn({ sessionId: live, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch });
    // The head has to be non-zero before it can be shown to survive: `cat` echoes nothing until it is
    // written to, so a pristine session would read 0 either way and the check would prove nothing.
    const seed = Buffer.from(`SEED:${nonce}\n`);
    await ctx.keeper.write(live, epoch, seed);
    const headBefore = await until('the live session produced output before the refusals', 30_000, async () => {
      const head = await ctx.keeper.head(live);
      return head > 0 ? head : null;
    });

    const probes = [
      ['head', () => ctx.keeper.head(absent)],
      ['attach', () => ctx.keeper.attach(absent, epoch, 0)],
      ['detach', () => ctx.keeper.detach(absent, epoch)],
      ['kill', () => ctx.keeper.kill(absent, epoch)],
      ['checkpoint query', () => ctx.keeper.checkpointQuery(absent)],
      ['checkpoint install', () => ctx.keeper.checkpointInstall(absent, epoch, `cp-${nonce}`, 0, Buffer.alloc(4, 0x61))],
      ['checkpoint begin', () => ctx.keeper.checkpointBegin(absent, epoch, `cp-begin-${nonce}`, 0, 16)],
      ['checkpoint chunk', () => ctx.keeper.checkpointChunk(absent, epoch, `cp-chunk-${nonce}`, 0, Buffer.alloc(4, 0x61))],
      ['checkpoint commit', () => ctx.keeper.checkpointCommit(absent, epoch, `cp-commit-${nonce}`, 0)],
      ['checkpoint fetch', () => ctx.keeper.checkpointFetch(absent, epoch, `cp-fetch-${nonce}`, 0, 10_000)],
    ];

    const refusals = [];
    for (const [label, probe] of probes) {
      let refusal = null;
      try {
        await probe();
      } catch (error) {
        if (!(error instanceof KeeperRefusal)) throw error;
        refusal = error;
      }
      if (!refusal) {
        throw fail(FAIL.PROTOCOL, `${label} on the absent session ${absent} was accepted`);
      }
      if (refusal.code !== 'UNKNOWN_SESSION') {
        throw fail(
          FAIL.LIMIT_NOT_TYPED,
          `${label} on the absent session was refused with ${refusal.code}, not UNKNOWN_SESSION`,
        );
      }
      refusals.push({ verb: label, code: refusal.code });
    }

    // Refusals must not have poisoned the connection or the keeper. The head is monotonic, so it can
    // only have grown or stayed put -- a zero here would mean the session's log was reset by them.
    const headAfter = await ctx.keeper.head(live);
    if (headAfter < headBefore) {
      throw fail(
        FAIL.PROTOCOL,
        `the live session ${live} lost output during the refusals: head ${headBefore} -> ${headAfter}`,
      );
    }
    const marker = Buffer.from(`STILL:${nonce}\n`);
    await ctx.keeper.write(live, epoch, marker);
    await until('the live session still echoes after the refusals', 30_000, async () => {
      const replay = await ctx.keeper.replayAll(live, 0);
      if (replay.gap) return null;
      return replay.bytes.includes(marker) ? true : null;
    });

    await ctx.evidence.record({ case: 'unknown-session-typed', absent, live, refusals, headBefore, headAfter });

    return {
      absent,
      verbsRefused: refusals.length,
      headBefore,
      headAfter,
      liveStillServes: true,
      note: 'each absence is the typed UNKNOWN_SESSION code, and none of them closed the connection',
    };
  },
);

/**
 * Plan Phase 3, keeper service: "`flock`-guarded single instance."
 *
 * A second keeper on the same socket must not bind, must not steal the lock, and must exit without a
 * crash loop. The refusal is only meaningful if the FIRST keeper keeps serving, so both halves are
 * asserted: the second exits 0 naming the held lock, and the first still answers a fresh client with
 * its own pid in the handshake -- which is how the socket is shown to still belong to it.
 */
defineCase(
  'keeper-single-instance-lock',
  'a second keeper on the same socket exits without binding while the first keeps serving',
  ['keeper'],
  async (ctx) => {
    const argv = [ctx.binaries.keeperBinary, '--socket', ctx.keeperSocketPath];
    const env = ctx.env({});
    const second = spawn(argv[0], argv.slice(1), {
      cwd: ctx.paths.repo,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    const supervised = new Supervised('keeper-second', second, argv, env, ctx.evidence);
    let exit;
    try {
      exit = await supervised.waitForExit(30_000);
    } finally {
      await supervised.stop().catch(() => null);
    }
    if (supervised.spawnError) {
      throw fail(
        FAIL.PROTOCOL,
        `could not execute ${ctx.binaries.keeperBinary} a second time (${supervised.spawnError.code ?? supervised.spawnError.message})`,
      );
    }
    if (exit.signal) {
      throw fail(FAIL.PROTOCOL, `the second keeper died on ${exit.signal} instead of exiting on the held lock`);
    }
    // `main.rs` treats a held lock as a clean outcome, not a failure: a supervisor must not
    // restart-loop a keeper that is correctly refusing to duplicate the instance.
    if (exit.code !== 0) {
      throw fail(FAIL.PROTOCOL, `the second keeper exited ${exit.code}, not 0: ${supervised.stderr.slice(-2_048)}`);
    }
    if (!supervised.stderr.includes('another keeper already holds')) {
      throw fail(
        FAIL.MISSING_OUTPUT,
        `the second keeper exited 0 without naming the held lock: ${supervised.stderr.slice(-2_048)}`,
      );
    }

    // The socket must still belong to the FIRST keeper. A second keeper that bound before noticing
    // the lock would answer here under its own pid, and the first keeper would be unreachable.
    const fresh = await KeeperClient.connect(ctx.keeperSocketPath, ctx.evidence);
    let hello;
    try {
      hello = await fresh.hello();
    } finally {
      fresh.close();
    }
    if (Number(hello.pid) !== ctx.keeperProcess.pid) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `the socket now answers as pid ${hello.pid}, but the first keeper is ${ctx.keeperProcess.pid}`,
      );
    }

    // And the first keeper still serves new sessions.
    const sessionId = `lock-${nonce()}`;
    await ctx.keeper.spawn({ sessionId, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch: 1 });
    const marker = Buffer.from(`LOCKED:${nonce()}\n`);
    await ctx.keeper.write(sessionId, 1, marker);
    await until('the surviving keeper still serves a new session', 30_000, async () => {
      const replay = await ctx.keeper.replayAll(sessionId, 0);
      if (replay.gap) return null;
      return replay.bytes.includes(marker) ? true : null;
    });

    await ctx.evidence.record({
      case: 'keeper-single-instance-lock',
      firstKeeperPid: ctx.keeperProcess.pid,
      secondExit: exit,
      secondStderr: supervised.stderr.slice(-2_048),
      handshakePid: Number(hello.pid),
    });

    return {
      firstKeeperPid: ctx.keeperProcess.pid,
      secondExitCode: exit.code,
      lockFile: 'keeper.lock',
      note: 'the refusal is a clean exit 0 naming the held lock, and the socket still answers as the first keeper',
    };
  },
);

/**
 * Plan Phase 3, keeper service limits: "max concurrent connections 8."
 *
 * The ninth peer must be told so in the protocol rather than left waiting or silently dropped:
 * `server.rs::refuse_over_limit` writes one `ERROR` frame carrying `TOO_MANY_CONNECTIONS` under
 * `NO_REQUEST_ID`, then closes. The slot must also come back, or one busy moment would wedge the
 * keeper for good -- so a fresh peer is admitted again once the extra connections close.
 */
defineCase(
  'connection-limit-typed',
  'the ninth concurrent connection is refused with TOO_MANY_CONNECTIONS and a freed slot is reusable',
  ['keeper'],
  async (ctx) => {
    // `ctx.keeper` already holds one slot, so this opens one fewer extra than the cap.
    const extras = KEEPER_MAX_CONNECTIONS - 1;
    const extra = [];
    // Declared outside the `try`: the evidence record after the `finally` reports it, and a `let`
    // inside the block is not in scope there (that ReferenceError, not a keeper refusal, is what this
    // case used to fail with).
    let refusal = null;
    try {
      for (let index = 0; index < extras; index += 1) {
        const client = await KeeperClient.connect(ctx.keeperSocketPath, ctx.evidence);
        try {
          await client.hello();
        } catch (error) {
          // A connected socket whose handshake failed is still one of this keeper's slots; leaving it
          // open would make the cap this case measures wrong by one.
          client.close();
          throw error;
        }
        extra.push(client);
      }
      if (extra.length !== extras) {
        throw fail(FAIL.PROTOCOL, `opened ${extra.length} of the ${extras} extra connections needed to fill the keeper`);
      }

      // The peer over the cap. It is refused before any request, so the harness sends none: the frame
      // arrives unsolicited and carries the all-dash request id.
      const over = await KeeperClient.connect(ctx.keeperSocketPath, ctx.evidence);
      try {
        const frame = await over.nextFrame(`over-limit connection ${KEEPER_MAX_CONNECTIONS + 1}`, 10_000);
        refusal = { name: frame.name, code: frame.payload?.code ?? null, requestId: frame.requestId };
      } finally {
        over.close();
      }
      if (refusal.name !== 'Error') {
        throw fail(FAIL.LIMIT_NOT_TYPED, `the over-limit peer received ${refusal.name}, not an ERROR frame`);
      }
      if (refusal.code !== 'TOO_MANY_CONNECTIONS') {
        throw fail(
          FAIL.LIMIT_NOT_TYPED,
          `the over-limit peer was refused with ${refusal.code}, not TOO_MANY_CONNECTIONS`,
        );
      }
      if (refusal.requestId !== KEEPER_NO_REQUEST_ID) {
        throw fail(
          FAIL.PROTOCOL,
          `the refusal carries request id ${JSON.stringify(refusal.requestId)}, not the no-request placeholder`,
        );
      }
    } finally {
      for (const client of extra) client.close();
    }

    // The slots must return: a fresh peer has to be admitted once the extras are gone.
    const fresh = await until('a freed connection slot is reusable', 15_000, async () => {
      let client = null;
      try {
        client = await KeeperClient.connect(ctx.keeperSocketPath, ctx.evidence);
        const ack = await client.hello();
        return { client, ack };
      } catch {
        // A half-open attempt would hold the very slot this probe is waiting for, so it is closed
        // before the retry instead of being left to the garbage collector.
        client?.close();
        return null;
      }
    });
    try {
      if (Number(fresh.ack.protocolMajor) !== 1) {
        throw fail(FAIL.PROTOCOL, `the reused slot answered protocol major ${fresh.ack.protocolMajor}`);
      }
    } finally {
      fresh.client.close();
    }

    // The original connection was never disturbed by the refusals.
    const sessionId = `conn-${nonce()}`;
    await ctx.keeper.spawn({ sessionId, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch: 1 });
    const marker = Buffer.from(`FULL:${nonce()}\n`);
    await ctx.keeper.write(sessionId, 1, marker);
    await until('the original connection still serves after the refusals', 30_000, async () => {
      const replay = await ctx.keeper.replayAll(sessionId, 0);
      if (replay.gap) return null;
      return replay.bytes.includes(marker) ? true : null;
    });

    await ctx.evidence.record({
      case: 'connection-limit-typed',
      maxConnections: KEEPER_MAX_CONNECTIONS,
      extraConnections: extras,
      refusal,
      slotReused: true,
    });

    return {
      maxConnections: KEEPER_MAX_CONNECTIONS,
      refusalCode: 'TOO_MANY_CONNECTIONS',
      slotReused: true,
      note: 'the refusal is one typed ERROR under the no-request placeholder, and the slot returns when a peer closes',
    };
  },
);

/**
 * Plan Phase 3, keeper service: "directory 0700, socket 0600, owner-only; peer credentials checked."
 *
 * What is asserted here is exactly what the `--socket` path guarantees, because that is the path this
 * harness uses: `main.rs` skips `prepare_dir` for an overridden socket and enforces
 * `require_private_parent` instead (owner, and no group/other write), while `bind_owner_only` always
 * sets `SOCKET_MODE` and verifies the owner. Asserting `DIR_MODE` here would be wrong -- the harness
 * creates that directory, not the keeper, so its mode is the harness's, not the policy's.
 */
defineCase(
  'keeper-endpoint-privacy',
  'the keeper socket is owner-only and its directory cannot be replaced by another user',
  ['keeper'],
  async (ctx) => {
    const uid = typeof process.getuid === 'function' ? process.getuid() : userInfo().uid;
    const socketStat = await stat(ctx.keeperSocketPath);
    if (!socketStat.isSocket()) {
      throw fail(FAIL.PROTOCOL, `${ctx.keeperSocketPath} is not a socket`);
    }
    const socketMode = socketStat.mode & 0o777;
    if (socketMode !== KEEPER_SOCKET_MODE) {
      throw fail(
        FAIL.SAFETY,
        `the keeper socket is mode ${socketMode.toString(8)}, not the owner-only ${KEEPER_SOCKET_MODE.toString(8)}`,
      );
    }
    if (socketStat.uid !== uid) {
      throw fail(FAIL.SAFETY, `the keeper socket is owned by uid ${socketStat.uid}, not ${uid}`);
    }

    // The directory policy that actually applies to a `--socket` override: another user must not be
    // able to unlink the bound socket and substitute their own.
    const dirStat = await stat(ctx.paths.runtimeDir);
    const dirMode = dirStat.mode & 0o777;
    if (dirStat.uid !== uid) {
      throw fail(FAIL.SAFETY, `the keeper runtime directory is owned by uid ${dirStat.uid}, not ${uid}`);
    }
    if ((dirMode & 0o022) !== 0) {
      throw fail(
        FAIL.SAFETY,
        `the keeper runtime directory is mode ${dirMode.toString(8)}, which lets group or other replace the socket`,
      );
    }

    await ctx.evidence.record({
      case: 'keeper-endpoint-privacy',
      socketPath: ctx.keeperSocketPath,
      socketMode: socketMode.toString(8),
      socketOwner: socketStat.uid,
      dirPath: ctx.paths.runtimeDir,
      dirMode: dirMode.toString(8),
      dirOwner: dirStat.uid,
      uid,
    });

    return {
      socketMode: socketMode.toString(8),
      dirMode: dirMode.toString(8),
      note: `socket ${KEEPER_SOCKET_MODE.toString(8)} is asserted exactly; the directory is asserted un-replaceable (no group/other write), and DIR_MODE ${KEEPER_DIR_MODE.toString(8)} applies only to the keeper-owned endpoint this harness deliberately does not use`,
    };
  },
);

/**
 * Plan Phase 3, stated exclusion: "keeper death, host reboot and system-wide resource exhaustion
 * terminate their PTYs; a restarted keeper cannot re-adopt them."
 *
 * That is a LOSS the plan accepts, not a survival property, and the plan asks for it to be "stated as
 * a gap, not silently assumed solved". This case is what turns the statement into a measurement: the
 * child really dies with its keeper, and a restarted keeper really starts empty. A later change that
 * quietly claimed re-adoption would fail here instead of shipping.
 */
defineCase(
  'keeper-death-terminates-pty',
  'keeper death terminates its PTYs, and a restarted keeper starts with an empty registry',
  ['keeper', 'ps'],
  async (ctx) => {
    const sessionId = `deads-${nonce()}`;
    const epoch = 1;
    const spawned = await ctx.keeper.spawn({ sessionId, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch });
    if (!Number.isInteger(spawned.pid) || spawned.pid <= 0) {
      throw fail(FAIL.IDENTITY_MISMATCH, `the spawn reported pid ${JSON.stringify(spawned.pid)}`);
    }
    const before = await processTable();
    if (!before.has(spawned.pid)) {
      throw fail(FAIL.IDENTITY_MISMATCH, `the spawned child ${spawned.pid} is not in the process table`);
    }
    const keeperPid = ctx.keeperProcess.pid;

    // The crash the exclusion is about. `SIGKILL` so the keeper cannot close the masters politely;
    // the child dies because the master side of the PTY is gone, which is the mechanism claimed.
    const exit = await ctx.keeperProcess.kill('SIGKILL');
    if (exit.signal !== 'SIGKILL') {
      throw fail(FAIL.PROTOCOL, `the keeper exit reports ${JSON.stringify(exit)}, not a SIGKILL`);
    }

    // The child must be gone. It is not reaped by this process (it was the keeper's child), so it may
    // linger as a zombie briefly; `ps` still lists zombies, so the wait is for it to leave the table
    // entirely and the assertion below is on absence, not on state.
    let gone = null;
    await until('the PTY child terminated with its keeper', 30_000, async () => {
      const table = await processTable();
      gone = !table.has(spawned.pid);
      return gone ? true : null;
    });
    if (!gone) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `child ${spawned.pid} outlived keeper ${keeperPid}, so the stated exclusion does not hold`,
      );
    }

    // A restarted keeper on the same socket must start empty: a keeper that could re-adopt the session
    // would be claiming a survival the plan explicitly does not promise.
    ctx.keeper.close();
    ctx.keeper = await ctx.startKeeper();
    const after = await ctx.keeper.ownership();
    if (after.some((entry) => entry.sessionId === sessionId)) {
      throw fail(
        FAIL.PROTOCOL,
        `a restarted keeper re-adopted ${sessionId}, which the plan states it cannot do`,
      );
    }
    if (after.length !== 0) {
      throw fail(FAIL.PROTOCOL, `the restarted keeper lists ${after.length} sessions, expected an empty registry`);
    }

    await ctx.evidence.record({
      case: 'keeper-death-terminates-pty',
      sessionId,
      childPid: spawned.pid,
      keeperPid,
      keeperExit: exit,
      restartedKeeperPid: ctx.keeperProcess.pid,
      restartedRegistry: after,
    });

    return {
      childPid: spawned.pid,
      keeperPid,
      restartedRegistrySize: after.length,
      note: 'this is the plan\'s stated exclusion measured, not a survival path: keeper death terminates its PTYs and a restarted keeper starts empty',
    };
  },
);

/**
 * Plan Phase 3, older-keeper compatibility: an older generation keeps holding its own sessions while
 * a newer generation is installed and running beside it.
 *
 * The plan's requirement is not "the new binary parses" -- it is that upgrading does not strand the
 * sessions an older keeper owns. Two generations do not share a registry (each keeper is its own
 * process on its own socket), so the property is: after the newer generation has started and served a
 * session of its own, the older generation still answers for its session with the SAME child pid and
 * start time, and a fresh client can still read that session's bytes.
 *
 * The reattach is done with a new connection, which is what an app restart actually is: a successor
 * process dialling the keeper that already owns the sessions.
 */
defineCase(
  'multi-generation-compat',
  'an older keeper generation keeps its sessions while a newer generation runs beside it',
  ['keeper', 'ps'],
  async (ctx) => {
    const generations = ctx.binaries.generations;
    if (generations.length < 2) {
      throw gap(GAP.KEEPER_GENERATIONS_MISSING, `${GAP.KEEPER_GENERATIONS_MISSING}: got ${generations.length}`);
    }
    // What this run can honestly claim about the two "generations". `planKeeperGenerations` stages a
    // second copy when only one build was supplied, and that copy compiles to the same generation
    // string, so the case then measures two keeper processes on ONE generation -- coexistence, but not
    // an upgrade between generations. The distinction is carried in the result and the evidence rather
    // than dropped, because the release gate is the path that supplies two real builds and this note
    // is the only thing that tells the two runs apart afterwards.
    const plan = ctx.binaries.generationPlan ?? { synthesized: false, identicalBytes: false };
    const caveat = plan.identicalBytes
      ? plan.synthesized
        ? 'a second generation directory was staged from the one supplied keeper build, so both processes run the same compiled generation string: this proves two keepers coexist, not that an upgrade preserves sessions'
        : 'the supplied keeper builds are the same bytes, so both processes run the same compiled generation string'
      : null;

    // One entry per generation: the started process, its socket, its live client and its session. They
    // are all kept alive together, because the property is about coexistence, not about each binary
    // working alone.
    //
    // Each generation gets its own runtime directory, and that is a correctness requirement rather
    // than tidiness. `ferryx-ptykeeper` derives its single-instance lock from the *parent of* its
    // `--socket` path: `main.rs` builds `Endpoint::new(runtime_dir, uid)` with `socket.parent()` as the
    // runtime dir, and `endpoint.rs` sets `lock: dir.join(LOCK_NAME)`. Two generations pointed at
    // `${runtimeDir}/keeper-gen<N>.sock` therefore contend on ONE `${runtimeDir}/keeper.lock` -- which
    // this case's own keeper (`ctx.startKeeper`) already holds, so generation 0 exited immediately
    // with "another keeper already holds .../keeper.lock; exiting". The coexistence property this
    // case asserts is about two keepers serving side by side, which the lock makes impossible on a
    // shared directory; the case's comment already says each keeper is its own process on its own
    // socket, so separate directories are what it always meant.
    //
    // Kept directly under the run root rather than under a `generations/` parent: a UDS path is capped
    // at 103 bytes (`endpoint::MAX_SOCKET_PATH_BYTES`), and the run root under `$TMPDIR` already spends
    // most of that budget on macOS (a `/var/folders/.../T/ferryx-phase3-XXXXXX` root alone is ~60
    // bytes, which is why the root is taken under `/tmp` there). Every extra path segment is taken
    // straight out of the headroom before a typed refusal.
    const running = [];
    try {
      for (const [index, binary] of generations.entries()) {
        const runtimeDir = join(ctx.paths.root, `gen${index}`);
        const socketPath = join(runtimeDir, KEEPER_SOCKET_NAME);
        if (Buffer.byteLength(socketPath) > MAX_SOCKET_PATH_BYTES) {
          throw fail(FAIL.SAFETY, `generation socket path ${socketPath} exceeds ${MAX_SOCKET_PATH_BYTES} bytes`);
        }
        // Created before the spawn, and without group/other write: a `--socket` override requires its
        // parent to already exist and to be un-replaceable (`main.rs::require_private_parent`), so the
        // keeper refuses a directory this harness had not made or had left world-writable.
        await mkdir(runtimeDir, { recursive: true, mode: KEEPER_DIR_MODE });
        await chmod(runtimeDir, KEEPER_DIR_MODE);
        const argv = [binary, '--socket', socketPath];
        // The runtime dir is forwarded too, so nothing in this keeper can fall back to a path shared
        // with the case's own keeper or with the other generation.
        const env = ctx.env({ FERRYX_RUNTIME_DIR: runtimeDir });
        const child = spawn(argv[0], argv.slice(1), { cwd: ctx.paths.repo, env, stdio: ['ignore', 'pipe', 'pipe'] });
        const supervised = new Supervised(`keeper-gen${index}`, child, argv, env, ctx.evidence);
        running.push({ index, binary, runtimeDir, socketPath, supervised, client: null, session: null });

        const client = await until(`generation ${index} answers a handshake`, 30_000, async () => {
          if (supervised.exited) {
            throw fail(FAIL.PROTOCOL, `generation ${index} exited before binding: ${supervised.stderr.slice(-2_048)}`);
          }
          try {
            const connected = await KeeperClient.connect(socketPath, ctx.evidence);
            try {
              await connected.hello();
              return connected;
            } catch {
              connected.close();
              return null;
            }
          } catch {
            return null;
          }
        });
        const sessionId = `gen${index}-${randomUUID().slice(0, 8)}`;
        const spawned = await client.spawn({ sessionId, argv: echoChild(), env: [], cwd: ctx.paths.repo, epoch: 1 });
        if (spawned.pid === null) throw fail(FAIL.PROTOCOL, `generation ${index} spawned a session with no pid`);
        const marker = Buffer.from(`GEN${index}:${nonce()}\n`);
        await client.write(sessionId, 1, marker);
        // The marker travels through the PTY and returns on the reader's schedule, so the replay is
        // polled until it carries the bytes instead of read once and trusted.
        await until(`generation ${index} replays its own marker`, 20_000, async () => {
          const replay = await client.replayAll(sessionId, 0);
          return replay.bytes.includes(marker) ? true : null;
        });
        running[index].client = client;
        running[index].session = { sessionId, marker, pid: spawned.pid, startTime: spawned.processStartTime };
        // The generation string this keeper compiled in. Read back from the keeper's own registry row
        // rather than assumed, because that string is the identity the caveat above talks about.
        const ownedHere = await client.ownership();
        running[index].reportedGeneration = ownedHere.find((entry) => entry.sessionId === sessionId)?.generation ?? null;
      }

      // Both generations are up. Now prove the OLDER one still owns its session, with a brand-new
      // connection -- the same thing a restarted app does.
      const oldest = running[0];
      const table = await processTable();
      const before = await assertProcessIdentity(
        table,
        oldest.session.pid,
        oldest.session.startTime,
        `generation 0 before the reattach`,
        'us',
      );

      const reattached = await KeeperClient.connect(oldest.socketPath, ctx.evidence);
      try {
        await reattached.hello();
        const owned = await reattached.ownership();
        const still = owned.find((entry) => entry.sessionId === oldest.session.sessionId);
        if (!still) {
          throw fail(
            FAIL.MISSING_OUTPUT,
            `generation 0 no longer lists ${oldest.session.sessionId} while generation 1 is running`,
          );
        }
        if (still.pid !== oldest.session.pid) {
          throw fail(
            FAIL.IDENTITY_MISMATCH,
            `generation 0 pid changed ${oldest.session.pid} -> ${still.pid} after the newer generation started`,
          );
        }
        // The bytes an app restart must still be able to show: the marker written before the newer
        // generation existed.
        const replay = await reattached.replayAll(oldest.session.sessionId, 0);
        if (replay.gap) throw fail(FAIL.BYTE_ACCOUNTING, `generation 0 replayed a gap: ${JSON.stringify(replay.gap)}`);
        if (!replay.bytes.includes(oldest.session.marker)) {
          throw fail(FAIL.MISSING_OUTPUT, 'generation 0 lost the marker written before the newer generation started');
        }
        const after = await assertProcessIdentity(
          await processTable(),
          still.pid,
          still.pid === before.pid ? oldest.session.startTime : null,
          `generation 0 after the reattach`,
          'us',
        );
        if (after.startTimeMicros !== before.startTimeMicros) {
          throw fail(
            FAIL.IDENTITY_MISMATCH,
            `generation 0 start time changed ${before.startTimeMicros}us -> ${after.startTimeMicros}us`,
          );
        }

        const rows = running.map((entry) => ({
          index: entry.index,
          binary: entry.binary,
          // Recorded so evidence shows the isolation this case depends on, not just the socket paths.
          runtimeDir: entry.runtimeDir,
          socketPath: entry.socketPath,
          sessionId: entry.session.sessionId,
          pid: entry.session.pid,
          processStartTime: entry.session.startTime,
          // The generation string the keeper itself reports, which is what "a generation" means to
          // retention and to the daemon. Recorded so the caveat above is checkable from evidence and
          // not only from this harness's own bookkeeping.
          keeperGeneration: entry.reportedGeneration ?? null,
        }));
        await ctx.evidence.record({
          case: 'multi-generation-compat',
          generations: rows,
          reattachedGeneration: 0,
          reattachMarkerSurvived: true,
          identityMicros: after.startTimeMicros,
          identicalBytes: plan.identicalBytes === true,
          synthesized: plan.synthesized === true,
          caveat,
        });
        return {
          generations: rows,
          coexisted: running.length,
          reattachedWithoutRestart: true,
          identicalBytes: plan.identicalBytes === true,
          synthesized: plan.synthesized === true,
          note: caveat
            ? `the older generation kept its session, its pid and its bytes while the newer process ran; ${caveat}`
            : 'the older generation kept its session, its pid and its bytes while the newer generation ran',
          caveat,
        };
      } finally {
        reattached.close();
      }
    } finally {
      // Every session is killed through the keeper that owns it, then every generation is stopped. A
      // keeper that outlives the case would hold PTY masters and leak them into the next one.
      for (const entry of running) {
        const owned = entry.session;
        if (!owned) continue;
        try {
          await entry.client.kill(owned.sessionId, 1);
        } catch {
          await ctx.evidence.record({ step: 'generation-kill-failed', index: entry.index, sessionId: owned.sessionId });
        }
        entry.client.close();
      }
      for (const entry of running) {
        await entry.supervised.stop().catch(() => null);
      }
    }
  },
);

/** Small nonce helper, so the generation case reads cleanly. */
function nonce() {
  return randomUUID().slice(0, 8);
}

/**
 * Plan Phase 3, graceful app replacement: a successor daemon starts while the predecessor still lives
 * and adopts its sessions.
 *
 * This must go through the real handover path, because that is the only path in the tree that carries a
 * session across an app restart without a crash: `SIGTERM` is NOT a handover -- `cli.rs` answers it by
 * calling `shutdown_gracefully`, which persists remote sessions and removes this owner's endpoint
 * artifacts, and the PTYs die with the process. So the case drives what the tree actually implements:
 *
 *   1. `prepareHandover` on the predecessor, which binds the one-shot legacy socket and returns its
 *      path plus the sessions it currently holds.
 *   2. start the successor with `--handover-from <that path>`, which is the flag `cli.rs` reads to
 *      decide it is a candidate.
 *   3. `commitHandover` on the predecessor, which hands ownership over and retires it.
 *
 * A session that did not survive is a real failure here, because the graceful path is supposed to
 * carry every local session across: the predecessor keeps publishing its own socket until commit and
 * never exits before the successor is serving.
 */
defineCase(
  'graceful-app-replacement',
  'graceful replacement preserves child identities through the real --handover-from path',
  ['keeper', 'daemon', 'handoverPath'],
  async (ctx) => {
    // A readable prefix cannot live in the id: `session_service.rs` only accepts a canonical
    // lowercase UUID and refuses anything else with `INVALID_ARGUMENT` before spawning.
    const sessionId = randomUUID();
    await ctx.daemon.spawn({ sessionId, workspaceId: ctx.workspaceId, cwd: ctx.paths.repo });
    const live = (await ctx.daemon.liveSessions()).find((row) => row.backendId === sessionId);
    if (!live) throw fail(FAIL.MISSING_OUTPUT, `${sessionId} never appeared in describeLiveSessions`);

    const before = await assertProcessIdentity(
      await processTable(),
      live.localChildPid,
      live.localChildStartTime,
      'before graceful replacement',
      'ms',
    );
    const predecessorPid = ctx.daemonProcess.pid;

    const prepared = await ctx.daemon.prepareHandover();
    if (!prepared.legacySocketPath) {
      throw fail(FAIL.PROTOCOL, 'prepareHandover returned no legacy socket path for the successor');
    }
    await ctx.evidence.record({
      case: 'graceful-app-replacement',
      step: 'prepared',
      predecessorPid,
      legacySocketPath: prepared.legacySocketPath,
      activeSessions: prepared.activeSessions,
    });
    // The predecessor is told about the session it is about to hand over. If it does not list this
    // session, the successor cannot be expected to adopt it, and the identity assertion below would
    // be testing the wrong thing.
    if (!prepared.activeSessions.includes(sessionId)) {
      throw fail(
        FAIL.MISSING_OUTPUT,
        `prepareHandover listed ${JSON.stringify(prepared.activeSessions)} without ${sessionId}`,
      );
    }

    // The real candidate start. This process adopts over the handover socket and, once it commits,
    // serves the predecessor's public socket.
    ctx.daemon = await ctx.startDaemon({ role: 'successor', handoverFrom: prepared.legacySocketPath });
    const successorPid = ctx.daemonProcess.pid;

    // `--handover-from` does NOT bind the canonical socket: the successor serves the predecessor's
    // inherited listener only once it commits, and it sends that commit itself
    // (`daemon/server.rs:6336`, `:5920-5923`). Until then `<runtimeDir>/daemon.sock` is still the
    // predecessor's, so `startDaemon`'s readiness handshake -- and every request issued straight
    // after it -- can be answered by either process. That is not a product defect (the socket never
    // disappearing is the point), but it does mean the case must wait for the event that names who
    // serves it. The handshake carries the serving process's own pid (`server.rs:7274`), so the event
    // is observable: connected pid equals the successor's. Without this wait the attach below could
    // read from one daemon while the write reached the other, and the echo would never arrive --
    // which is exactly the deadline this case used to hit.
    const servedBy = await awaitSuccessorServing(ctx, 'the --handover-from successor serves the canonical socket');
    await ctx.evidence.record({
      case: 'graceful-app-replacement',
      step: 'successor-serving',
      successorPid,
      servedBy,
      predecessorPid,
    });

    // Identity first, straight through the successor's own view: a child that is still the same
    // process is the whole property, and asserting it here means a failure is reported as an identity
    // mismatch instead of being blurred into a later attach error.
    const afterRows = await ctx.daemon.liveSessions();
    const after = afterRows.find((row) => row.backendId === sessionId);
    if (!after) {
      throw fail(
        FAIL.MISSING_OUTPUT,
        `${sessionId} was not adopted by the --handover-from successor (it lists ${JSON.stringify(afterRows.map((row) => row.backendId))})`,
      );
    }
    if (after.localChildPid !== before.pid) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `pid changed ${before.pid} -> ${after.localChildPid} across a graceful replacement`,
      );
    }
    const proved = await assertProcessIdentity(
      await processTable(),
      after.localChildPid,
      after.localChildStartTime,
      'after graceful replacement',
      'ms',
    );
    if (proved.startTimeMicros !== before.startTimeMicros) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `start time changed ${before.startTimeMicros}us -> ${proved.startTimeMicros}us across a graceful replacement`,
      );
    }

    // A successor that adopted the session must also be able to serve its bytes, not merely list it.
    const attachment = await ctx.attachSession(sessionId, null);
    if (attachment.gap) {
      throw fail(FAIL.GAP_IN_NO_GAP_CASE, `the adopted session reported a gap: ${JSON.stringify(attachment.gap)}`);
    }
    const session = new AttachedSession(attachment, ctx.evidence);
    // This case spawns through `ctx.daemon.spawn` with no argv (`:5301`), so its child is the daemon's
    // configured login shell, not an `echoChild()`. run14c's adopted stream carried exactly
    // `HANDOVER:9e5f000d\r\n\x1b[?2004l\r` followed by `bash: HANDOVER:9e5f000d: command not found\r\n`,
    // which is the write echoed back AND executed - proof the handover delivered it. `writeToSession`
    // folds the tty's `\r` and backspace repaint, so the default needle matches.
    const ping = Buffer.from(`HANDOVER:${nonce()}\n`);
    await writeToSession(ctx, sessionId, session, ping, 'output from the adopted session', 30_000);
    session.attachment.stream.close();

    // The keeper is only involved when the daemon actually routes through it. When it does, the same
    // pid it held before the replacement must be the pid it holds after; when the daemon opens masters
    // directly there is no keeper entry to compare, and that absence is recorded rather than asserted
    // away -- the identity assertions above stand on their own.
    const ownedAfter = (await ctx.keeper.ownership()).find((entry) => entry.sessionId === sessionId) ?? null;
    if (ownedAfter && ownedAfter.pid !== before.pid) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `keeper pid changed ${before.pid} -> ${ownedAfter.pid} across a graceful replacement`,
      );
    }

    return {
      sessionId,
      predecessorPid,
      successorPid: ctx.daemonProcess.pid,
      pid: proved.pid,
      startTimeMicros: proved.startTimeMicros,
      predecessorListedSession: true,
      keeperPidUnchanged: ownedAfter ? true : null,
      keeperHeldSession: Boolean(ownedAfter),
      note: ownedAfter
        ? 'driven through prepareHandover + --handover-from, with the keeper holding the same child pid throughout'
        : 'driven through prepareHandover + --handover-from; the daemon opened this PTY directly, so there is no keeper entry to compare',
    };
  },
);

/**
 * Plan Phase 3, mixed ownership: both ownership types present at once, produced the way the plan
 * says they are produced -- **not** by migrating a live descriptor.
 *
 * Plan lines 149-152: "The flag applies to new sessions only. Enabling it routes new spawns to the
 * keeper; existing direct-owned sessions keep their original owner and drain there unless an
 * explicit FD migration succeeds." So the coexistence state is reached by *ordering*, not by
 * migration: direct session first, enable, then a keeper session. `fdMigrationPresent` is therefore
 * not a prerequisite of this scenario, and treating it as one blocked a case that is drivable.
 *
 * Two production facts make the ordering work, and both are the reason this is a real test:
 *
 *   - `OwnershipTable::new_session_owner()` consults only the persisted flag, and
 *     `terminal/pty.rs` asks `routes_new_sessions_to_keeper()` at *every* spawn, re-reading the
 *     table from disk. A session spawned before the flag was enabled is never rewritten; it simply
 *     drains under its original owner.
 *   - The flag is written by `start_production` -> `activate_new_sessions` -> `activate_ownership`
 *     at daemon start, so enabling it is a restart boundary, and `may_activate` is false on an
 *     isolated runtime, so no `launchctl` call is involved.
 *
 * What the *daemon* needs beyond that is the bundle layout `bundle_keeper_dir` derives from
 * `current_exe()`: a plain cargo binary yields `PayloadAbsent` and never installs the runtime. The
 * case therefore runs bundled copies of the real binaries (see `prepareKeeperBundle`), which is a
 * layout change, not a substitution.
 */
defineCase(
  'mixed-ownership-drain',
  'a direct-owned session drains while a keeper-owned session is created after enabling the flag',
  ['keeper', 'daemon', 'ps'],
  async (ctx) => {
    // Measured here rather than inherited, so the case states which seam it depends on.
    let surfaces;
    try {
      surfaces = (await readSourceFacts(ctx.paths.repo)).surfaces;
    } catch (error) {
      const detail = error instanceof SourceFactError ? `${error.code}: ${error.detail}` : String(error?.message ?? error);
      throw gap(GAP.OWNERSHIP_FLAG_ABSENT, `${GAP.OWNERSHIP_FLAG_ABSENT}: the source surfaces could not be read (${detail})`);
    }
    const surfaceGaps = [];
    if (!surfaces.ownershipEnableSurface) surfaceGaps.push('ownershipEnableSurface');
    if (!surfaces.keeperSessionWired?.length) surfaceGaps.push('keeperSessionWired');
    if (surfaceGaps.length) {
      await ctx.evidence.record({ case: 'mixed-ownership-drain', surfaceGaps });
      throw gap(
        GAP.OWNERSHIP_FLAG_ABSENT,
        `${GAP.OWNERSHIP_FLAG_ABSENT}: absent ${surfaceGaps.join(', ')}, so a session can never be routed to the keeper`,
      );
    }
    // The sequence below is deliberately still driven where the flag cannot be enabled: the bundle
    // layout, the direct session, the graceful handover and the adopted-session byte check are
    // independent of keeper routing and are real product behaviour on any host. Only the flag check
    // needs the start pass, and that is where the platform gate is named.

    const { bundledDaemon } = await prepareKeeperBundle(
      ctx.paths,
      ctx.binaries.daemonBinary,
      ctx.binaries.keeperBinary,
    );
    // The runner already started a daemon for this case (every case declaring `daemon` gets one). It
    // must be gone before the bundled one binds the same socket, and it must be gone *before* the
    // first spawn so the direct session below is created by the bundled build, whose startup is what
    // installs the keeper runtime. `killDaemon` is a SIGTERM, so nothing here is a crash boundary.
    await ctx.killDaemon('SIGTERM');
    // The flag has to be OFF for this spawn, and a data dir is not something this case can assume: it
    // is `planPaths(root).dataDir`, one per RUN, not per case -- every case in the run shares
    // it. `runCase` now resets the durable table before each case, so no flag is inherited, but the
    // OFF state is still pinned explicitly rather than left to "the reset left nothing behind": a
    // direct-owned session is impossible to create once a flag is on, the spawn is then
    // keeper-routed, and the daemon answers `Session '<id>' not found` because a keeper-routed spawn
    // never inserts a `PtySession` into `pty_manager` (`terminal/pty.rs:729-750` returns before the
    // insert at `:870-872`), which is what `handle_spawn_requested` describes back through
    // `handle_describe_session` (`session_service.rs:3844`). `FERRYX_KEEPER_OWNERSHIP=disabled` is the
    // production surface for pinning this (`keeper_startup.rs:766` names the variable,
    // `OwnershipPolicy::parse` at `:772-784` accepts `disabled`, and `apply_policy` at `:955-963`
    // turns it into the disable write), so the phase is made deterministic instead of inherited.
    ctx.daemon = await ctx.startDaemon({ role: 'mixed-direct', binary: bundledDaemon, ownership: 'disabled' });

    // Phase 1 of the sequence: a direct-owned session, spawned while the flag is still off.
    const directId = randomUUID();
    await ctx.daemon.spawn({ sessionId: directId, workspaceId: ctx.workspaceId, cwd: ctx.paths.repo });
    const directLive = (await ctx.daemon.liveSessions()).find((row) => row.backendId === directId);
    if (!directLive) throw fail(FAIL.MISSING_OUTPUT, `${directId} never appeared in describeLiveSessions`);
    const directIdentity = await assertProcessIdentity(
      await processTable(),
      directLive.localChildPid,
      directLive.localChildStartTime,
      'the direct-owned session before the flag is enabled',
      // `localChildStartTime` is milliseconds (`start_token / 1000`), not microseconds.
      'ms',
    );
    const directOwnedBefore = (await ctx.keeper.ownership()).find((entry) => entry.sessionId === directId) ?? null;
    if (directOwnedBefore) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `${directId} is already keeper-owned before the flag was enabled, so there is no direct-owned session to drain`,
      );
    }

    // Enabling is a *restart* boundary -- `start_production` activates the flag at daemon start and
    // there is no IPC verb that flips it while serving. The restart must therefore be the graceful
    // path, not a kill: a SIGTERM'd daemon takes the direct session's PTY master with it, so the
    // "drains there" half of the plan would be untestable because there would be nothing left to
    // drain. `prepareHandover` + `--handover-from` is the production replacement path and it carries
    // local sessions across, which is exactly the boundary this scenario needs.
    const prepared = await ctx.daemon.prepareHandover();
    if (!prepared.legacySocketPath) {
      throw fail(FAIL.PROTOCOL, 'prepareHandover returned no legacy socket path for the enabling successor');
    }
    if (!prepared.activeSessions.includes(directId)) {
      throw fail(
        FAIL.MISSING_OUTPUT,
        `prepareHandover listed ${JSON.stringify(prepared.activeSessions)} without ${directId}, so the restart would not carry it`,
      );
    }
    ctx.daemon = await ctx.startDaemon({
      role: 'mixed-keeper',
      binary: bundledDaemon,
      handoverFrom: prepared.legacySocketPath,
    });
    // The direct session this case drains belongs to the predecessor until the successor commits, and
    // the drain below attaches on one connection and writes on another. Without this wait both can
    // reach different daemons and the echo is published to nobody, which is a harness fault reported
    // as a product timeout.
    await awaitSuccessorServing(ctx, 'the mixed-keeper successor serves the canonical socket');
    const flag = await readOwnershipTable(ctx.paths.dataDir);
    if (flag?.keeperOwnershipEnabled !== true) {
      // A flag never written because the start pass cannot run on this host is a platform gap, not a
      // daemon that ignored a flag it could have set.
      const unavailable = await keeperStartPassUnavailable(ctx);
      throw gap(
        unavailable?.code ?? GAP.OWNERSHIP_FLAG_ABSENT,
        `${unavailable?.detail ?? GAP.OWNERSHIP_FLAG_ABSENT}: the daemon did not enable keeper ownership on restart (table: ${JSON.stringify(flag)}), so no keeper-owned session can be created`,
      );
    }
    // The direct session survived the restart on its own terms and is STILL not keeper-owned: that
    // is the plan's "existing direct-owned sessions keep their original owner".
    const directAfter = (await ctx.daemon.liveSessions()).find((row) => row.backendId === directId);
    if (!directAfter) throw fail(FAIL.MISSING_OUTPUT, `${directId} is gone after the enabling restart`);
    await assertProcessIdentity(
      await processTable(),
      directIdentity.pid,
      directIdentity.startTimeMicros,
      'the direct-owned session after the flag was enabled',
    );
    const directOwnedAfter = (await ctx.keeper.ownership()).find((entry) => entry.sessionId === directId) ?? null;
    if (directOwnedAfter) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `${directId} became keeper-owned when the flag was enabled, but the plan says the flag applies to new sessions only`,
      );
    }

    // Phase 2: a session created under the enabled flag must be keeper-owned.
    const keeperId = randomUUID();
    await ctx.daemon.spawn({ sessionId: keeperId, workspaceId: ctx.workspaceId, cwd: ctx.paths.repo });
    const keeperLive = (await ctx.daemon.liveSessions()).find((row) => row.backendId === keeperId);
    if (!keeperLive) throw fail(FAIL.MISSING_OUTPUT, `${keeperId} never appeared in describeLiveSessions`);
    const keeperOwned = (await ctx.keeper.ownership()).find((entry) => entry.sessionId === keeperId) ?? null;
    if (!keeperOwned) {
      // The seam is measured, not assumed: the message names whichever one is actually missing.
      const routing = await keeperRoutingGap(ctx);
      throw gap(routing.code, `${routing.detail}: ${keeperId} was spawned with the flag on but is not in the keeper's registry`);
    }
    if (keeperOwned.pid !== keeperLive.localChildPid) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `the keeper holds pid ${keeperOwned.pid} for ${keeperId}, the daemon reports ${keeperLive.localChildPid}`,
      );
    }

    // Both ownership types now exist under one daemon, which is the plan's acceptance state.
    const table = await readOwnershipTable(ctx.paths.dataDir);
    const owners = Object.fromEntries(
      Object.entries(table?.sessions ?? {}).map(([id, record]) => [id, record?.owner ?? null]),
    );
    if (owners[keeperId] !== 'keeper') {
      throw fail(FAIL.PROTOCOL, `${keeperId} is recorded as ${JSON.stringify(owners[keeperId])}, not "keeper"`);
    }
    // A direct-owned session is never written to the ownership table at all: only `spawn` on the
    // keeper path and keeper discovery record rows, and both set `owner: Keeper`. So the correct
    // assertion is absence, not an `owner: "daemon"` row -- asserting a row would fail on a healthy
    // tree. Its ownership is proven by `describeLiveSessions` serving it while the keeper is unaware
    // of it, which the checks above established.
    if (directId in owners) {
      throw fail(
        FAIL.PROTOCOL,
        `${directId} appears in the ownership table as ${JSON.stringify(owners[directId])}; a session spawned before the flag was enabled must not be recorded as keeper-owned`,
      );
    }

    // The direct session must still serve bytes: draining is a description of ownership, not of a
    // dead PTY. This is what makes "drain there" observable rather than a table entry.
    const attachment = await ctx.attachSession(directId, null);
    if (attachment.gap) throw fail(FAIL.GAP_IN_NO_GAP_CASE, `the draining session reported a gap: ${JSON.stringify(attachment.gap)}`);
    const draining = new AttachedSession(attachment, ctx.evidence);
    try {
      const ping = Buffer.from(`DRAIN:${nonce()}\n`);
      await writeToSession(ctx, directId, draining, ping, 'output from the draining direct session', 30_000);
    } finally {
      draining.attachment.stream.close();
    }

    await ctx.evidence.record({
      case: 'mixed-ownership-drain',
      directId,
      keeperId,
      directPid: directIdentity.pid,
      keeperPid: keeperOwned.pid,
      flagEnabled: true,
      owners,
    });
    return {
      directId,
      keeperId,
      // The table only ever stores keeper-owned rows, so the direct session's ownership is reported
      // for what was observed of it: served by the daemon, absent from the keeper's registry.
      directOwner: 'daemon (unrecorded; served directly)',
      keeperOwner: owners[keeperId],
      directPidUnchanged: true,
      drainedWithoutMigration: true,
      note: 'direct session spawned before enabling, keeper session after; no FD migration was involved',
    };
  },
);

/**
 * Plan Phase 3, rollback refusal: a rollback to a build that cannot service keeper-owned sessions is
 * refused, and the refusal names the exact live count.
 *
 * This drives `ferryx-cli service rollback <version> --json` -- the shipped surface -- against a
 * fixture. Three properties make it safe and real at once:
 *
 *   - The ownership table is NOT fabricated. The session is spawned through the daemon and confirmed
 *     in the keeper's own registry first; a refusal produced from a hand-written table would prove
 *     only that JSON parses. `run_rollback` reads ownership through `FERRYX_DATA_DIR`, which the
 *     harness already points at the fixture (`OwnershipStore::for_runtime`), so the tool sees the
 *     real table of this isolated runtime and never the user's.
 *   - No `launchctl` runs. `keeper_launchd::rollback_gate` decides before `real_paths()` is resolved,
 *     and a refusal returns before any file is written, so the refusal path touches nothing outside
 *     the fixture. This case therefore claims only the refusal; the successful half of the sequence
 *     is `enable-disable-rollback-sequence`, which redirects the paths a success writes.
 *   - Isolation is asserted, not assumed: every path is checked against the fixture root before the
 *     CLI is spawned, and the child gets a scrubbed environment with the QA label.
 */
defineCase(
  'rollback-refusal-names-keeper-count',
  'service rollback refuses with a typed code and the live keeper-owned count, changing nothing',
  ['keeper', 'daemon', 'keeperIntegration', 'rollback', 'cli'],
  async (ctx) => {
    // A version the fixture definitely has no staged generation for. The refusal must come from the
    // ownership rule, not from "target not staged", so the assertion below checks the CODE.
    const targetVersion = '1900.101.1';

    const sessionId = randomUUID();
    await ctx.daemon.spawn({ sessionId, workspaceId: ctx.workspaceId, cwd: ctx.paths.repo });
    const owned = (await ctx.keeper.ownership()).find((entry) => entry.sessionId === sessionId);
    if (!owned) {
      throw fail(
        FAIL.MISSING_OUTPUT,
        `${sessionId} is not keeper-owned, so a rollback would not strand anything and the refusal would be about something else`,
      );
    }
    // The durable table is what the CLI reads, so the record must exist there too -- the keeper
    // registry alone is a different store.
    const tablePath = join(ctx.paths.dataDir, 'pty-ownership.json');
    let tableBytes;
    try {
      tableBytes = await readFile(tablePath, 'utf8');
    } catch (error) {
      throw fail(
        FAIL.PROTOCOL,
        `the durable ownership table ${tablePath} is unreadable (${error?.code ?? error}), so the CLI could not see this session`,
      );
    }
    let table;
    try {
      table = JSON.parse(tableBytes);
    } catch (error) {
      throw fail(FAIL.PROTOCOL, `${tablePath} is not JSON: ${String(error)}`);
    }
    const recorded = table?.sessions?.[sessionId];
    if (!recorded) {
      throw fail(FAIL.PROTOCOL, `${tablePath} has no record for ${sessionId}, so the CLI would count 0`);
    }
    if (recorded.owner !== 'keeper') {
      throw fail(FAIL.PROTOCOL, `${sessionId} is recorded as owned by ${JSON.stringify(recorded.owner)}, not the keeper`);
    }

    // The refusal this case wants is the *ownership* one, and the gate checks the version floor
    // first. A missing manifest refuses with `KEEPER_ROLLBACK_FLOOR_UNVERIFIED`; a manifest naming a
    // floor marks the target keeper-capable, which makes keeper sessions stop blocking. So the
    // fixture manifest must be valid JSON that records NO floor, which is exactly the
    // `FloorAnswer::NoFloorRecorded` case: only the ownership rule applies.
    const manifestPath = join(ctx.paths.keeperDir, 'rollback-manifest.json');
    await writeFile(manifestPath, '{}\n');

    // Safety gate: the CLI must not be able to reach a production path from here. The label is a QA
    // name of its own (not the keeper's) so the daemon plists a successful rollback could rewrite
    // would be QA files too -- and the plist path is a fixture directory, never ~/Library.
    const cliEnv = ctx.env({
      FERRYX_DAEMON_SUPERVISOR_NAME: 'com.ferryx.qa.phase3-daemon',
      FERRYX_DAEMON_PLIST_PATH: join(ctx.paths.keeperDir, 'agents'),
      FERRYX_ROLLBACK_MANIFEST: manifestPath,
    });
    for (const key of ['FERRYX_RUNTIME_DIR', 'FERRYX_LEGACY_RUNTIME_DIR', 'FERRYX_DATA_DIR', 'FERRYX_SESSION_DIR']) {
      const value = cliEnv[key];
      if (!value || !resolve(value).startsWith(resolve(ctx.paths.root))) {
        throw fail(FAIL.SAFETY, `${key} is ${JSON.stringify(value)}, which is not inside the fixture root`);
      }
    }
    for (const key of ['FERRYX_DAEMON_PLIST_PATH', 'FERRYX_ROLLBACK_MANIFEST']) {
      if (!resolve(cliEnv[key]).startsWith(resolve(ctx.paths.root))) {
        throw fail(FAIL.SAFETY, `${key} is ${JSON.stringify(cliEnv[key])}, which is not inside the fixture root`);
      }
    }

    const argv = [ctx.binaries.cliBinary, 'service', 'rollback', targetVersion, '--json'];
    const child = spawn(argv[0], argv.slice(1), {
      cwd: ctx.paths.repo,
      env: cliEnv,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    const supervised = new Supervised('cli-rollback', child, argv, cliEnv, ctx.evidence);
    let exit;
    try {
      exit = await supervised.waitForExit(60_000);
    } finally {
      await supervised.stop().catch(() => null);
    }
    if (supervised.spawnError) {
      throw gap(
        GAP.CLI_BINARY_ABSENT,
        `${GAP.CLI_BINARY_ABSENT}: could not execute ${ctx.binaries.cliBinary} (${supervised.spawnError.code ?? supervised.spawnError.message})`,
      );
    }

    // `print_up_cli_error` writes the JSON to stderr; `UP_EXIT_OTHER` is the refusal exit code
    // (`UP_EXIT_OK` is 0, `UP_EXIT_USAGE` is 2), so a 2 is a harness bug, not a refusal.
    if (exit.code === 2) {
      throw fail(FAIL.USAGE, `the CLI rejected the invocation ${JSON.stringify(argv.slice(1))}: ${supervised.stderr.slice(-1_024)}`);
    }
    if (exit.code !== 1) {
      throw fail(
        FAIL.PROTOCOL,
        `service rollback exited ${exit.code}${exit.signal ? ` on ${exit.signal}` : ''}, expected 1 (refusal): ${supervised.stderr.slice(-2_048)}`,
      );
    }
    let refusal;
    try {
      refusal = JSON.parse(supervised.stderr.trim().split('\n').pop());
    } catch (error) {
      throw fail(FAIL.PROTOCOL, `the refusal was not JSON on stderr: ${supervised.stderr.slice(-2_048)}`);
    }
    if (refusal?.type !== 'error') {
      throw fail(FAIL.PROTOCOL, `the refusal JSON has type ${JSON.stringify(refusal?.type)}, expected "error"`);
    }
    // The typed code is the whole point: a refusal for "no staged generation" would also exit 1, so
    // the code must be the ownership one, and the message must carry the live count.
    if (refusal.code !== 'KEEPER_ROLLBACK_REFUSED') {
      throw fail(
        FAIL.REFUSAL_CODE,
        `service rollback refused with ${JSON.stringify(refusal.code)}, not KEEPER_ROLLBACK_REFUSED (message: ${JSON.stringify(refusal.message)})`,
      );
    }
    const claimed = /(\d+) session\(s\)/.exec(String(refusal.message ?? ''));
    if (!claimed) {
      throw fail(FAIL.MISSING_OUTPUT, `the refusal names no session count: ${JSON.stringify(refusal.message)}`);
    }
    const counted = Number(claimed[1]);
    // The count is read from the keeper's own registry at this instant, not from the table the CLI
    // just read, so a table the harness wrote could not satisfy this.
    const liveOwned = (await ctx.keeper.ownership()).length;
    if (counted !== liveOwned) {
      throw fail(
        FAIL.BYTE_ACCOUNTING,
        `the refusal names ${counted} keeper-owned session(s), the keeper reports ${liveOwned}`,
      );
    }
    // The refusal must have changed nothing: the session still echoes, so no PTY was disturbed.
    const attachment = await ctx.attachSession(sessionId, null);
    if (attachment.gap) throw fail(FAIL.GAP_IN_NO_GAP_CASE, `the session reported a gap after the refusal: ${JSON.stringify(attachment.gap)}`);
    const session = new AttachedSession(attachment, ctx.evidence);
    try {
      const ping = Buffer.from(`RBK:${nonce()}\n`);
      await writeToSession(ctx, sessionId, session, ping, 'echo after the refused rollback', 30_000);
    } finally {
      session.attachment.stream.close();
    }

    await ctx.evidence.record({
      case: 'rollback-refusal-names-keeper-count',
      argv,
      targetVersion,
      exitCode: exit.code,
      refusalCode: refusal.code,
      refusalMessage: refusal.message,
      counted,
      keeperOwned: liveOwned,
    });
    return {
      code: refusal.code,
      keeperOwned: counted,
      sessionStillServing: true,
      note: 'the shipped CLI refused the rollback and named the live keeper-owned count; nothing was rewritten',
    };
  },
);

/**
 * Plan Phase 3, mixed generation: the app is replaced twice and the daemon crashes in between, so a
 * session must survive an old build handing over to a new build, and then survive that new build
 * dying. This is the plan's "old and new run side by side, then the crash" line, which the keeper-only
 * `multi-generation-compat` case does not cover: that one never starts a daemon.
 *
 * It is driven with two real binaries supplied by the operator (`--daemon-generations`). Both must
 * answer a handshake with a *different* `expectedVersion`, because "old replaced by new" is a
 * statement about protocol revisions; the same bytes copied twice would prove nothing, so an
 * identical pair is refused rather than passed.
 *
 * Sequence:
 *   1. build A starts, spawns a session and becomes the serving daemon.
 *   2. A prepares, build B adopts over the real `--handover-from` path and becomes the successor.
 *   3. B is SIGKILLed. Its PTYs must survive, because the keeper holds the masters, not the daemon.
 *   4. build A starts again and must reattach the same child pid and start time.
 *
 * Every assertion about identity is made against `ps`, not against a claim in the protocol.
 */
defineCase(
  'mixed-generation-replacement-and-crash',
  'old build hands over to a new build, that build dies, and the child survives both boundaries',
  ['keeper', 'daemon', 'daemonGenerations', 'keeperIntegration', 'ps'],
  async (ctx) => {
    const [buildA, buildB] = ctx.options.daemonGenerations;

    // The runner already started a daemon for this case (it declares `daemon`), and that one holds
    // `daemon.lock` and is serving `daemon.sock`. A second `--daemon` on the same runtime loses the
    // instance-lock race and exits 0 with an empty stderr by design (`cli.rs:4718-4739`: "Another
    // daemon already serves this endpoint; exiting without taking over" is a `tracing::warn`, i.e.
    // it goes to the daemon log, never to stderr), which the readiness wait reported as "exited
    // before binding" with nothing after the colon. Every other case that starts its own daemon
    // (`mixed-ownership-drain` at `:5547`, `enable-disable-rollback-sequence` at `:7130`) releases
    // the runner's first; this one has to as well, or build A never gets to run.
    await ctx.killDaemon('SIGTERM');

    // Where build A's own log output will start. The start-pass question below must be answered by
    // *this* generation, and the daemon log is one append-only file shared by every daemon this run
    // starts, so the offset has to be taken before the spawn rather than inferred afterwards.
    const daemonLogOffset = ((await ctx.evidence.daemonLog(ctx.paths.dataDir)).text ?? '').length;

    // Build A is the predecessor. `negotiateProtocol` is deliberately on: the harness is compiled
    // against one revision and these are two builds, so each is asked which revision it serves.
    // No ownership policy is passed on purpose: `runCase` reset the durable table before this case
    // started, so this daemon decides routing at its own start pass -- which is the behaviour the
    // final crash boundary depends on, and pinning `enabled` would only hide a start pass that failed.
    ctx.daemon = await ctx.startDaemon({ role: 'initial', binary: buildA, negotiateProtocol: true });
    const versionA = ctx.daemon.identity?.version ?? null;

    const sessionId = randomUUID();
    await ctx.daemon.spawn({ sessionId, workspaceId: ctx.workspaceId, cwd: ctx.paths.repo });
    const live = (await ctx.daemon.liveSessions()).find((row) => row.backendId === sessionId);
    if (!live) throw fail(FAIL.MISSING_OUTPUT, `${sessionId} never appeared in describeLiveSessions`);
    const before = await assertProcessIdentity(
      await processTable(),
      live.localChildPid,
      live.localChildStartTime,
      `before the ${buildA} -> ${buildB} replacement`,
      'ms',
    );
    // The durable proof that this session is keeper-owned: without it a surviving child would only
    // show the daemon never died, not that the keeper carried it.
    const owned = (await ctx.keeper.ownership()).find((entry) => entry.sessionId === sessionId);

    // Held separately because `startDaemon` reassigns `ctx.daemonProcess` to the successor. Without
    // this the predecessor would only be reachable through `shutdown()`, and a predecessor that has
    // not fully retired would keep the runtime lock, making the restart below look like a crash.
    const predecessor = ctx.daemonProcess;
    if (!owned) {
      // Which side is at fault is decided by evidence, not by assumption. A build with no keeper start
      // pass cannot route a spawn no matter how long the harness waits for the flag, so the older
      // generation's own log is what tells "never ran a pass" apart from "ran one and did not route".
      const generationLog = ((await ctx.evidence.daemonLog(ctx.paths.dataDir)).text ?? '').slice(daemonLogOffset);
      if (!keeperStartPassRan(generationLog)) {
        throw gap(
          GAP.KEEPER_INTEGRATION_ABSENT,
          `${GAP.KEEPER_INTEGRATION_ABSENT}: the build supplied as the old generation (${buildA}) never ran a ` +
            `keeper start pass -- its own daemon log carries no "keeper startup pass finished" line after it ` +
            `started, so this binary contains no keeper code and every spawn it makes is direct-owned. No ` +
            `keeper-owned session can exist for the A -> B boundary to carry, so this case's premise is not ` +
            `drivable with the generation pair supplied. Supply an older build that already routes new ` +
            `sessions to the keeper (and that speaks a different protocol revision than the newer one).`,
        );
      }
      const routing = await keeperRoutingGap(ctx);
      throw gap(
        routing.code,
        `${routing.detail}: this generation ran its keeper start pass and still left ${sessionId} direct-owned`,
      );
    }

    const prepared = await ctx.daemon.prepareHandover();
    if (!prepared.legacySocketPath) throw fail(FAIL.PROTOCOL, 'prepareHandover returned no legacy socket path');
    if (!prepared.activeSessions.includes(sessionId)) {
      throw fail(
        FAIL.MISSING_OUTPUT,
        `prepareHandover listed ${JSON.stringify(prepared.activeSessions)} without ${sessionId}`,
      );
    }
    const predecessorPid = ctx.daemonProcess.pid;

    // Boundary 1: a different build adopts the session.
    ctx.daemon = await ctx.startDaemon({
      role: 'successor',
      binary: buildB,
      handoverFrom: prepared.legacySocketPath,
      negotiateProtocol: true,
    });
    await awaitSuccessorServing(ctx, 'the build-B successor serves the canonical socket');
    const versionB = ctx.daemon.identity?.version ?? null;
    // The whole point of using two builds: they must not be the same protocol revision. When they are
    // identical, the pair is one binary and this case cannot claim to have tested a replacement.
    if (versionA === null || versionB === null) {
      throw gap(
        GAP.DAEMON_GENERATIONS_INDISTINCT,
        `${GAP.DAEMON_GENERATIONS_INDISTINCT}: build A reported version ${JSON.stringify(versionA)}, build B ${JSON.stringify(versionB)}`,
      );
    }
    if (versionA === versionB) {
      throw gap(
        GAP.DAEMON_GENERATIONS_INDISTINCT,
        `${GAP.DAEMON_GENERATIONS_INDISTINCT}: both builds speak protocol ${versionA}`,
      );
    }

    const adopted = (await ctx.daemon.liveSessions()).find((row) => row.backendId === sessionId);
    if (!adopted) throw fail(FAIL.MISSING_OUTPUT, `${sessionId} was not adopted by the newer build`);
    if (adopted.localChildPid !== before.pid) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `pid changed ${before.pid} -> ${adopted.localChildPid} across the build replacement`,
      );
    }
    const afterHandover = await assertProcessIdentity(
      await processTable(),
      adopted.localChildPid,
      adopted.localChildStartTime,
      'after the build replacement',
      'ms',
    );
    if (afterHandover.startTimeMicros !== before.startTimeMicros) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `start time changed ${before.startTimeMicros}us -> ${afterHandover.startTimeMicros}us across the replacement`,
      );
    }

    // The commit retired the predecessor; stopping it explicitly guarantees no process is still
    // holding this runtime's lock when build A is started again below.
    const predecessorExit = await predecessor.stop().catch(() => null);
    await ctx.evidence.record({
      case: 'mixed-generation-replacement-and-crash',
      step: 'predecessor-retired',
      pid: predecessor.pid,
      exit: predecessorExit,
    });

    // Boundary 2: the successor crashes. SIGKILL, so nothing is flushed and only the keeper's hold on
    // the master can explain the child still being there.
    const crashedPid = ctx.daemonProcess.pid;
    await ctx.killDaemon('SIGKILL');
    const tableDuringOutage = await processTable();
    if (!tableDuringOutage.get(before.pid)) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `child ${before.pid} is gone after the successor died, so the keeper did not hold the master`,
      );
    }
    await assertProcessIdentity(tableDuringOutage, before.pid, before.startTimeMicros, 'during the outage');

    // Recovery: build A serves the same session again. This is the third boundary, and it crosses
    // back over a protocol revision, which is the mixed-generation part.
    ctx.daemon = await ctx.startDaemon({ role: 'recovered', binary: buildA, negotiateProtocol: true });
    const recovered = (await ctx.daemon.liveSessions()).find((row) => row.backendId === sessionId);
    if (!recovered) throw fail(FAIL.MISSING_OUTPUT, `${sessionId} did not return after the crash`);
    if (recovered.localChildPid !== before.pid) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `pid changed ${before.pid} -> ${recovered.localChildPid} across the crash`,
      );
    }
    const afterCrash = await assertProcessIdentity(
      await processTable(),
      recovered.localChildPid,
      recovered.localChildStartTime,
      'after the crash',
      'ms',
    );
    if (afterCrash.startTimeMicros !== before.startTimeMicros) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `start time changed ${before.startTimeMicros}us -> ${afterCrash.startTimeMicros}us across the crash`,
      );
    }

    // The session must still be usable, not merely listed.
    const attachment = await ctx.attachSession(sessionId, null);
    if (attachment.gap) throw fail(FAIL.GAP_IN_NO_GAP_CASE, `the recovered session reported a gap: ${JSON.stringify(attachment.gap)}`);
    const session = new AttachedSession(attachment, ctx.evidence);
    try {
      const ping = Buffer.from(`MIXGEN:${nonce()}\n`);
      await writeToSession(ctx, sessionId, session, ping, 'echo through the recovered session', 30_000);
    } finally {
      session.attachment.stream.close();
    }

    await ctx.evidence.record({
      case: 'mixed-generation-replacement-and-crash',
      buildA,
      buildB,
      protocolA: versionA,
      protocolB: versionB,
      predecessorPid,
      crashedPid,
      sessionId,
      childPid: before.pid,
      childStartTimeMicros: before.startTimeMicros,
      keeperOwnedBeforeHandover: true,
    });
    return {
      protocols: { a: versionA, b: versionB },
      childPid: before.pid,
      survivedReplacement: true,
      survivedCrash: true,
      note: 'two distinct daemon builds and one SIGKILL; the same child pid and start time throughout',
    };
  },
);

/**
 * Plan Phase 3, ownership flag: after the flag is disabled, every session the keeper already owns is
 * still attached and readable, and only *new* sessions stop going to the keeper.
 *
 * The disable is `FERRYX_KEEPER_OWNERSHIP=disabled`, read at daemon start by
 * `keeper_startup::reconcile_and_activate`. That pass attaches every keeper-held session first and
 * applies the policy second, inside one function, so a disable cannot strand a shell by ordering.
 * This case drives exactly that boundary: sessions are created under an enabled daemon, that daemon
 * is stopped with SIGTERM (`shutdown_gracefully`, not a handover), and the successor that has to
 * serve them starts with ownership disabled.
 *
 * Three facts the production code makes non-negotiable, so this case asserts them instead of
 * tolerating a difference:
 *   - the keeper-owned rows are NOT rewritten (`set_keeper_enabled` writes only the flag), so the
 *     table must still say `owner: "keeper"` for each pre-existing session;
 *   - nothing is respawned, so each session keeps the pid it had before the restart;
 *   - a session created after the disable is direct-owned, which is observable as absence from the
 *     keeper's registry and from the ownership table (only keeper rows are ever recorded).
 *
 * The bundle layout is required for the same reason as `mixed-ownership-drain`: `start_production`
 * derives the keeper payload from `current_exe()` and aborts with `PayloadAbsent` for a plain cargo
 * binary, in which case the startup pass that applies this policy never runs at all.
 */
defineCase(
  'ownership-disable-attaches-keeper-sessions',
  'after ownership is disabled every keeper-owned session is still attached and readable, and only new sessions are direct',
  ['keeper', 'daemon', 'keeperIntegration', 'ownershipFlag'],
  async (ctx) => {
    const { bundledDaemon } = await prepareKeeperBundle(
      ctx.paths,
      ctx.binaries.daemonBinary,
      ctx.binaries.keeperBinary,
    );
    // Every case declaring `daemon` already has one running. It must be gone before the bundled build
    // binds the same socket, and before the first spawn, so the sessions below are created by the
    // daemon whose own startup installed the keeper runtime.
    await ctx.killDaemon('SIGTERM');
    ctx.daemon = await ctx.startDaemon({ role: 'own-enabled', binary: bundledDaemon });

    // Enabling is a startup decision too, so the flag is on only because the pass that just ran found
    // a live keeper. Measured from the durable table, never assumed.
    const enabledTable = await readOwnershipTable(ctx.paths.dataDir);
    if (enabledTable?.keeperOwnershipEnabled !== true) {
      throw gap(
        GAP.OWNERSHIP_FLAG_ABSENT,
        `${GAP.OWNERSHIP_FLAG_ABSENT}: the daemon did not enable keeper ownership at startup (table: ${JSON.stringify(enabledTable)}), so there is no keeper-owned session to strand`,
      );
    }

    // Two sessions, so "every pre-existing session" is a statement about a set rather than one row.
    const sessionIds = [randomUUID(), randomUUID()];
    const before = [];
    for (const sessionId of sessionIds) {
      await ctx.daemon.spawn({ sessionId, workspaceId: ctx.workspaceId, cwd: ctx.paths.repo });
      const owned = (await ctx.keeper.ownership()).find((entry) => entry.sessionId === sessionId) ?? null;
      if (!owned) {
        const routing = await keeperRoutingGap(ctx);
        throw gap(routing.code, `${routing.detail}: ${sessionId} was spawned with the flag on but is not in the keeper's registry`);
      }
      before.push({ sessionId, pid: owned.pid, epoch: owned.epoch });
    }

    // The flag is a daemon-start boundary, so the flip takes a restart. SIGTERM is
    // `shutdown_gracefully`, not a handover: this daemon gives up its socket without transferring
    // ownership, which is what leaves the keeper holding every session for its successor.
    await ctx.killDaemon('SIGTERM');
    ctx.daemon = await ctx.startDaemon({
      role: 'own-disabled',
      binary: bundledDaemon,
      ownership: 'disabled',
    });

    // The policy really ran. Without this, everything below would be asserting continuity under a flag
    // that was never written -- the exact claim `startDaemon` used to refuse to make.
    const disabledTable = await readOwnershipTable(ctx.paths.dataDir);
    if (disabledTable?.keeperOwnershipEnabled !== false) {
      throw fail(
        FAIL.PROTOCOL,
        `${KEEPER_OWNERSHIP_ENV}=disabled did not clear the new-session routing flag (table: ${JSON.stringify(disabledTable)})`,
      );
    }

    // Attachment happens inside the startup pass, before the socket accepts a client, so the
    // successor's own view is the observation: every pre-existing session must be listed at its old
    // pid and still recorded as keeper-owned.
    const live = await until(
      `the disabled daemon serves all ${before.length} keeper-owned sessions`,
      30_000,
      async () => {
        const rows = await ctx.daemon.liveSessions();
        const seen = new Set(rows.map((row) => row.backendId));
        return before.every((entry) => seen.has(entry.sessionId)) ? rows : null;
      },
    );
    const liveById = new Map(live.map((row) => [row.backendId, row]));
    for (const entry of before) {
      const row = liveById.get(entry.sessionId);
      if (!row) throw fail(FAIL.MISSING_OUTPUT, `${entry.sessionId} is not served by the disabled daemon`);
      if (row.localChildPid !== entry.pid) {
        throw fail(
          FAIL.IDENTITY_MISMATCH,
          `${entry.sessionId}: pid changed ${entry.pid} -> ${row.localChildPid} across the disable restart, so the session was respawned instead of reattached`,
        );
      }
      const recorded = disabledTable?.sessions?.[entry.sessionId] ?? null;
      if (recorded?.owner !== 'keeper') {
        throw fail(
          FAIL.PROTOCOL,
          `${entry.sessionId} is recorded as ${JSON.stringify(recorded?.owner ?? null)} after the disable; the flip is a routing rule for new sessions and must not rewrite an existing owner`,
        );
      }
      if (recorded?.pid !== entry.pid) {
        throw fail(
          FAIL.IDENTITY_MISMATCH,
          `${entry.sessionId} is recorded with pid ${JSON.stringify(recorded?.pid ?? null)} after the disable, expected ${entry.pid}`,
        );
      }
    }

    // Being listed is not the same as being served: every session must still round-trip bytes through
    // the disabled daemon, which is what makes the attach real rather than a registry row.
    for (const entry of before) {
      const attachment = await ctx.attachSession(entry.sessionId, null);
      if (attachment.gap) {
        throw fail(
          FAIL.GAP_IN_NO_GAP_CASE,
          `attach of ${entry.sessionId} after the disable reported a gap: ${JSON.stringify(attachment.gap)}`,
        );
      }
      const session = new AttachedSession(attachment, ctx.evidence);
      try {
        const ping = Buffer.from(`DISABLED:${entry.sessionId}:${nonce()}\n`);
        await writeToSession(
          ctx,
          entry.sessionId,
          session,
          ping,
          `echo from ${entry.sessionId} after ownership was disabled`,
          30_000,
        );
      } finally {
        session.attachment.stream.close();
      }
    }

    // A session created *after* the restart is direct-owned: absent from the keeper's registry and
    // from the ownership table, but still live and usable. That is the half of the contract that says
    // the disable actually changed new-session routing.
    const freshId = randomUUID();
    await ctx.daemon.spawn({ sessionId: freshId, workspaceId: ctx.workspaceId, cwd: ctx.paths.repo });
    const fresh = await until(`the disabled daemon lists ${freshId}`, 30_000, async () => {
      const rows = await ctx.daemon.liveSessions();
      return rows.find((row) => row.backendId === freshId) ?? null;
    });
    const freshIsKeeperOwned = (await ctx.keeper.ownership()).some((entry) => entry.sessionId === freshId);
    if (freshIsKeeperOwned) {
      throw fail(
        FAIL.PROTOCOL,
        `${freshId} was spawned after ${KEEPER_OWNERSHIP_ENV}=disabled and is keeper-owned, so the disable did not change which owner new sessions get`,
      );
    }
    const tableAfterFresh = await readOwnershipTable(ctx.paths.dataDir);
    if (tableAfterFresh?.sessions?.[freshId] !== undefined) {
      throw fail(
        FAIL.PROTOCOL,
        `${freshId} was written into the ownership table as ${JSON.stringify(tableAfterFresh?.sessions?.[freshId])}; a direct-owned session has no keeper row`,
      );
    }
    const freshAttachment = await ctx.attachSession(freshId, null);
    if (freshAttachment.gap) {
      throw fail(
        FAIL.GAP_IN_NO_GAP_CASE,
        `the post-disable session reported a gap: ${JSON.stringify(freshAttachment.gap)}`,
      );
    }
    const freshSession = new AttachedSession(freshAttachment, ctx.evidence);
    try {
      const ping = Buffer.from(`DIRECT:${nonce()}\n`);
      await writeToSession(ctx, freshId, freshSession, ping, 'echo from the post-disable session', 30_000);
    } finally {
      freshSession.attachment.stream.close();
    }

    await ctx.evidence.record({
      case: 'ownership-disable-attaches-keeper-sessions',
      ownershipEnv: `${KEEPER_OWNERSHIP_ENV}=disabled`,
      preExisting: before,
      attachedPids: before.map((entry) => liveById.get(entry.sessionId)?.localChildPid ?? null),
      postDisableSession: freshId,
      postDisablePid: fresh.localChildPid ?? null,
      keeperHoldsPostDisableSession: freshIsKeeperOwned,
    });
    return {
      sessionIds,
      newSessionId: freshId,
      reusedPids: before.map((entry) => entry.pid),
      note: 'every keeper-owned session was reattached at its original pid with routing disabled, and only a session created after the restart is direct-owned',
    };
  },
);

/**
 * The harness's own safety net: the isolation contract must refuse a production path before anything
 * is started. A harness that can be pointed at the real runtime is worse than no harness.
 */
defineCase(
  'isolation-guard',
  'the isolation contract refuses production paths and overlapping roots',
  [],
  async () => {
    // Every variant starts from a valid layout and breaks exactly one property, so a passing check
    // cannot be an accident of a half-built path object. The root is short on purpose: the baseline
    // must satisfy the unix socket path budget, or every variant would fail for the wrong reason.
    const root = join(tmpdir(), 'qa-p3-selftest');
    const baseline = () => ({
      root,
      runtimeDir: join(root, 'runtime'),
      legacyRuntimeDir: join(root, 'legacy'),
      dataDir: join(root, 'data'),
      sessionDir: join(root, 'sessions'),
      keeperDir: join(root, 'keeper'),
      rawTtyDir: join(root, 'tty'),
      bundleDir: join(root, 'bundle'),
      homeDir: join(root, 'home'),
      keeperSocketPath: join(root, 'runtime', 'keeper.sock'),
      keeperPlistPath: join(root, 'keeper', `${QA_KEEPER_LABEL}.plist`),
    });
    const cases = [
      { label: 'production runtime dir', mutate: (p) => ({ ...p, runtimeDir: '/tmp' }), expect: GAP.RUNTIME_DIR_UNISOLATED },
      { label: 'missing legacy root', mutate: (p) => ({ ...p, legacyRuntimeDir: null }), expect: GAP.LEGACY_RUNTIME_DIR_UNISOLATED },
      // The real `HOME` is a production location too: once the keeper install pass runs,
      // `write_keeper_launchd_plist` creates `$HOME/Library/Logs/Ferryx`.
      { label: 'inherited home', mutate: (p) => ({ ...p, homeDir: homedir() }), expect: GAP.RUNTIME_DIR_UNISOLATED },
      { label: 'staged bundle outside the root', mutate: (p) => ({ ...p, bundleDir: join(tmpdir(), 'qa-p3-bundle') }), expect: GAP.RUNTIME_DIR_UNISOLATED },
      {
        label: 'overlapping roots',
        mutate: (p) => ({ ...p, legacyRuntimeDir: join(p.runtimeDir, 'legacy') }),
        expect: GAP.RUNTIME_LEGACY_OVERLAP,
      },
      {
        label: 'production label',
        mutate: (p) => ({ ...p, keeperPlistPath: join(root, `${PRODUCTION_KEEPER_LABEL}.plist`) }),
        expect: GAP.PRODUCTION_LABEL_REFUSED,
      },
      {
        label: 'oversize socket path',
        mutate: (p) => ({ ...p, keeperSocketPath: join(root, 'x'.repeat(MAX_SOCKET_PATH_BYTES), 'keeper.sock') }),
        expect: GAP.KEEPER_SOCKET_PATH_TOO_LONG,
      },
    ];
    // The baseline itself must pass, or the refusals below would prove nothing.
    assertIsolation(baseline());
    const refused = [];
    for (const item of cases) {
      let caught = null;
      try {
        assertIsolation(item.mutate(baseline()));
      } catch (error) {
        caught = error;
      }
      if (!caught) throw fail(FAIL.SAFETY, `${item.label} was accepted by the isolation contract`);
      // `assertIsolation` reports the safety failure and names the specific gap in `extra.code`, so
      // the assertion is on that, not on the generic `FAIL.SAFETY` kind.
      const named = caught.extra?.code ?? caught.code;
      if (named !== item.expect) {
        throw fail(FAIL.SAFETY, `${item.label} was refused as ${named}, expected ${item.expect}`);
      }
      refused.push({ label: item.label, code: named });
    }
    return { baselineAccepted: true, refused, note: 'checked before any process would be started' };
  },
);

/**
 * The fixture's own regression test for the ownership reset, and the reason the case next to it can
 * trust its starting state.
 *
 * `runCase` shares one `dataDir` across the whole run, so `pty-ownership.json` is the only durable
 * thing one case can hand the next. When that happened for real, `mixed-generation-replacement-and-
 * crash` failed at "not keeper-owned" because the preceding case left `keeperOwnershipEnabled=false`
 * and its daemon started under `unchanged`. This case pins the two properties the reset has to have,
 * against a real directory and the real `readOwnershipTable`/`resetOwnershipTable` pair: a table plus
 * a stranded `save()` temp file disappear, and an untouched data dir reads as `null` (the daemon's
 * own answer for "absent", `OwnershipStore::load` -> `OwnershipTable::default()`). It also pins the
 * refusal that keeps the reset from ever naming a path outside the fixture root.
 *
 * No binaries are involved: this is a contract of the harness, so it must not need a keeper to run.
 */
defineCase(
  'ownership-table-reset',
  'the per-case ownership reset removes a table left by a previous case and refuses a path outside the fixture',
  [],
  async (ctx) => {
    // Scratch state lives inside the run's own fixture root, so this self-check writes nowhere the
    // isolation contract has not already accepted.
    const root = join(ctx.paths.root, 'ownership-reset-selftest');
    const dataDir = join(root, 'data');
    await mkdir(dataDir, { recursive: true });
    const paths = { root, dataDir };

    // `readdir` is part of the reset, so an absent data dir must read as "nothing to remove"
    // rather than as a failure: the first case of a run starts here.
    const absent = await resetOwnershipTable({ root, dataDir: join(root, 'not-created') });
    if (absent.removed.length !== 0) throw fail(FAIL.SAFETY, `the reset removed ${JSON.stringify(absent.removed)} from a data dir that does not exist`);

    // A table exactly as a previous case leaves it: the routing flag written off, one keeper row.
    const tablePath = join(dataDir, 'pty-ownership.json');
    await writeFile(
      tablePath,
      `${JSON.stringify({ sessions: { 'left-behind': { sessionId: 'left-behind', owner: 'keeper', pid: 1, epoch: 1, cols: 80, rows: 24, createdAt: 1, checkpointOffset: 0 } }, keeperOwnershipEnabled: false }, null, 2)}\n`,
    );
    // `OwnershipStore::save` stages through a sibling and renames; a daemon killed mid-write leaves
    // that sibling behind, and it is the second half of "no table to inherit".
    const stranded = join(dataDir, 'pty-ownership.tmp-1234-999');
    await writeFile(stranded, '{}\n');
    const beforeReset = await readOwnershipTable(dataDir);
    if (beforeReset?.keeperOwnershipEnabled !== false) {
      throw fail(FAIL.PROTOCOL, `the fixture did not stage a table with the flag off (read back ${JSON.stringify(beforeReset)}), so the reset below would prove nothing`);
    }

    const reset = await resetOwnershipTable(paths);
    const expected = ['pty-ownership.json', 'pty-ownership.tmp-1234-999'];
    if (JSON.stringify([...reset.removed].sort()) !== JSON.stringify(expected.sort())) {
      throw fail(FAIL.SAFETY, `the reset removed ${JSON.stringify(reset.removed)}, expected ${JSON.stringify(expected)}`);
    }
    if ((await readOwnershipTable(dataDir)) !== null) {
      throw fail(FAIL.PROTOCOL, 'a table is still readable after the reset, so the next case would inherit the previous case\'s routing flag');
    }
    if (await exists(tablePath)) throw fail(FAIL.SAFETY, `${tablePath} still exists after the reset`);
    if (await exists(stranded)) throw fail(FAIL.SAFETY, `${stranded} still exists after the reset`);

    // The reset is a write, so it has to obey the isolation contract it is used under: a data dir
    // outside the fixture root is refused before anything is unlinked. `resetOwnershipTable` checks
    // that before it lists the directory, so a path that does not exist still gets the safety answer
    // rather than an ENOENT.
    let refusal = null;
    try {
      await resetOwnershipTable({ root, dataDir: join(tmpdir(), 'qa-p3-not-the-fixture') });
    } catch (error) {
      refusal = error;
    }
    if (!(refusal instanceof AcceptanceError) || refusal.code !== FAIL.SAFETY) {
      throw fail(FAIL.SAFETY, `a data dir outside the fixture root was not refused: ${refusal === null ? 'it was accepted' : String(refusal?.message ?? refusal)}`);
    }

    return {
      removed: reset.removed,
      tableAfterReset: null,
      refusedOutsideRoot: refusal.code,
      note: 'a table left by a previous case, and the temp file a killed daemon would strand, are both gone before the next case spawns',
    };
  },
);

// ---------------------------------------------------------------------------------------------
// Runner
//
// Three outcomes, and only one of them is acceptance:
//   pass     the case asserted its property against the real binaries and it held
//   fail     the case ran and the property did not hold
//   blocked  a named seam is missing, so the case could not be attempted (`GAP.*`)
//
// A blocked case is never reported as a pass. The run exits non-zero for both `fail` and `blocked`,
// because "not accepted" is the answer the operator needs either way.
// ---------------------------------------------------------------------------------------------

const USAGE = `Usage: node scripts/qa/phase3-pty-crash-survival.acceptance.mjs [options]

Runs the Phase 3 PTY crash-survival acceptance cases against the real keeper and daemon binaries.
Every run is isolated: a fresh root under the OS temp directory, its own FERRYX_RUNTIME_DIR and
FERRYX_LEGACY_RUNTIME_DIR, its own keeper socket, and the QA launchd label. Production sessions,
daemons and plists are never read, started, signalled or written.

Options:
  --keeper-binary <path>     ferryx-ptykeeper to run (default: FERRYX_PTY_KEEPER_BIN, then the crate target)
  --daemon-binary <path>     ferryx daemon to run (default: FERRYX_DAEMON_BIN, then the src-tauri target)
  --generations <a,b>        two keeper builds for the generation-compatibility case
  --daemon-generations <a,b> two daemon builds for the app-replacement case; they must speak
                             different protocol versions, or the replacement is not a real upgrade
  --cli-binary <path>        ferryx-cli, to drive 'service rollback' (default: FERRYX_CLI_BIN)
  --sessions <n>             sessions in the SIGKILL gate (default 60)
  --case <id>                run one case; repeatable
  --list                     list case ids and exit
  --keep                     keep the evidence root even when every case passes
  --json                     print the summary as JSON
  -h, --help                 print this text

Exit codes: 0 every selected case passed; 1 any case failed or was blocked; 2 usage error;
3 the run finished but could not release every process or socket it recorded (a harness fault,
reported on stderr as TEARDOWN LEAK and never as a case verdict).`;

function parseArgs(argv) {
  const options = {
    keeperBinary: process.env.FERRYX_PTY_KEEPER_BIN ?? null,
    daemonBinary: process.env.FERRYX_DAEMON_BIN ?? null,
    cliBinary: process.env.FERRYX_CLI_BIN ?? null,
    daemonGenerations: (process.env.FERRYX_DAEMON_GENERATIONS ?? '').split(',').map((value) => value.trim()).filter(Boolean),
    generations: (process.env.FERRYX_PTY_KEEPER_GENERATIONS ?? '').split(',').map((value) => value.trim()).filter(Boolean),
    sessions: 60,
    cases: [],
    list: false,
    keep: false,
    json: false,
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
      case '--keeper-binary':
        options.keeperBinary = take(flag);
        break;
      case '--daemon-binary':
        options.daemonBinary = take(flag);
        break;
      case '--cli-binary':
        options.cliBinary = take(flag);
        break;
      case '--daemon-generations':
        options.daemonGenerations = take(flag).split(',').map((value) => value.trim()).filter(Boolean);
        break;
      case '--generations':
        options.generations = take(flag).split(',').map((value) => value.trim()).filter(Boolean);
        break;
      case '--sessions': {
        const raw = take(flag);
        const parsed = Number(raw);
        if (!Number.isInteger(parsed) || parsed < 1 || parsed > 512) {
          throw fail(FAIL.USAGE, `--sessions must be an integer in 1..512, got ${JSON.stringify(raw)}`);
        }
        options.sessions = parsed;
        break;
      }
      case '--case':
        options.cases.push(take(flag));
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
 * The isolated layout. `mkdtemp` gives a root nobody else can name, and every path the children see is
 * derived from it, so a mistake can only ever point inside it.
 */
function planPaths(root) {
  const runtimeDir = join(root, 'runtime');
  const legacyRuntimeDir = join(root, 'legacy-runtime');
  return {
    root,
    runtimeDir,
    legacyRuntimeDir,
    dataDir: join(root, 'data'),
    sessionDir: join(root, 'sessions'),
    keeperDir: join(root, 'keeper'),
    // Where echo children record the tty mode they set. Inside the fixture root, so the existing
    // isolation assertions cover it too.
    rawTtyDir: join(root, 'tty'),
    // Staged app bundles: one per distinct daemon build, keyed by digest (see `stageDaemonBundle`).
    bundleDir: join(root, 'bundle'),
    // `HOME` for every child. `write_keeper_launchd_plist` creates `$HOME/Library/Logs/Ferryx`, so
    // once the install pass actually runs an inherited real `HOME` would write into the user's home.
    homeDir: join(root, 'home'),
    evidenceDir: join(root, 'evidence'),
    keeperSocketPath: join(runtimeDir, KEEPER_SOCKET_NAME),
    keeperPlistPath: join(root, 'keeper', `${QA_KEEPER_LABEL}.plist`),
    repo: REPO_ROOT,
  };
}

function selectCases(options) {
  if (options.cases.length === 0) return CASES;
  const selected = [];
  for (const id of options.cases) {
    const found = CASES.find((entry) => entry.id === id);
    if (!found) throw fail(FAIL.USAGE, `unknown case ${JSON.stringify(id)}; run with --list to see the ids`);
    if (!selected.includes(found)) selected.push(found);
  }
  return selected;
}

/**
 * Resolves the prerequisites once and records what was actually found. A `requires` entry that did not
 * hold becomes each dependent case's `blocked` outcome, so the report says which seam stopped which
 * case instead of the whole run collapsing.
 */
/**
 * Every constant at the top of this file is a copy of a Rust item, and a copy can go stale. This
 * compares each one against the source it mirrors and refuses to run when they disagree: a harness
 * that speaks a protocol the binaries do not is worse than no harness, because it produces confident
 * verdicts about the wrong wire.
 *
 * Only exact mirrors are compared. Values that are deliberately narrower than the source (a fixture
 * cap, a QA timeout) are not part of the gate.
 */
async function assertNoSourceDrift(facts) {
  const checks = [
    ['keeper magic', KEEPER_MAGIC.toString('ascii'), facts.keeper.magic],
    ['keeper header bytes', KEEPER_HEADER_BYTES, facts.keeper.headerBytes],
    ['keeper framing version', KEEPER_FRAMING_VERSION, facts.keeper.framingVersion],
    ['keeper max payload bytes', KEEPER_MAX_PAYLOAD_BYTES, facts.keeper.maxFramePayloadBytes],
    ['keeper no-request id', KEEPER_NO_REQUEST_ID, facts.keeper.noRequestId],
    ['keeper socket name', KEEPER_SOCKET_NAME, facts.keeper.socketName],
    ['max socket path bytes', MAX_SOCKET_PATH_BYTES, facts.keeper.maxSocketPathBytes],
    ['replay session bytes', REPLAY_SESSION_BYTES, facts.keeper.replaySessionBytes],
    ['replay aggregate bytes', REPLAY_AGGREGATE_BYTES, facts.keeper.replayAggregateBytes],
    ['budget aggregate bytes', BUDGET_AGGREGATE_BYTES, facts.keeper.budgetAggregateBytes],
    ['session reservation bytes', SESSION_RESERVATION_BYTES, facts.keeper.sessionReservationBytes],
    ['keeper max write bytes', KEEPER_MAX_WRITE_BYTES, facts.keeper.maxWriteBytes],
    ['keeper max connections', KEEPER_MAX_CONNECTIONS, facts.keeper.maxConnections],
    ['keeper max checkpoint bytes', KEEPER_MAX_CHECKPOINT_BYTES, facts.keeper.maxCheckpointBytes],
    ['keeper attach inline max bytes', KEEPER_ATTACH_INLINE_MAX_BYTES, facts.keeper.attachInlineMaxBytes],
    ['keeper dir mode', KEEPER_DIR_MODE, facts.keeper.dirMode],
    ['keeper socket mode', KEEPER_SOCKET_MODE, facts.keeper.socketMode],
    ['daemon protocol version', DAEMON_PROTOCOL_VERSION, facts.daemon.protocolVersion],
    ['qa keeper label', QA_KEEPER_LABEL, facts.keeper.qaLabel],
    ['production keeper label', PRODUCTION_KEEPER_LABEL, facts.keeper.productionLabel],
  ];
  const drifted = checks.filter(([, harnessValue, sourceValue]) => {
    if (sourceValue === null || sourceValue === undefined) return true;
    return String(harnessValue) !== String(sourceValue);
  });

  // `KIND` is a full table, so a missing or renumbered verb is drift even when every constant above
  // matches. Comparing the whole set keeps a newly added verb from silently becoming unknown.
  const sourceKinds = new Map((facts.keeper.kinds ?? []).map((entry) => [entry.name, entry.value]));
  const kindDrift = [];
  for (const [name, value] of Object.entries(KIND)) {
    const source = sourceKinds.get(name);
    if (source === undefined) kindDrift.push(`${name} is not a keeper Kind (harness says 0x${value.toString(16)})`);
    else if (source !== value) kindDrift.push(`${name} is 0x${source.toString(16)} in protocol.rs, 0x${value.toString(16)} here`);
  }
  for (const name of sourceKinds.keys()) {
    if (!(name in KIND)) kindDrift.push(`${name} is a keeper Kind the harness does not know`);
  }

  if (drifted.length || kindDrift.length) {
    const detail = [
      ...drifted.map(([label, harnessValue, sourceValue]) => `${label}: harness ${JSON.stringify(harnessValue)}, source ${JSON.stringify(sourceValue)}`),
      ...kindDrift,
    ].join('; ');
    throw fail(FAIL.SOURCE_DRIFT, detail, { drifted, kindDrift });
  }
  return {
    checked: checks.length,
    kinds: sourceKinds.size,
    protocolMajor: facts.keeper.protocolMajor,
    protocolMinor: facts.keeper.protocolMinor,
    liveStartTimeIsMillis: facts.daemon.liveStartTimeIsMillis,
  };
}

async function buildPrerequisites(ctx, options, binaries) {
  const state = {
    keeper: null,
    daemon: binaries.daemonBinary ? null : GAP.DAEMON_BINARY_ABSENT,
    ps: (await exists('/bin/ps')) ? null : GAP.HELPER_ABSENT,
    keeperIntegration: null,
    livePush: null,
    handoverKeeper: GAP.HANDOVER_KEEPER_UNPROVEN,
    handoverPath: GAP.HANDOVER_STEPS_ABSENT,
    ownershipFlag: GAP.OWNERSHIP_FLAG_ABSENT,
    ownershipEnable: false,
    rollback: GAP.ROLLBACK_REFUSAL_ABSENT,
    generations: (binaries.generationPlan?.count ?? options.generations.length) < 2 ? GAP.KEEPER_GENERATIONS_MISSING : null,
    // Two daemon builds are what make an "app replacement" real; one binary run twice is a restart.
    // The distinct-version check needs both processes started, so it is a typed gap inside the case
    // rather than a guess here.
    daemonGenerations: (options.daemonGenerations ?? []).length < 2 ? GAP.DAEMON_GENERATIONS_MISSING : null,
    cli: binaries.cliBinary ? null : GAP.CLI_BINARY_ABSENT,
  };

  // The ownership/migration/rollback/handover prerequisites are facts about the tree, not about a
  // running process, so they are read from the real source instead of being asserted by hand. A
  // hardcoded gap would keep saying "absent" after the seam landed, and a hardcoded pass would
  // claim a seam that does not exist; neither is a measurement.
  let facts = null;
  try {
    facts = await readSourceFacts(ctx.paths.repo);
    const drift = await assertNoSourceDrift(facts);
    state.sourceFacts = drift;
    const surfaces = facts.surfaces;
    // Note: absence of FD migration is NOT a gap for coexistence. The plan (lines 149-152) has
    // existing direct-owned sessions *drain* under their original owner while new spawns go to the
    // keeper, so both types coexist by ordering alone. Only a case that had to move a live master
    // would need `fdMigrationPresent`, and no case here does.
    // A flag exists only if the daemon's spawn path consults it. `keeper_session.rs` defines
    // `keeper_ownership_enabled`, but a module no wired file references is never consulted at
    // runtime, so the flag is not reachable from a session.
    //
    // Enable and disable are judged separately because they are different seams: enable is the
    // desktop start pass calling `activate_new_sessions`, disable is `OwnershipPolicy` reading
    // `FERRYX_KEEPER_OWNERSHIP` at daemon start and `reconcile_and_activate` writing the flag off.
    // A tree with only the enable half can still produce keeper-owned sessions, so collapsing the two
    // would either hide a working enable path or claim a disable path that does not exist.
    const flagReachable = surfaces.ownershipFlagPresent && surfaces.keeperSessionWired.length > 0;
    state.ownershipEnable = flagReachable && surfaces.ownershipEnableSurface;
    state.ownershipFlag =
      flagReachable && surfaces.ownershipEnableSurface && surfaces.ownershipDisableSurface
        ? null
        : GAP.OWNERSHIP_FLAG_ABSENT;
    // The rollback refusal is reachable only when a production CLI path gets to the gate. `daemon::
    // rollback::run_rollback` is that path and `ferryx-cli service rollback` calls it, so both halves
    // are required; a gate nobody invokes would report a refusal no user could ever see.
    if (surfaces.rollbackGateDefined && surfaces.rollbackCliReachesGate) state.rollback = null;
    // A rollback refusal is observable only through a verb a user can invoke; the pure
    // `rollback_gate` with no call site outside its own tests is not one.
    if (surfaces.rollbackGateDefined && (surfaces.rollbackGateCallSitesOutsideDefinition > 0 || surfaces.rollbackCliVerb)) {
      state.rollback = null;
    }
    // The graceful path can only hand keeper-owned sessions over if it names them at all.
    if (surfaces.handoverKeeperMentions > 0) state.handoverKeeper = null;
    // The graceful case drives prepare/transfer/commit plus `--handover-from`, which are independent
    // of the keeper. Without all four there is no graceful path to drive at all.
    if (surfaces.handoverVerbsPresent) state.handoverPath = null;
    await ctx.evidence.record({ step: 'source-facts', drift, surfaces });
  } catch (error) {
    // Drift is a harness bug, not a product verdict: it must stop the run, not become a gap that
    // lets the isolation self-test pass while every constant below it is wrong.
    if (error instanceof AcceptanceError) throw error;
    // A tree that cannot be read is not a product verdict, but it does block every case whose
    // prerequisite comes from it. Report it as such rather than silently keeping the gaps.
    const detail = error instanceof SourceFactError ? `${error.code}: ${error.detail}` : String(error?.message ?? error);
    state.sourceFacts = detail;
    state.ownershipFlag = `${GAP.OWNERSHIP_FLAG_ABSENT} (source facts unreadable: ${detail})`;
    state.rollback = `${GAP.ROLLBACK_REFUSAL_ABSENT} (source facts unreadable: ${detail})`;
    state.handoverKeeper = GAP.HANDOVER_KEEPER_UNPROVEN;
    await ctx.evidence.record({ step: 'source-facts', error: detail });
  }

  if (binaries.keeperBinary) {
    try {
      await ctx.startKeeper();
    } catch (error) {
      // The binary was found, so "absent" would be the wrong gap: it started and did not become
      // usable, which is a different thing to fix.
      state.keeper = `${GAP.KEEPER_START_FAILED}: ${error?.detail ?? error?.message ?? error}`;
    }
  } else {
    state.keeper = GAP.KEEPER_BINARY_ABSENT;
  }

  // Live push is a property of the keeper alone, so it is measured even when the daemon is absent.
  if (state.keeper === null) {
    try {
      const push = await probeLivePush(ctx);
      state.livePush = push.push ? null : GAP.LIVE_PUSH_ABSENT;
      await ctx.evidence.record({ probe: 'live-push', outcome: push });
    } catch (error) {
      state.livePush = `${GAP.LIVE_PUSH_ABSENT} (probe failed: ${error?.detail ?? error?.message ?? error})`;
      await ctx.evidence.record({ probe: 'live-push', outcome: { error: String(error?.message ?? error) } });
    }
  } else {
    state.livePush = GAP.LIVE_PUSH_ABSENT;
  }

  // The decisive seam: does a daemon-spawned session show up in the keeper's registry?
  if (state.keeper === null && state.daemon === null) {
    try {
      ctx.daemon = await ctx.startDaemon({ role: 'initial' });
      const integration = await probeKeeperIntegration(ctx);
      // `detail` carries the measured reason (which seam, from what evidence); the bare code is only
      // the fallback when a probe answers without one. Reporting the constant alone is what made
      // every blocked case repeat a sentence that had stopped being true.
      state.keeperIntegration = integration.ok ? null : integration.detail ?? integration.code;
      await ctx.evidence.record({ probe: 'keeper-integration', outcome: integration });
    } catch (error) {
      state.keeperIntegration = `${GAP.KEEPER_INTEGRATION_ABSENT} (${error?.detail ?? error?.message ?? error})`;
      await ctx.evidence.record({ probe: 'keeper-integration', outcome: { ok: false, error: String(error?.message ?? error) } });
    }
  } else {
    // Neither probe could run: no keeper, or no daemon binary to spawn through. Neither is a verdict
    // about keeper routing, so the wording says so instead of naming a missing call site.
    state.keeperIntegration = `${GAP.KEEPER_INTEGRATION_ABSENT} (not measured: ${state.keeper ?? state.daemon})`;
  }

  return state;
}

async function runCase(entry, options, paths, prerequisites) {
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

  const evidence = new Evidence(paths.evidenceDir);
  const ctx = new Context(options, paths, evidence, {
    keeperBinary: prerequisites.keeperBinary,
    daemonBinary: prerequisites.daemonBinary,
    cliBinary: prerequisites.cliBinary,
    generations: options.generations,
  });
  const started = Date.now();
  let outcome;
  try {
    // The ownership table is the one durable thing the fixture root shares between cases (one
    // `dataDir` per run), so it is reset before this case spawns anything: every case then starts from
    // "no table", and any flag it reads was written by its own daemon. Done before the keeper and the
    // daemon so the reset can neither race a start pass nor erase state a running daemon just wrote.
    const ownershipReset = await resetOwnershipTable(paths);
    await evidence.record({ case: entry.id, step: 'ownership-table-reset', ...ownershipReset });
    // Each case gets its own keeper process: a case that kills a daemon, overflows a replay log, or
    // leaves sessions behind must not be able to influence the next one. A case that needs no keeper
    // (the isolation self-test) starts none, so the check is honest about what it exercises.
    if (entry.requires.includes('keeper')) await ctx.startKeeper();
    // A daemon is started only for cases that declared one, and only after the keeper is up: a
    // daemon that starts first would find no keeper socket and the case would measure a wiring
    // failure it did not mean to test.
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
  } finally {
    // Nothing this run created may outlive it, but the evidence is written before the teardown so a
    // teardown failure cannot erase the record of what happened. A non-pass outcome additionally
    // carries what the daemon and keeper wrote to their own streams: without that a timeout says only
    // that this harness stopped waiting, and the reason the product is silent is lost with the process.
    outcome.durationMs = Date.now() - started;
    outcome.startedAt = startedAt;
    if (outcome.outcome !== 'pass') {
      outcome.processStreams = await captureProcessStreams(ctx, entry.id, {
        daemon: ctx.daemonProcess,
        keeper: ctx.keeperProcess,
        ...(ctx.daemonProcesses.length > 1
          ? Object.fromEntries(
              ctx.daemonProcesses.slice(0, -1).map((process, index) => [`daemon-generation-${index}`, process]),
            )
          : {}),
        ...(ctx.keeperProcesses.length > 1
          ? Object.fromEntries(
              ctx.keeperProcesses.slice(0, -1).map((process, index) => [`keeper-generation-${index}`, process]),
            )
          : {}),
      }).catch((error) => ({ error: String(error?.stack ?? error) }));
      // The daemon's own `tracing` output lands in this file rather than on stderr, so the streams
      // above can be silent while the reason sits here. Captured as its tail: the interesting lines are
      // the last ones before the failure, and the file is shared by every daemon this run started.
      outcome.daemonLog = await evidence
        .daemonLog(paths.dataDir)
        .then(async (log) => {
          if (log.text === null) return { path: log.path, present: false };
          const tail = log.text.slice(-32_768);
          const captured = await evidence.captureRaw(join('processes', `${entry.id}-daemon.log`), tail);
          return { path: log.path, present: true, bytes: log.text.length, tail: captured };
        })
        .catch((error) => ({ error: String(error?.stack ?? error) }));
    }
    await evidence.writeCase(entry.id, outcome).catch(() => null);
    await ctx.reapKeeperSessions().catch(() => null);
    await ctx.shutdown().catch(() => null);
  }
  return outcome;
}

function summarize(outcomes) {
  const count = (name) => outcomes.filter((row) => row.outcome === name).length;
  return {
    total: outcomes.length,
    passed: count('pass'),
    failed: count('fail'),
    blocked: count('blocked'),
    accepted: outcomes.length > 0 && count('pass') === outcomes.length,
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
    }
    if (row.outcome === 'pass' && row.result?.note) lines.push(`        ${row.result.note}`);
    if (row.durationMs) lines.push(`        ${row.durationMs}ms`);
  }
  lines.push('');
  lines.push(`${summary.passed} passed, ${summary.failed} failed, ${summary.blocked} blocked of ${summary.total} cases`);
  // "Accepted" is a claim about the crash-survival gate, and that gate is only meaningful if a real
  // keeper or daemon actually ran. Saying it for a self-test-only selection would be a false pass.
  // `ownership-table-reset` is a harness contract like `isolation-guard`: it exercises the fixture's
  // own reset, never a supplied binary, so it cannot be what makes a run "exercised" either.
  const selfTests = new Set(['isolation-guard', 'ownership-table-reset']);
  const exercised = outcomes.some((row) => row.outcome === 'pass' && !selfTests.has(row.id));
  if (!summary.accepted) {
    lines.push('NOT ACCEPTED: the Phase 3 gate does not hold');
  } else if (exercised) {
    lines.push('ACCEPTED: every selected case passed against the supplied keeper/daemon binaries');
  } else {
    lines.push('SELF-CHECK ONLY: the harness contract holds, but no crash-survival case ran');
  }
  if (!binaries?.keeperBinary || !binaries?.daemonBinary) {
    lines.push(
      `prerequisites: keeper ${binaries?.keeperBinary ?? 'missing'}, daemon ${binaries?.daemonBinary ?? 'missing'}`,
    );
  }
  return lines.join('\n');
}

export async function main(argv = process.argv.slice(2)) {
  let options;
  try {
    options = parseArgs([...argv]);
  } catch (error) {
    process.stderr.write(`${error.message}\n\n${USAGE}\n`);
    return 2;
  }
  if (options.help) {
    process.stdout.write(`${USAGE}\n`);
    return 0;
  }
  if (options.list) {
    for (const entry of CASES) {
      process.stdout.write(`${entry.id}\n    ${entry.title}\n    requires: ${entry.requires.join(', ') || '(nothing)'}\n`);
    }
    return 0;
  }

  let selected;
  try {
    selected = selectCases(options);
  } catch (error) {
    process.stderr.write(`${error.message}\n\n${USAGE}\n`);
    return 2;
  }

  // A UDS path is capped at 103 bytes (`endpoint::MAX_SOCKET_PATH_BYTES`), and the daemon binds
  // `runtime/legacy-<pid>-<id>.sock` under this root. On macOS `tmpdir()` is the long
  // `/var/folders/.../T`, which leaves no room and the bind fails with SUN_LEN; `/tmp` is short and
  // on the same filesystem. Elsewhere `tmpdir()` stays.
  const root = await mkdtemp(join(process.platform === 'darwin' ? '/tmp' : tmpdir(), 'ferryx-phase3-'));
  runRoot = root;
  runKeepRoot = options.keep === true;
  const paths = planPaths(root);
  rawTtyDir = paths.rawTtyDir;
  // Isolation is asserted before a single process is spawned, from the paths that will be used.
  const isolation = assertIsolation(paths);
  for (const dir of [paths.runtimeDir, paths.legacyRuntimeDir, paths.dataDir, paths.sessionDir, paths.keeperDir, paths.rawTtyDir, paths.homeDir, paths.bundleDir, paths.evidenceDir]) {
    await mkdir(dir, { recursive: true });
  }

  const evidence = new Evidence(paths.evidenceDir);
  runEvidence = evidence;
  await evidence.init();
  await evidence.record({
    run: 'phase3-pty-crash-survival',
    step: 'started',
    argv: [...argv],
    platform: process.platform,
    node: process.version,
    root,
    paths,
    isolation,
    launchd: plannedLaunchdIdentity(paths),
  });

  const binaries = await preflight(options, paths);

  // A supplied keeper is one generation even when `--generations` is unset. Seeding it keeps the
  // generation count honest ("got 1, need two builds") instead of claiming zero while a binary ran.
  if (options.generations.length === 0 && binaries.keeperBinary) options.generations = [binaries.keeperBinary];
  // Which generations this run can claim, decided before preflight publishes the gaps so the count it
  // reports and the binaries the cases receive are the same fact.
  const generationPlan = await planKeeperGenerations({ options, paths, binaries, evidence });
  binaries.gaps = binaries.gaps.filter((entry) => entry !== GAP.KEEPER_GENERATIONS_MISSING);
  if (generationPlan.gap) binaries.gaps.push(generationPlan.gap);
  options.generations = generationPlan.generations;
  binaries.generationPlan = {
    synthesized: generationPlan.synthesized,
    identicalBytes: generationPlan.identicalBytes,
    count: generationPlan.generations.length,
  };
  await evidence.record({ step: 'preflight', binaries, gaps: binaries.gaps });

  const prerequisites = { keeperBinary: binaries.keeperBinary, daemonBinary: binaries.daemonBinary };
  // `cli` is the case's own prerequisite name; the binary it needs travels beside it so the case does
  // not have to re-resolve a path the preflight already resolved.
  prerequisites.cliBinary = binaries.cliBinary;
  const probeContext = new Context(options, paths, evidence, {
    keeperBinary: binaries.keeperBinary,
    daemonBinary: binaries.daemonBinary,
    cliBinary: binaries.cliBinary,
    generations: options.generations,
  });
  try {
    Object.assign(prerequisites, await buildPrerequisites(probeContext, options, binaries));
  } finally {
    // In a `finally`: when `buildPrerequisites` throws, the probe's keeper and daemon are already
    // running. Awaiting the shutdown only on the success path orphaned both, and the top-level catch
    // then reported a harness error while a daemon stayed up.
    await probeContext.shutdown().catch(() => null);
  }

  const outcomes = [];
  for (const entry of selected) {
    const outcome = await runCase(entry, options, paths, prerequisites);
    outcomes.push(outcome);
    // Streamed as it happens: a case that wedges should be visible before the run finishes.
    if (!options.json) process.stdout.write(`${outcome.outcome === 'pass' ? 'PASS   ' : outcome.outcome === 'fail' ? 'FAIL   ' : 'BLOCKED'} ${outcome.id}\n`);
  }

  const summary = summarize(outcomes);
  const report = { run: 'phase3-pty-crash-survival', root, isolation, binaries, prerequisites, summary, outcomes };
  await evidence.writeCase('summary', report);
  await evidence.record({ step: 'finished', summary });

  if (options.json) {
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  } else {
    process.stdout.write(`\n${render(outcomes, summary, binaries)}\n`);
    process.stdout.write(`\nevidence: ${paths.evidenceDir}\n`);
  }

  // The fixture root is deliberately NOT deleted here. `finishRun` deletes it after teardown, and only
  // when nothing leaked: `rm` before returning would destroy the evidence of a run that then turned
  // out to have left a process behind, and a survivor's pid would point at a directory already gone.
  return summary.accepted ? 0 : 1;
}

/**
 * Plan rev 5, lines 162-164, as one ordered sequence rather than as separate cases:
 *
 *   "an enable/disable/rollback harness with **both ownership types present at once** -- after
 *    disabling the flag, existing keeper sessions still attach; a rollback attempt is refused while a
 *    keeper session lives and succeeds once it is gone."
 *
 * The steps are dependent, which is why they are one case: the refusal must name the count from the
 * *same* table that later lets the rollback through, and "succeeds once gone" is only meaningful if
 * the session that blocked it is the one that ended. Split across cases, each would need its own
 * fixture and the sequence itself would never be exercised.
 *
 *  1. both owners live: a direct session (spawned before the flag) and a keeper session (after).
 *  2. roll back to a build without keeper support: refused, typed, naming the live keeper count.
 *  3. end the keeper session, restart so the vanished row is written off, then roll back again: it
 *     must now succeed.
 *  4. disable the flag with `FERRYX_KEEPER_OWNERSHIP=disabled` on a restart, then attach a keeper
 *     session created while the flag was still on. It runs last so a gap here cannot preempt stages
 *     1-3, and it is guarded on the measured `ownershipDisableSurface` so a tree without the disable
 *     path still reports the gap instead of skipping the stage.
 *
 * Stage 3 is the half no existing case covered, and it is drivable in isolation: `plan_rollback`
 * resolves generations from `daemon_runtime_root()`, i.e. `<FERRYX_DATA_DIR>/daemon-runtime`, and
 * `apply_rollback` writes only plists under `launch_agents_dir` plus `$HOME/Library/Logs/Ferryx`,
 * both of which this case points into the fixture.
 */
defineCase(
  'enable-disable-rollback-sequence',
  'both owners live, rollback refused with the count, succeeds once the keeper session ends, and a disable restart still serves a keeper session',
  ['keeper', 'daemon', 'keeperIntegration', 'ownershipFlag', 'rollback', 'cli'],
  async (ctx) => {
    let surfaces;
    try {
      surfaces = (await readSourceFacts(ctx.paths.repo)).surfaces;
    } catch (error) {
      const detail = error instanceof SourceFactError ? `${error.code}: ${error.detail}` : String(error?.message ?? error);
      throw gap(GAP.OWNERSHIP_FLAG_ABSENT, `${GAP.OWNERSHIP_FLAG_ABSENT}: the source surfaces could not be read (${detail})`);
    }
    const missing = [];
    if (!surfaces.ownershipEnableSurface) missing.push('ownershipEnableSurface');
    if (!surfaces.keeperSessionWired?.length) missing.push('keeperSessionWired');
    if (!surfaces.rollbackGateDefined) missing.push('rollbackGateDefined');
    if (!surfaces.rollbackCliReachesGate) missing.push('rollbackCliReachesGate');
    if (missing.length) {
      await ctx.evidence.record({ case: 'enable-disable-rollback-sequence', missing });
      throw gap(
        GAP.ROLLBACK_REFUSAL_ABSENT,
        `${GAP.ROLLBACK_REFUSAL_ABSENT}: absent ${missing.join(', ')}, so the sequence has no seam to drive`,
      );
    }

    const { bundledDaemon } = await prepareKeeperBundle(ctx.paths, ctx.binaries.daemonBinary, ctx.binaries.keeperBinary);
    await ctx.killDaemon('SIGTERM');

    // Step 1a: a direct-owned session, spawned while the persisted flag is off.
    //
    // The flag lives in `planPaths(root).dataDir`, which is one directory per RUN, not per case, so a
    // flag left by an earlier case would route this spawn to the keeper and the case would fail with
    // "<id> is keeper-owned before the flag was ever enabled" (run12). `runCase` resets that table
    // before every case, and `FERRYX_KEEPER_OWNERSHIP=disabled` pins the state this step needs on top
    // of that (`keeper_startup.rs::OwnershipPolicy::from_env`), the same way `mixed-ownership-drain`
    // does it above. Disabling is a routing rule for *new* sessions only: it still attaches keeper
    // sessions that already exist, so the later stages are untouched.
    ctx.daemon = await ctx.startDaemon({ role: 'seq-direct', binary: bundledDaemon, ownership: 'disabled' });
    const tableBeforeEnable = await readOwnershipTable(ctx.paths.dataDir);
    if (tableBeforeEnable?.keeperOwnershipEnabled !== false) {
      throw fail(
        FAIL.PROTOCOL,
        `${KEEPER_OWNERSHIP_ENV}=disabled did not leave the routing flag off (table: ${JSON.stringify(tableBeforeEnable)}), so a direct-owned session cannot be created`,
      );
    }
    const directId = randomUUID();
    await ctx.daemon.spawn({ sessionId: directId, workspaceId: ctx.workspaceId, cwd: ctx.paths.repo });
    if ((await ctx.keeper.ownership()).some((entry) => entry.sessionId === directId)) {
      throw fail(FAIL.IDENTITY_MISMATCH, `${directId} is keeper-owned before the flag was ever enabled`);
    }

    // Step 1b: enabling is a daemon-start boundary, and the direct session must survive it, so the
    // restart is the real replacement path rather than a kill.
    const prepared = await ctx.daemon.prepareHandover();
    if (!prepared.legacySocketPath) {
      throw fail(FAIL.PROTOCOL, 'prepareHandover returned no legacy socket path for the enabling successor');
    }
    if (!prepared.activeSessions.includes(directId)) {
      throw fail(FAIL.MISSING_OUTPUT, `prepareHandover listed ${JSON.stringify(prepared.activeSessions)} without ${directId}`);
    }
    ctx.daemon = await ctx.startDaemon({
      role: 'seq-keeper',
      binary: bundledDaemon,
      handoverFrom: prepared.legacySocketPath,
    });
    // Everything below reads the flag, the keeper's registry and the daemon's own lists; a successor
    // that has not committed yet would have a predecessor answer those reads.
    await awaitSuccessorServing(ctx, 'the enabling successor serves the canonical socket');
    const tableAfterEnable = await readOwnershipTable(ctx.paths.dataDir);
    if (tableAfterEnable?.keeperOwnershipEnabled !== true) {
      const unavailable = await keeperStartPassUnavailable(ctx);
      throw gap(
        unavailable?.code ?? GAP.OWNERSHIP_FLAG_ABSENT,
        `${unavailable?.detail ?? GAP.OWNERSHIP_FLAG_ABSENT}: the daemon did not enable keeper ownership on restart (table: ${JSON.stringify(tableAfterEnable)})`,
      );
    }
    const keeperId = randomUUID();
    await ctx.daemon.spawn({ sessionId: keeperId, workspaceId: ctx.workspaceId, cwd: ctx.paths.repo });
    const keeperEntry = (await ctx.keeper.ownership()).find((entry) => entry.sessionId === keeperId);
    if (!keeperEntry) {
      const routing = await keeperRoutingGap(ctx);
      throw gap(routing.code, `${routing.detail}: ${keeperId} was spawned with the flag on but is not in the keeper's registry`);
    }
    // Both ownership types are live under one daemon -- the plan's precondition, asserted rather than
    // assumed before anything is disabled.
    const bothLive = await readOwnershipTable(ctx.paths.dataDir);
    if (bothLive?.sessions?.[keeperId]?.owner !== 'keeper') {
      throw fail(FAIL.PROTOCOL, `${keeperId} is not recorded as keeper-owned in ${JSON.stringify(bothLive?.sessions?.[keeperId])}`);
    }
    if (bothLive?.sessions?.[directId] !== undefined) {
      throw fail(FAIL.PROTOCOL, `${directId} was written into the ownership table, so the two owners are not distinct`);
    }

    // Step 2: refused while the keeper session lives. A manifest that records no floor means only the
    // ownership rule applies; a missing one would refuse for `FloorUnverified` instead, which is a
    // different contract.
    const manifestPath = join(ctx.paths.keeperDir, 'rollback-manifest.json');
    await writeFile(manifestPath, '{}\n');
    const targetVersion = '1900.101.1';
    const refused = await runServiceRollback(ctx, targetVersion, { FERRYX_ROLLBACK_MANIFEST: manifestPath });
    if (refused.exit.code !== 1) {
      throw fail(
        FAIL.PROTOCOL,
        `rollback to ${targetVersion} exited ${refused.exit.code}, expected 1 (refusal) while ${keeperId} lives: ${refused.supervised.stderr.slice(-2_048)}`,
      );
    }
    let refusal;
    try {
      refusal = JSON.parse(refused.supervised.stderr.trim().split('\n').pop());
    } catch {
      throw fail(FAIL.PROTOCOL, `the refusal was not JSON on stderr: ${refused.supervised.stderr.slice(-2_048)}`);
    }
    if (refusal?.code !== 'KEEPER_ROLLBACK_REFUSED') {
      throw fail(FAIL.REFUSAL_CODE, `refused with ${JSON.stringify(refusal?.code)}, not KEEPER_ROLLBACK_REFUSED`);
    }
    const claimed = /(\d+) session\(s\)/.exec(String(refusal.message ?? ''));
    if (!claimed) throw fail(FAIL.MISSING_OUTPUT, `the refusal names no session count: ${JSON.stringify(refusal.message)}`);
    const counted = Number(claimed[1]);
    const liveOwned = (await ctx.keeper.ownership()).length;
    if (counted !== liveOwned) {
      throw fail(FAIL.BYTE_ACCOUNTING, `the refusal names ${counted} keeper-owned session(s), the keeper reports ${liveOwned}`);
    }

    // Step 3: end the keeper session, then roll back again. Two things must happen: the session must
    // be gone from the keeper's registry, and the daemon must write off the vanished row from the
    // durable table, because that table -- not the live registry -- is what `durable_ownership` counts.
    await ctx.keeper.kill(keeperId, keeperEntry.epoch);
    const stillRegistered = (await ctx.keeper.ownership()).some((entry) => entry.sessionId === keeperId);
    if (stillRegistered) {
      throw fail(FAIL.PROTOCOL, `${keeperId} is still in the keeper registry after kill, so the rollback would still be refused`);
    }
    // `startup_discovery` prunes keeper rows whose session the keeper no longer holds, so a restart is
    // what converts "the session ended" into "the table says zero".
    await ctx.killDaemon('SIGTERM');
    ctx.daemon = await ctx.startDaemon({ role: 'seq-after-end', binary: bundledDaemon });
    const afterEnd = await readOwnershipTable(ctx.paths.dataDir);
    if (afterEnd?.sessions?.[keeperId] !== undefined) {
      throw fail(
        FAIL.PROTOCOL,
        `${keeperId} is still recorded in the ownership table after the session ended (${JSON.stringify(afterEnd?.sessions?.[keeperId])}), so the rollback would still see it`,
      );
    }

    // The target generation must exist for `plan_rollback` to resolve it; the refusal above came from
    // the ownership gate, which is checked before the target is looked up, so staging it now does not
    // weaken step 3.
    const staged = await stageGeneration(ctx.paths, targetVersion, ctx.binaries.daemonBinary, '0123abcd');
    const allowed = await runServiceRollback(ctx, targetVersion, { FERRYX_ROLLBACK_MANIFEST: manifestPath });
    if (allowed.exit.code !== 0) {
      throw fail(
        FAIL.PROTOCOL,
        `rollback to ${targetVersion} exited ${allowed.exit.code} with no keeper session left, expected 0: ${allowed.supervised.stderr.slice(-2_048)}`,
      );
    }
    let report;
    try {
      report = JSON.parse(allowed.supervised.stdout.trim().split('\n').pop());
    } catch {
      throw fail(FAIL.PROTOCOL, `the successful rollback printed no JSON report: ${allowed.supervised.stdout.slice(-2_048)}`);
    }
    // The plist is the observable effect: `apply_rollback` names the staged generation for login
    // launch and removes load-time launch from the others.
    const plists = await readdir(allowed.agentsDir).catch(() => []);
    if (!plists.some((name) => name.endsWith('.plist'))) {
      throw fail(FAIL.MISSING_OUTPUT, `the successful rollback wrote no plist into ${allowed.agentsDir}`);
    }
    const written = await readFile(join(allowed.agentsDir, plists.find((name) => name.endsWith('.plist'))), 'utf8');
    if (!written.includes(staged.key)) {
      throw fail(
        FAIL.MISSING_OUTPUT,
        `the written plist does not name the rolled-back generation ${staged.key}: ${written.slice(0, 1_024)}`,
      );
    }

    // Step 4: disable the flag, then attach a keeper session. `attach` keys on the session's *stored
    // owner* (`keeper_session.rs::attach` refuses only when `owner != Keeper`), never on the flag, so
    // a disabled daemon must still serve a keeper session. This runs last on purpose: the rollback
    // rule above is "while a keeper session exists" and does not depend on the flag, so putting this
    // stage last keeps stages 1-3 genuinely exercised instead of preempted by a gap.
    //
    // The keeper session from step 1 was ended to let the rollback succeed, so the attach target has
    // to be a new keeper session created while the flag is still on.
    //
    // `FERRYX_KEEPER_OWNERSHIP=disabled` is the production disable: `OwnershipPolicy::from_env` reads
    // it at daemon start and `reconcile_and_activate` applies it after attaching. A tree without that
    // seam keeps the gap instead of reporting a stage it never drove.
    if (!surfaces.ownershipDisableSurface) {
      await ctx.evidence.record({
        case: 'enable-disable-rollback-sequence',
        step: 'disable',
        absent: 'ownershipDisableSurface',
        driven: ['both-owners-live', 'rollback-refused-with-count', 'rollback-succeeded-once-ended'],
        note: `nothing in production clears the keeper flag (no startup pass reads ${KEEPER_OWNERSHIP_ENV}=disabled), so disable-then-attach is undriven`,
      });
      throw gap(
        GAP.OWNERSHIP_FLAG_ABSENT,
        `${GAP.OWNERSHIP_FLAG_ABSENT}: no startup pass disables the keeper flag, so "after disabling the flag, existing keeper sessions still attach" cannot be driven. The other three stages ran and are recorded in evidence`,
      );
    }
    const strandId = randomUUID();
    await ctx.daemon.spawn({ sessionId: strandId, workspaceId: ctx.workspaceId, cwd: ctx.paths.repo });
    const strandEntry = (await ctx.keeper.ownership()).find((entry) => entry.sessionId === strandId);
    if (!strandEntry) {
      const routing = await keeperRoutingGap(ctx);
      throw gap(
        routing.code,
        `${routing.detail}: ${strandId} was spawned with the flag on but is not in the keeper's registry, so there is no keeper session to strand`,
      );
    }
    const beforeDisable = await readOwnershipTable(ctx.paths.dataDir);
    if (beforeDisable?.sessions?.[strandId]?.owner !== 'keeper') {
      throw fail(
        FAIL.PROTOCOL,
        `${strandId} is recorded as ${JSON.stringify(beforeDisable?.sessions?.[strandId] ?? null)} before the disable, expected a keeper row`,
      );
    }

    // SIGTERM is `shutdown_gracefully`, not a handover: the daemon yields its socket without
    // transferring ownership, which is what leaves the keeper holding `strandId` for the successor.
    await ctx.killDaemon('SIGTERM');
    ctx.daemon = await ctx.startDaemon({
      role: 'seq-disabled',
      binary: bundledDaemon,
      ownership: 'disabled',
    });
    const afterDisable = await readOwnershipTable(ctx.paths.dataDir);
    if (afterDisable?.keeperOwnershipEnabled !== false) {
      throw fail(
        FAIL.PROTOCOL,
        `${KEEPER_OWNERSHIP_ENV}=disabled did not clear the new-session routing flag (table: ${JSON.stringify(afterDisable)})`,
      );
    }
    const stranded = await until(`the disabled daemon lists ${strandId}`, 30_000, async () => {
      const rows = await ctx.daemon.liveSessions();
      return rows.find((row) => row.backendId === strandId) ?? null;
    });
    if (stranded.localChildPid !== strandEntry.pid) {
      throw fail(
        FAIL.IDENTITY_MISMATCH,
        `${strandId}: pid changed ${strandEntry.pid} -> ${stranded.localChildPid} across the disable restart, so the session was respawned instead of reattached`,
      );
    }
    if (afterDisable?.sessions?.[strandId]?.owner !== 'keeper') {
      throw fail(
        FAIL.PROTOCOL,
        `${strandId} is recorded as ${JSON.stringify(afterDisable?.sessions?.[strandId] ?? null)} after the disable; the flip must not rewrite an existing owner`,
      );
    }
    const strandedAttachment = await ctx.attachSession(strandId, null);
    if (strandedAttachment.gap) {
      throw fail(
        FAIL.GAP_IN_NO_GAP_CASE,
        `attach of ${strandId} after the disable reported a gap: ${JSON.stringify(strandedAttachment.gap)}`,
      );
    }
    const strandedSession = new AttachedSession(strandedAttachment, ctx.evidence);
    try {
      const ping = Buffer.from(`SEQ-DISABLED:${nonce()}\n`);
      await writeToSession(ctx, strandId, strandedSession, ping, 'echo from the keeper session after the disable', 30_000);
    } finally {
      strandedSession.attachment.stream.close();
    }

    await ctx.evidence.record({
      case: 'enable-disable-rollback-sequence',
      directId,
      keeperId,
      targetVersion,
      stagedKey: staged.key,
      refusalCode: refusal.code,
      counted,
      plists,
      report,
      ownershipEnv: `${KEEPER_OWNERSHIP_ENV}=disabled`,
      disabledSession: strandId,
      disabledSessionPid: strandEntry.pid,
    });
    return {
      directId,
      keeperId,
      counted,
      refusalCode: refusal.code,
      rollbackSucceededAfterSessionEnded: true,
      stagedGenerationUsed: staged.key,
      disabledSessionStillAttached: strandId,
      note: 'the refusal and the success are driven through the shipped CLI, and a restart with keeper ownership disabled still serves the keeper session at its original pid',
    };
  },
);

// Importing this file must never start anything: the CLI runs only when it is the entry point.
if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  // Teardown runs before the exit code is chosen, so the exit can never stand in for a clean
  // shutdown: by this point every recorded child has been stopped and every socket this run opened
  // has been destroyed. The previous version set `process.exitCode` and returned, which is correct
  // only when nothing is left holding the loop open. When a piped child's stdio or an abandoned socket
  // still was, the report printed and the process sat there for hours -- which is the hang this gate
  // exists to prevent.
  //
  // `exitCode` rather than `process.exit()` on the normal path, because `process.exit` truncates a
  // report still being written to a pipe. The watchdog is the backstop for a handle nobody recorded:
  // it is `unref`ed, so it never keeps the process up by itself, and it only fires if the loop is
  // still alive after teardown has finished. A run that needs it has already been reported as a leak.
  const finish = (code) => {
    process.exitCode = code;
    const watchdog = setTimeout(() => {
      process.stderr.write(`WATCHDOG: exit ${code} forced; a handle survived teardown\n`);
      process.exit(code);
    }, 5_000);
    watchdog.unref();
  };
  main()
    .then((code) => finishRun(code))
    .then(finish)
    .catch((error) => {
      process.stderr.write(`${error?.stack ?? error}\n`);
      return finishRun(1).then(finish);
    });
}


