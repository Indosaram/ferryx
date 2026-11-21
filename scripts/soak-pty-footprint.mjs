#!/usr/bin/env node
// Phase 1 footprint soak (plan rev 5, Test B): 71 local panes producing scripted output for 60
// minutes, sampled every 30 s. Daemon and PTY keeper are measured TOGETHER, because an allocation
// that moves from one to the other must not leave the total looking healthier than it is.
//
//   node scripts/soak-pty-footprint.mjs --daemon-bin <path> --keeper-bin <path> [options]
//
// Acceptance (`.omo/plans/local-pty-crash-survival-20261116.md`, "Test B"):
//   * every sample of the last 20 minutes is within +/-5 % of the sample at t=40 min (the numeric
//     form of "no upward trend"), and
//   * no combined daemon+keeper sample exceeds 2.5 GiB.
// Anything the run could not positively verify is a failure, never a pass: a missing sample, a
// sample whose processes could not be identified, or a session the keeper does not own all end the
// run with exit 4 and a typed reason. The whole point of this file is to be unable to lie.
//
// Isolation contract (this script never reaches the user's real daemon or keeper):
//   * Every location key the daemon and keeper read is pointed inside one freshly made temp root:
//     FERRYX_RUNTIME_DIR (where both `daemon.sock`/`control.sock` and the keeper's `keeper.sock`
//     live), FERRYX_LEGACY_RUNTIME_DIR, FERRYX_DATA_DIR, FERRYX_SESSION_DIR,
//     FERRYX_ACCOUNT_DATA_DIR, FERRYX_RELAY_DATA_DIR, HOME, XDG_*, TMPDIR and
//     FERRYX_DAEMON_SUPERVISOR_NAME. Those overrides are also what stops launchd.rs installing a
//     real unit, so no OS job is created or touched.
//   * The daemon's retained-scrollback budget is pinned to this run's session count
//     (`FERRYX_SCROLLBACK_BUDGET_BYTES` = sessions x 512 KiB, `FERRYX_MIN_SESSION_HISTORY_BYTES` =
//     512 KiB), so its per-session retained history is 512 KiB instead of 1536 MiB / sessions. At the
//     default budget each of 71 sessions would get 21.6 MiB and need ~14.5 h to fill, which no 60 min
//     run can saturate - see `SOAK_RETAINED_CEILING_BYTES` and `assertSaturatesBeforeTrend`.
//   * The production runtime dir is computed only so a root that equals it is refused. The script
//     never connects to, signals, or resolves a socket outside its own root.
//   * Both processes are started here, and each one's OS identity (-o lstart= start stamp plus its
//     executable path) is recorded at spawn. A process is only ever sampled or signalled while that
//     recorded identity still matches. The keeper is NOT a child of the daemon (it is a launchd job
//     in production), so parentage cannot identify it and name matching must not: the keeper is
//     ours because we started it and because its own HELLO_ACK reports the pid we recorded.
//
// Nothing is built here: it runs already-built binaries. This is deliberately not a unit test - it
// needs a wall clock and real processes. Rev-5 acceptance is `evaluateSoak`; the saturation guard
// (`assertSaturatesBeforeTrend`) refuses a run whose workload cannot fill BOTH bounded buffers - the
// 512 KiB OUTPUT ring and the session's retained reconstruction history - before the trend window
// opens. The pure parts (argument parsing, sample coverage, the rev-5 acceptance arithmetic, the
// saturation and retained-bound arithmetic, the daemon env, routing reconciliation, ps parsing) are
// exported so scripts/soak-pty-footprint.test.mjs can prove them without spawning anything.
//
// Exit codes:
//   0 pass
//   1 target breached (over 2.5 GiB, or an upward trend past +/-5 %)
//   2 usage / guard error (bad flags, unsafety refused, binary missing)
//   3 daemon or keeper exited, or a process went unreadable
//   4 unverified - a sample, a route or an owned process could not be established (fail closed)

import { spawn, spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import {
  copyFileSync,
  createWriteStream,
  existsSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
  appendFileSync,
} from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { keeperPayloadDirFor, stageAppBundle } from './lib/keeper-bundle.mjs';

const MIB = 1024 * 1024;
/** The acceptance target: daemon + keeper resident together must stay under 2.5 GiB. */
export const TARGET_BYTES = 2560 * MIB;
/** `SAMPLE_SCHEMA_VERSION` marks the evidence format, so old summaries are never misread. v3 added
 *  `measurement`/`targetElapsedMs`/`sampleLagMs`/`sampleCostMs` and made `totalBytes` phys_footprint
 *  on Darwin; a v2 sample's `totalBytes` is an RSS sum and must never be compared with a v3 one. */
export const SAMPLE_SCHEMA_VERSION = 3;

/** The plan's numbers. A run that changes any of them can never report pass. */
export const PLAN_SESSIONS = 71;
export const PLAN_DURATION_MIN = 60;
export const PLAN_INTERVAL_MS = 30_000;
/** "Last 20 minutes": every sample from here to the end must be within +/-5 % of t=40 min. */
export const TREND_FROM_MS = 40 * 60_000;
export const TREND_TOLERANCE = 0.05;

const KEEPER_SOCKET_NAME = 'keeper.sock';
const DAEMON_SOCKET_NAME = 'daemon.sock';
const CONTROL_SOCKET_NAME = 'control.sock'; // daemon::control::CONTROL_SOCKET_FILE
const DAEMON_READY_MARKER = 'FERRYX_DAEMON_READY';
/** macOS caps `sun_path` at 104 bytes including the NUL; both sockets must fit. */
const MAX_SOCKET_PATH_BYTES = 103;
/** The daemon's per-session OUTPUT ring: `DEFAULT_BUFFER_CAPACITY` in
 *  src-tauri/src/terminal/output_hub.rs:11 (`512 * 1024`). The keeper's checkpoint is a
 *  serialization of this same ring (`TerminalOutputHub::checkpoint_bytes`), so filling it bounds
 *  both sides at once. */
export const OUTPUT_RING_CAPACITY_BYTES = 524288; // output_hub.rs:11
/** The ring must be full by this fraction of `TREND_FROM_MS`, so the trend window measures a steady
 *  footprint instead of the linear fill curve that produced the run14b `upward-trend` breach.
 *  0.75 of the 40 min reference is 30 min: a rate still filling at that point would have 10 min of
 *  fill left inside the window the plan judges flat to +/-5 %, so the run could not be believed. */
export const SATURATION_DEADLINE_RATIO = 0.75;

/** Retained reconstruction history the daemon may keep for ONE session during this soak.
 *
 *  The daemon's per-session ceiling is `SCROLLBACK_BUDGET_BYTES / live sessions`
 *  (src-tauri/src/terminal/scrollback_budget.rs:30-38 `session_ceiling_bytes_with_floor`), so the
 *  plan's default 1536 MiB budget spreads to 1536 MiB / 71 = 21.6 MiB per session. At 12288 B per
 *  30 s tick that needs ~14.5 h to fill - it never saturates inside a 60 min run, which is the
 *  same unbounded-fill defect the 512 KiB ring had before run14b. The run therefore sets the
 *  daemon's budget to `sessions * SOAK_RETAINED_CEILING_BYTES` and its floor to this same value, so
 *  each session's retained history is bounded at exactly 512 KiB, the size of the OUTPUT ring, and
 *  both can be full before the t=40 min reference.
 *
 *  Read by the daemon at startup through `FERRYX_SCROLLBACK_BUDGET_BYTES` /
 *  `FERRYX_MIN_SESSION_HISTORY_BYTES` (src-tauri/src/terminal/preferences.rs:36-39, 69-73), which
 *  `DaemonServer::new()` picks up via `TerminalOutputHub::for_daemon()`
 *  (src-tauri/src/daemon/server.rs:4185, output_hub.rs:949-951). The keeper reads neither variable. */
export const SOAK_RETAINED_CEILING_BYTES = 524288;

/** The daemon's per-session minimum, set to the same 512 KiB.
 *
 *  Validation (`preferences.rs:60-66`) falls back to the 1536 MiB defaults unless the minimum is
 *  strictly above libghostty's ~10 KiB default (`LIBGHOSTTY_DEFAULT_SCROLLBACK_BYTES`, 10_000 bytes)
 *  and no larger than the budget. 524288 satisfies both, and the budget is derived from it so the
 *  pair can never be inverted. */
export const SOAK_MIN_SESSION_HISTORY_BYTES = SOAK_RETAINED_CEILING_BYTES;

/** `IMAGE_RETENTION_RATIO` in src-tauri/src/terminal/retained_history.rs:14. */
export const IMAGE_RETENTION_RATIO = 16;

/** Produced OUTPUT bytes per pane per tick - what the child prints, not what is typed at it.
 *
 *  Sized so every bounded buffer is full before the t=40 min reference, not during the window:
 *  ceil(524288 / 12288) = 43 ticks = 21.5 min at a 30 s interval, which is 54 % of TREND_FROM_MS
 *  (40 min) and inside the 0.75 saturation deadline (30 min). At the old 4096 the ring needed 128
 *  ticks = 64 min, so it was still filling for the whole run (t=60 held 484 KiB of 512 KiB) and the
 *  footprint could only trend upward - no bounded design could ever pass.
 *
 *  The direction matters as much as the rate. run14d wrote the 12 KiB itself, as *input* to each
 *  pane's tty, and the tty made the harness pay for it twice: a canonical line is capped at
 *  MAX_CANON (1024 B on macOS) and the line discipline echoes it back. Tick cost went from ~15 s to
 *  139-366 s, every 30 s sample was missed and the run was unverifiable. Here the input is a ~38 B
 *  command (see `outputCommand`, bounded by `MAX_INPUT_BYTES_PER_TICK`) and the child prints the
 *  volume, which is what the OUTPUT ring and the keeper's replay log actually store. */
const DEFAULT_BYTES_PER_TICK = 12288;

/** Ceiling on what this workload may type into one pane in one tick.
 *
 *  Deliberately far below MAX_CANON (1024 B on macOS) and below the 65536 B PTY input limit
 *  (src-tauri/src/terminal/session.rs:1600): the point of the budget is that the soak can never
 *  again measure its own tty write cost instead of the product's footprint. */
export const MAX_INPUT_BYTES_PER_TICK = 256;

/** Keeper framing constants (crates/ferryx-ptykeeper/src/protocol.rs). Pinned, never renumbered. */
const KEEPER_HEADER_BYTES = 60;
const KEEPER_MAGIC = Buffer.from('FXKP', 'ascii');
const KEEPER_FRAMING_VERSION = 1;
const KEEPER_KIND = Object.freeze({
  hello: 0x0001,
  helloAck: 0x0002,
  ownershipQuery: 0x0070,
  ownershipList: 0x0071,
  shutdown: 0x00f1,
  error: 0x00f0,
});

/** A usage or guard error: the run cannot start safely. */
export class GuardError extends Error {}
/** The run cannot establish something the acceptance depends on. Fail closed, never pass. */
export class UnverifiedError extends Error {
  constructor(code, message) {
    super(`${code}: ${message}`);
    this.code = code;
  }
}
/** A recorded process left while the run was in progress. */
export class ProcessGoneError extends Error {}

// ---------------------------------------------------------------------------------------------
// arguments and layout (pure)
// ---------------------------------------------------------------------------------------------

export function parseArgs(argv, env = process.env) {
  const options = {
    daemonBin: env.FERRYX_DAEMON_BIN ?? null,
    keeperBin: null,
    sessions: PLAN_SESSIONS,
    durationMin: PLAN_DURATION_MIN,
    intervalMs: PLAN_INTERVAL_MS,
    bytesPerTick: DEFAULT_BYTES_PER_TICK,
    out: null,
    keepRoot: false,
    help: false,
  };
  const take = (index, flag) => {
    const value = argv[index];
    if (value === undefined || value.startsWith('--')) throw new GuardError(`${flag} needs a value`);
    return value;
  };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    switch (flag) {
      case '--daemon-bin': options.daemonBin = take(++i, flag); break;
      case '--keeper-bin': options.keeperBin = take(++i, flag); break;
      case '--sessions': options.sessions = Number(take(++i, flag)); break;
      case '--duration-min': options.durationMin = Number(take(++i, flag)); break;
      case '--interval-ms': options.intervalMs = Number(take(++i, flag)); break;
      case '--bytes-per-tick': options.bytesPerTick = Number(take(++i, flag)); break;
      case '--out': options.out = take(++i, flag); break;
      case '--keep-root': options.keepRoot = true; break;
      case '--help': case '-h': options.help = true; break;
      default: throw new GuardError(`unknown argument ${flag}`);
    }
  }
  if (options.help) return options;
  // Both binaries are explicit. There is no PATH or name fallback for either one: a resolved by
  // name keeper could be somebody else's, and this script must only ever measure its own.
  if (!options.daemonBin) throw new GuardError('--daemon-bin (or FERRYX_DAEMON_BIN) is required');
  if (!options.keeperBin) throw new GuardError('--keeper-bin is required (there is no name lookup)');
  if (!existsSync(options.daemonBin)) throw new GuardError(`daemon binary not found: ${options.daemonBin}`);
  if (!existsSync(options.keeperBin)) throw new GuardError(`keeper binary not found: ${options.keeperBin}`);
  if (!Number.isInteger(options.sessions) || options.sessions < 1) {
    throw new GuardError('--sessions must be a positive integer');
  }
  if (!Number.isFinite(options.durationMin) || options.durationMin <= 0) {
    throw new GuardError('--duration-min must be a positive number');
  }
  if (!Number.isFinite(options.intervalMs) || options.intervalMs < 1000) {
    throw new GuardError('--interval-ms must be at least 1000');
  }
  if (!Number.isInteger(options.bytesPerTick) || options.bytesPerTick < 0) {
    throw new GuardError('--bytes-per-tick must be a non-negative integer');
  }
  assertInputStaysSmall(options);
  assertSaturatesBeforeTrend(options);
  return options;
}

/** How long the configured rate takes to fill one session's OUTPUT ring, in ms (Infinity when no
 *  output is produced at all). Pure, so the test file can pin the arithmetic exactly: run14b's
 *  4096 B/tick needs ceil(524288 / 4096) = 128 ticks = 64 min, the shipped 12288 needs 43 = 21.5.
 *
 *  No rounding for the `\r` of the typed line: `bytesPerTick` is what the child prints, and that is
 *  the volume the ring stores. The line echo only ever adds to it, so the arithmetic under-states
 *  the fill slightly and the saturation deadline stays conservative. */
export function ringFillMs(options) {
  if (!(options.bytesPerTick > 0)) return Infinity;
  const ticksToFill = Math.ceil(OUTPUT_RING_CAPACITY_BYTES / options.bytesPerTick);
  return ticksToFill * options.intervalMs;
}

/**
 * The text half of a retained-history ceiling.
 *
 * Mirrors `RetentionBudget::from_total` (src-tauri/src/terminal/retained_history.rs:37-45), which
 * splits a total ceiling by `divisor = IMAGE_RETENTION_RATIO + 1` and takes
 * `text_bytes = total - total / divisor`. Only text is stored per tick by this workload (the panes
 * print ASCII, no images), so the text share is the ceiling that actually has to fill.
 */
export function retainedTextCeilingBytes(totalBytes) {
  return totalBytes - Math.floor(totalBytes / (IMAGE_RETENTION_RATIO + 1));
}

/**
 * The scrollback pair this run's daemon is started with, derived from the session count so
 * `--sessions` can never leave the budget and the ceiling disagreeing.
 *
 * `perSessionCeilingBytes` is the daemon's own arithmetic - `budget / live sessions`, floored at the
 * minimum (`scrollback_budget.rs:30-38`) - applied to the plan's session count. With the derived
 * budget it lands exactly on `SOAK_RETAINED_CEILING_BYTES`, which is the point: a run that passed
 * under a ceiling it could not reach would say nothing about the per-session bound.
 */
export function retainedBudgetFor(sessions) {
  const liveSessions = Number.isInteger(sessions) && sessions > 0 ? sessions : PLAN_SESSIONS;
  const budgetBytes = liveSessions * SOAK_RETAINED_CEILING_BYTES;
  const minSessionHistoryBytes = SOAK_MIN_SESSION_HISTORY_BYTES;
  const perSessionCeilingBytes = Math.max(
    Math.floor(budgetBytes / liveSessions),
    minSessionHistoryBytes,
  );
  return {
    budgetBytes,
    minSessionHistoryBytes,
    perSessionCeilingBytes,
    retainedTextCeilingBytes: retainedTextCeilingBytes(perSessionCeilingBytes),
  };
}

/** How long the configured rate takes to fill one session's retained TEXT history, in ms.
 *
 *  `retainedCeilingBytes` defaults to the run's own per-session ceiling. It exists as an override so
 *  the guard can be proved against a deployment whose budget yields a larger share: with the derived
 *  budget the text ceiling (493448 B) is smaller than the ring, so the ring always fills last and the
 *  retained arm of the guard would otherwise be untestable. */
export function retainedFillMs(options) {
  if (!(options.bytesPerTick > 0)) return Infinity;
  const ceiling =
    options.retainedCeilingBytes ?? retainedBudgetFor(options.sessions).perSessionCeilingBytes;
  const ticksToFill = Math.ceil(retainedTextCeilingBytes(ceiling) / options.bytesPerTick);
  return ticksToFill * options.intervalMs;
}

/**
 * Refuse a run whose workload cannot saturate every bounded buffer before the trend window opens.
 *
 * A soak that is still filling a bounded buffer at t=40 min measures a fill curve, not a footprint:
 * the last 20 minutes cannot be flat to +/-5 % no matter how correct the product is. run14b proved
 * it for the 512 KiB ring - it needed 64 min at 4096 B/tick, so the whole run trended upward.
 *
 * TWO bounds decide that, not one. The OUTPUT ring is bounded by `OUTPUT_RING_CAPACITY_BYTES` and the
 * session's retained reconstruction history by `budget / live sessions` (split 16:17 into text and
 * images, `retained_history.rs:37-45`). Checking only the ring was the run15 defect: the ring filled
 * in 21.5 min while the retained history, at the plan's default 1536 MiB / 71 = 21.6 MiB, would have
 * needed ~14.5 h, so the retained growth continued through the whole trend window and the run could
 * only breach. The fill time is therefore the worse of the two. The run pins the daemon's budget to
 * `sessions * SOAK_RETAINED_CEILING_BYTES` (see `retainedBudgetFor`), which makes both bounds 512 KiB
 * and both fill before the deadline; the retained term is what keeps a future budget change honest.
 *
 * Only runs that actually reach `TREND_FROM_MS` are held to this: a short smoke run has no trend
 * window to protect and may legitimately use any rate.
 */
export function assertSaturatesBeforeTrend(options) {
  if (options.durationMin * 60_000 < TREND_FROM_MS) return;
  const ringMs = ringFillMs(options);
  const retainedMs = retainedFillMs(options);
  const fillMs = Math.max(ringMs, retainedMs);
  const deadlineMs = TREND_FROM_MS * SATURATION_DEADLINE_RATIO;
  // Refused at or past the deadline, not only beyond it: a run that fills exactly at the deadline has
  // no settled window left, so the boundary must not be the passing side.
  if (fillMs >= deadlineMs) {
    // The rate that gets under the deadline. `maxTicks` is one fewer than the ticks that fit in the
    // deadline (filling exactly at the boundary is refused above). `bytesPerTick` is output the child
    // prints, so the smallest accepted rate is plain `bound / ticks`; at the plan's 30 s interval and
    // the shipped 512 KiB ring that is 8887, and 8886 still needs the full 60 ticks this check refuses.
    const maxTicks = Math.floor(deadlineMs / options.intervalMs) - 1;
    const retained = retainedBudgetFor(options.sessions);
    // Nullish, matching `retainedFillMs`'s own default, so the message and the arithmetic can never
    // describe two different ceilings.
    const overrideCeiling = options.retainedCeilingBytes ?? null;
    const retainedText = retainedTextCeilingBytes(
      overrideCeiling ?? retained.perSessionCeilingBytes,
    );
    // Whichever bound fills last is the one the message must name, or the operator is told to raise a
    // rate that is already enough for the buffer it mentions.
    const retainedBinds = retainedMs > ringMs;
    const boundBytes = Math.max(OUTPUT_RING_CAPACITY_BYTES, retainedText);
    const ceilingBytes = overrideCeiling ?? retained.perSessionCeilingBytes;
    // The derived ceiling is the run's own budget share; an explicit override is a caller modelling a
    // different deployment, and saying "out of a 37224448 B budget" there would name a budget the
    // override never came from.
    const share =
      overrideCeiling === null
        ? ` (session ceiling ${ceilingBytes} B out of a ${retained.budgetBytes} B budget)`
        : ` (an explicit ${ceilingBytes} B per-session ceiling, not this run's ` +
          `${retained.perSessionCeilingBytes} B share)`;
    const bound = retainedBinds
      ? `the ${retainedText} byte retained-history text ceiling${share}`
      : `the ${OUTPUT_RING_CAPACITY_BYTES} byte OUTPUT ring`;
    // A sampling interval too long to fit even one tick before the deadline leaves no rate to advise,
    // so say which knob to turn instead of printing an arithmetic result for a rate of Infinity.
    const advice =
      maxTicks < 1
        ? 'no rate fits before the deadline; shorten --interval-ms'
        : `>= ${Math.ceil(boundBytes / maxTicks)} at this interval`;
    throw new GuardError(
      `--bytes-per-tick ${options.bytesPerTick} cannot fill ${bound} ` +
        `before the t=${Math.round(TREND_FROM_MS / 60_000)} min reference: needs ` +
        `${Number.isFinite(fillMs) ? `${(fillMs / 60_000).toFixed(1)} min` : 'unbounded time'}, ` +
        `deadline is ${(deadlineMs / 60_000).toFixed(1)} min. The last 20 minutes would measure a fill ` +
        `curve, so the trend could only breach. Raise --bytes-per-tick (${advice}).`,
    );
  }
}

/**
 * Why a run's workload did not deliver the output it claims, or null when it did.
 *
 * The verdict is a statement about what the daemon and keeper retained, and that only means something
 * if the panes actually produced the volume the saturation arithmetic assumes. A shell that ignored
 * the command, or a keeper whose pump never ran, would leave the rings empty - and an empty bounded
 * buffer is trivially flat, which is the one way this soak could report a pass it did not earn. So
 * the produced total is measured from the keeper's replay heads and compared against what was asked.
 *
 * The floor allows for the tick in flight. Each tick's command is written and then the keeper's heads
 * are read immediately afterwards, so the last tick's output may not have been drained yet - the
 * shell has to run the pipeline first. Demanding all `ticksWritten` would therefore fail a perfectly
 * healthy run at its final sample, so the floor is the completed ticks: `ticksWritten - 1`. That is
 * still strict enough to catch a shell that never printed anything, which is the failure this exists
 * for. Every measured tick over-delivers, because the typed command's own echo lands in the same head.
 */
export function workloadShortfall({ sessions, bytesPerTick, ticksWritten, producedBytes }) {
  if (!(bytesPerTick > 0)) return null;
  const completedTicks = Math.max(0, ticksWritten - 1);
  const required = sessions * bytesPerTick * completedTicks;
  if (producedBytes >= required) return null;
  return (
    `the panes produced ${producedBytes} B of the ${required} B the workload asked for over ` +
    `${completedTicks} completed tick(s) (${sessions} pane(s) x ${bytesPerTick} B): the OUTPUT ring ` +
    `was never filled, so a flat footprint would not mean a bounded one`
  );
}

/** True only for a run that can report pass at all. Anything reduced is explicitly not the plan. */
export function isFullSpec(options) {
  return (
    options.sessions === PLAN_SESSIONS &&
    options.durationMin === PLAN_DURATION_MIN &&
    options.intervalMs === PLAN_INTERVAL_MS
  );
}

/**
 * Child argv for each binary. `spawn(file, args)` puts `args` at argv[1..], so the executable path
 * must NOT be repeated in the array: the keeper parses its own argv strictly and would reject its
 * own path as `unknown argument`, exiting 2 before it binds a socket (crates/ferryx-ptykeeper/src/
 * main.rs). Kept as exported functions so the mistake stays provable by a test.
 */
export function keeperArgv(socketPath) {
  return ['--socket', socketPath];
}

export function daemonArgv() {
  return ['--daemon']; // parse_launch_mode scans every arg, so argv[1] is enough
}

/** Where the user's real daemon lives. Computed only so a run can refuse to equal it. */
export function productionRuntimeDir(env = process.env, platform = process.platform) {
  if (platform === 'win32') {
    const base = env.LOCALAPPDATA || env.TEMP || 'C:\\ProgramData';
    return path.join(base, 'Ferryx', 'runtime');
  }
  if (typeof process.getuid !== 'function') return null;
  return path.join('/tmp', `ferryx-${process.getuid()}`);
}

export function isInside(child, parent) {
  const sep = path.sep;
  const normalise = (value) => (value.endsWith(sep) ? value : value + sep);
  return child === parent || child.startsWith(normalise(parent));
}

/**
 * Stages `daemonBin` into a minimal app bundle inside `layout.root`, and returns the executable to run.
 *
 * `keeper_startup::ensure_keeper` -> `bundle_keeper_dir(current_exe())` reads the executable's own
 * path: the parent must be named `MacOS` and `../Resources/keeper/ferryx-ptykeeper` must exist, with
 * no environment override. A daemon started straight from `target/debug/ferryx` therefore answers
 * `PayloadAbsent`, `start_production` aborts, the new-session routing flag is never written, and
 * every pane in this soak stays daemon-owned -- which is exactly the `KEEPER_ROUTING_UNVERIFIED`
 * outcome this run used to report. Staging the real bytes into the shipped layout is what lets the
 * soak measure a keeper-owned footprint at all.
 *
 * Linux has no keeper to install (`daemon/mod.rs` gates the module to macOS), so the daemon is run as
 * given there; the argv and environment the soak passes are unchanged on both platforms.
 */
export function stageSoakDaemon(layout, daemonBin, keeperBin, platform = process.platform) {
  if (platform !== 'darwin') return daemonBin;
  if (keeperPayloadDirFor(daemonBin) !== null) return daemonBin;
  const staged = stageAppBundle({
    appPath: path.join(layout.root, 'Ferryx.app'),
    daemonBinary: daemonBin,
    keeperBinary: keeperBin,
    triples: [],
  });
  return staged.daemon;
}

/** Every path the run will use. Pure: building the layout creates nothing. */
export function makeLayout(root) {
  return {
    root,
    runtime: path.join(root, 'run'),
    legacyRuntime: path.join(root, 'legacy-runtime'),
    data: path.join(root, 'data'),
    sessions: path.join(root, 'sessions'),
    home: path.join(root, 'home'),
    tmp: path.join(root, 'tmp'),
    logs: path.join(root, 'logs'),
    keeper: path.join(root, 'keeper'),
    project: path.join(root, 'project'),
    daemonSocket: path.join(root, 'run', DAEMON_SOCKET_NAME),
    controlSocket: path.join(root, 'run', CONTROL_SOCKET_NAME),
    keeperSocket: path.join(root, 'run', KEEPER_SOCKET_NAME),
  };
}

/** Refuses a layout that could point a process at the user's real daemon, or at a bad UDS path. */
export function assertIsolated(layout, env = process.env) {
  const production = productionRuntimeDir(env);
  for (const key of ['runtime', 'legacyRuntime', 'data', 'sessions', 'home', 'tmp', 'logs', 'project']) {
    if (!layout[key]) throw new GuardError(`layout.${key} is unset`);
    if (!isInside(layout[key], layout.root)) {
      throw new GuardError(`layout.${key}=${layout[key]} is outside the run root ${layout.root}`);
    }
  }
  if (production && (layout.root === production || layout.runtime === production)) {
    throw new GuardError(`run root ${layout.root} would reach the production runtime ${production}`);
  }
  for (const key of ['daemonSocket', 'controlSocket', 'keeperSocket']) {
    const length = Buffer.byteLength(layout[key], 'utf8');
    if (length > MAX_SOCKET_PATH_BYTES) {
      throw new GuardError(
        `${key} ${layout[key]} is ${length} bytes, longer than the ${MAX_SOCKET_PATH_BYTES} byte unix socket limit`,
      );
    }
  }
  return layout;
}

export function ensureLayout(layout) {
  for (const dir of [
    layout.runtime, layout.legacyRuntime, layout.data, layout.sessions,
    layout.home, layout.tmp, layout.logs, layout.keeper, layout.project,
  ]) {
    mkdirSync(dir, { recursive: true, mode: 0o700 });
  }
  return layout;
}

/**
 * The retained-scrollback pair the daemon reads at startup, as environment strings.
 *
 * `FERRYX_SCROLLBACK_BUDGET_BYTES` / `FERRYX_MIN_SESSION_HISTORY_BYTES`
 * (src-tauri/src/terminal/preferences.rs:36-39) are read by `configured_scrollback_budget()`
 * (preferences.rs:69-73) from `TerminalOutputHub::for_daemon()` (output_hub.rs:949-951), which
 * `DaemonServer::new()` builds at startup (server.rs:4185) - the path `--daemon` takes
 * (src-tauri/src/main.rs:146 -> cli.rs:4643). Values are decimal, exactly what `str::parse::<usize>`
 * expects; an unusable pair silently falls back to the 1536 MiB defaults (preferences.rs:60-66), so
 * the soak passes a pair it also asserts on rather than trusting the fallback.
 *
 * The keeper reads neither variable (crates/ferryx-ptykeeper has no reference), and receives them
 * only because it shares one child env; harmless, and stated here so nobody assumes it honours them.
 */
export function scrollbackEnv(sessions = PLAN_SESSIONS) {
  const { budgetBytes, minSessionHistoryBytes } = retainedBudgetFor(sessions);
  return {
    FERRYX_SCROLLBACK_BUDGET_BYTES: String(budgetBytes),
    FERRYX_MIN_SESSION_HISTORY_BYTES: String(minSessionHistoryBytes),
  };
}

/**
 * The child env: every location key the daemon and keeper read points inside `layout`. The set is
 * the one scripts/qa/unified-supervisor/harness.mjs uses for the same reason - it is what makes
 * supervisor_windows.rs and launchd.rs refuse to install anything.
 */
export function isolatedEnv(layout, runId, base = process.env, sessions = PLAN_SESSIONS) {
  return {
    ...base,
    ...scrollbackEnv(sessions),
    HOME: layout.home,
    USERPROFILE: layout.home,
    XDG_CONFIG_HOME: path.join(layout.home, 'config'),
    XDG_DATA_HOME: path.join(layout.home, 'data'),
    XDG_CACHE_HOME: path.join(layout.home, 'cache'),
    XDG_STATE_HOME: path.join(layout.home, 'state'),
    TMPDIR: layout.tmp,
    FERRYX_RUNTIME_DIR: layout.runtime,
    FERRYX_LEGACY_RUNTIME_DIR: layout.legacyRuntime,
    FERRYX_DATA_DIR: layout.data,
    FERRYX_SESSION_DIR: layout.sessions,
    FERRYX_ACCOUNT_DATA_DIR: path.join(layout.data, 'account'),
    FERRYX_RELAY_DATA_DIR: path.join(layout.data, 'relay'),
    FERRYX_KEEPER_DIR: layout.keeper,
    FERRYX_PTY_KEEPER_PLIST_PATH: path.join(layout.home, 'com.ferryx.ptykeeper.plist'),
    // A QA supervisor name is what makes the supervisor refuse to install a real OS job.
    FERRYX_DAEMON_SUPERVISOR_NAME: `ferryx-soak-${runId}`,
    FERRYX_QA_RUN_ID: runId,
  };
}

// ---------------------------------------------------------------------------------------------
// process identity: recorded at spawn, re-checked before every use
// ---------------------------------------------------------------------------------------------

/**
 * One `ps` row for `pid`: pid, ppid, resident bytes, the OS start stamp and the executable path.
 * `lstart` is the OS identity that survives pid reuse; `comm` (not `args`) is the executable, so a
 * reused pid running a different program can never look like ours. macOS and Linux both report rss
 * in KiB. Returns null when the process does not exist or ps failed.
 */
export function processIdentity(pid, run = spawnSync) {
  const probe = probeProcess(pid, run);
  return probe.state === 'alive' ? probe.identity : null;
}

/**
 * Liveness of one pid, in three states rather than two.
 *
 * `processIdentity` collapses "there is no such process" and "I could not read the process table" into
 * the same `null`. That is fine for teardown, where an unreadable process is simply not signalled, but
 * it is unsafe for the post-shutdown census: there, `null` was being read as proof that a pane had been
 * reaped, so a failed `ps` (missing binary, EAGAIN, an unparseable localised row, a permissions error)
 * fabricated "gone" for a shell that was still running, and the run then deleted the root underneath
 * it. This distinguishes the two:
 *
 *   - `alive`   - `ps` printed a row for the pid that parsed. `identity` is that row.
 *   - `gone`    - `ps` completed and printed nothing at all for the pid, with nothing on stderr. The
 *                 kernel had no matching process, which is the only real evidence of absence.
 *   - `unknown` - anything else: a non-empty stderr, a status that is neither 0 nor the no-match
 *                 status, non-string stdout, or output that did not parse. Not evidence of absence.
 *
 * The distinction rests on `ps`'s own behaviour: a missing pid yields exit 1 with empty stdout and
 * empty stderr, so silence is the signature of "no such process", while any diagnostic text means the
 * read itself failed. Guessing in the other direction here is what caused the defect.
 */
export function probeProcess(pid, run = spawnSync) {
  if (!Number.isInteger(pid) || pid <= 0) {
    return { state: 'unknown', reason: 'pid-not-a-positive-integer' };
  }
  let result;
  try {
    result = run('ps', ['-p', String(pid), '-o', 'pid=,ppid=,rss=,lstart=,comm='], {
      encoding: 'utf8',
      env: { ...process.env, ...PS_ENV_PIN },
    });
  } catch (error) {
    return { state: 'unknown', reason: `ps-threw: ${String(error?.message ?? error)}` };
  }
  if (!result || typeof result.stdout !== 'string') {
    return { state: 'unknown', reason: 'ps-returned-no-stdout-string' };
  }
  const stderr = typeof result.stderr === 'string' ? result.stderr.trim() : '';
  const stdout = result.stdout.trim();
  if (stdout) {
    const identity = parsePsRow(stdout);
    if (!identity) return { state: 'unknown', reason: 'ps-row-unparseable' };
    return { state: 'alive', identity };
  }
  // Empty stdout. Silence is the only acceptable evidence of absence: any stderr text, or a status
  // that is not a clean success or the documented no-match code, means the read failed rather than
  // the process being missing.
  if (stderr) return { state: 'unknown', reason: `ps-stderr: ${stderr.split('\n')[0]}` };
  if (result.status !== 0 && result.status !== 1) {
    return { state: 'unknown', reason: `ps-status-${String(result.status)}` };
  }
  if (result.error) return { state: 'unknown', reason: `ps-error: ${String(result.error?.message ?? result.error)}` };
  return { state: 'gone' };
}

/**
 * `lstart` and `comm` are localised: under a UTF-8 German or Japanese locale `ps` prints a translated
 * weekday/month and the parser below finds nothing, which turns every identity read into null and
 * disables teardown for the whole run. The child process owns its own locale, so the read is pinned
 * to C regardless of the operator's session. Without this the Darwin lane cannot pass on a
 * localised host - not a safety bug, but an unpassable run.
 */
export const PS_ENV_PIN = Object.freeze({ LC_ALL: 'C', LANG: 'C', LC_TIME: 'C' });

const PS_LSTART = /^(\d+)\s+(\d+)\s+(\d+)\s+([A-Z][a-z]{2}\s+[A-Z][a-z]{2}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2}\s+\d{4})\s+(.*)$/;

export function parsePsRow(stdout) {
  const line = String(stdout ?? '').trim();
  if (!line) return null;
  const match = PS_LSTART.exec(line);
  if (!match) return null;
  const args = match[5].trim();
  return {
    pid: Number(match[1]),
    ppid: Number(match[2]),
    rssBytes: Number(match[3]) * 1024,
    startStamp: match[4].replace(/\s+/g, ' ').trim(),
    executable: args,
  };
}

/**
 * `footprint -p <pid>` header: "ferryx [123]: 64-bit    Footprint: 1616 KB (...)".
 *
 * This, not resident size, is the quantity the plan states its acceptance in: the "Measured facts"
 * row that motivated the 2.5 GiB target is a `footprint -p` reading, and phys_footprint is what the
 * process is actually charged for. `ps -o rss` counts shared clean file-backed pages and whatever
 * happens to be resident at that instant, which is why a footprint acceptance judged on RSS was
 * reading a different (and much noisier) number than the one the band was written against.
 */
export function parseMacFootprint(stdout) {
  const match = /Footprint:\s*([\d.]+)\s*(B|KB|MB|GB)\b/.exec(String(stdout ?? ''));
  if (!match) return null;
  const unit = { B: 1, KB: 1024, MB: 1024 ** 2, GB: 1024 ** 3 }[match[2]];
  const bytes = Math.round(Number(match[1]) * unit);
  return Number.isFinite(bytes) && bytes > 0 ? bytes : null;
}

/**
 * Bytes the process is charged for, on the plan's own basis. macOS answers with phys_footprint from
 * `/usr/bin/footprint`; every other platform keeps the resident size already read for identity, so
 * the Linux lane is unchanged. Returns null (never a fabricated number) when the tool is missing or
 * its output does not parse, and the caller then falls back to the resident sum and says so.
 */
export function measureFootprintBytes(pid, run = spawnSync, platform = process.platform) {
  if (platform !== 'darwin' || !Number.isInteger(pid) || pid <= 0) return null;
  let result;
  try {
    result = run('/usr/bin/footprint', ['-p', String(pid)], {
      encoding: 'utf8',
      maxBuffer: 8 * 1024 * 1024,
      env: { ...process.env, ...PS_ENV_PIN },
    });
  } catch {
    return null;
  }
  if (!result || typeof result.stdout !== 'string') return null;
  return parseMacFootprint(result.stdout);
}

/** True while `pid` is still the exact process that was recorded. */
export function identityMatches(recorded, current) {
  if (!recorded || !current) return false;
  return (
    current.pid === recorded.pid &&
    current.startStamp === recorded.startStamp &&
    current.executable === recorded.executable
  );
}

// ---------------------------------------------------------------------------------------------
// daemon wire: JSON lines on a unix socket (data protocol and the frozen control-v1 channel)
// ---------------------------------------------------------------------------------------------

export function connectJsonLines(socketPath, timeoutMs = 15_000, callTimeoutMs = 30_000) {
  return new Promise((resolve, reject) => {
    const socket = net.connect({ path: socketPath });
    let buffer = '';
    let settled = false;
    const pending = [];
    socket.setNoDelay(true);

    const failAll = (error) => {
      while (pending.length > 0) pending.shift().reject(error);
    };
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      socket.destroy();
      reject(new Error(`timed out connecting to ${socketPath}`));
    }, timeoutMs);

    socket.on('data', (chunk) => {
      buffer += chunk.toString('utf8');
      let index = buffer.indexOf('\n');
      while (index !== -1) {
        const line = buffer.slice(0, index).trim();
        buffer = buffer.slice(index + 1);
        index = buffer.indexOf('\n');
        if (!line) continue;
        let value;
        try {
          value = JSON.parse(line);
        } catch {
          continue; // a partial or non-frame line is never a reply
        }
        // Streamed pane output is pushed, not answered; nothing here ever attaches, so these can
        // only be ignored. Every other frame is a reply and resolves the oldest outstanding call.
        if (value && (value.type === 'output' || value.type === 'replayChunk')) continue;
        const waiter = pending.shift();
        if (waiter) {
          clearTimeout(waiter.watchdog);
          waiter.resolve(value);
        }
      }
    });
    socket.on('error', (error) => {
      if (!settled) {
        settled = true;
        clearTimeout(timer);
        reject(error);
        return;
      }
      failAll(error);
    });
    socket.on('close', () => {
      failAll(new Error(`connection to ${socketPath} closed`));
    });
    socket.once('connect', () => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({
        call: (request) => new Promise((resolveCall, rejectCall) => {
          if (socket.destroyed) {
            rejectCall(new Error(`connection to ${socketPath} is closed`));
            return;
          }
          // A daemon that stops answering must fail the run, not hang it: every call has a bound.
          const watchdog = setTimeout(() => {
            const index = pending.findIndex((entry) => entry.watchdog === watchdog);
            if (index !== -1) pending.splice(index, 1);
            rejectCall(new Error(`no reply to ${request?.type} within ${callTimeoutMs} ms`));
          }, callTimeoutMs);
          pending.push({ resolve: resolveCall, reject: rejectCall, watchdog });
          socket.write(`${JSON.stringify(request)}\n`);
        }),
        close: () => socket.destroy(),
      });
    });
  });
}

/** The data protocol version this build speaks. Learned, never assumed: see handshakeWith. */
export async function handshakeWith(connection, pid) {
  let version = 6; // src-tauri/src/daemon/protocol.rs DAEMON_PROTOCOL_VERSION
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const reply = await connection.call({ type: 'handshake', version });
    if (reply?.type === 'handshakeOk') {
      if (reply.pid !== pid) {
        throw new UnverifiedError(
          'DAEMON_NOT_ISOLATED',
          `data socket answered as pid ${reply.pid}, expected the daemon we started (${pid})`,
        );
      }
      return reply;
    }
    if (reply?.type === 'protocolMismatch' && Number.isInteger(reply.expectedVersion)) {
      version = reply.expectedVersion;
      continue;
    }
    throw new UnverifiedError('DAEMON_HANDSHAKE_FAILED', `handshake reply ${JSON.stringify(reply)}`);
  }
  throw new UnverifiedError('DAEMON_HANDSHAKE_FAILED', 'no handshakeOk after adopting expectedVersion');
}

/** control-v1 Hello. `pid` is the isolation proof: it must be the daemon we started. */
export async function controlHello(control, pid) {
  const reply = await control.call({ control: 1, op: 'Hello' });
  if (!reply || reply.ok !== true) {
    throw new UnverifiedError('DAEMON_CONTROL_UNAVAILABLE', `control Hello refused: ${JSON.stringify(reply)}`);
  }
  if (reply.pid !== pid) {
    throw new UnverifiedError(
      'DAEMON_NOT_ISOLATED',
      `control socket answered as pid ${reply.pid}, expected the daemon we started (${pid})`,
    );
  }
  return reply;
}

export async function controlInventory(control) {
  const reply = await control.call({ control: 1, op: 'Inventory' });
  if (!reply || reply.ok !== true) {
    throw new UnverifiedError('DAEMON_CONTROL_UNAVAILABLE', `control Inventory refused: ${JSON.stringify(reply)}`);
  }
  return { liveLocal: reply.live_local, liveRemote: reply.live_remote, livePaired: reply.live_paired };
}

// ---------------------------------------------------------------------------------------------
// keeper wire: 60-byte big-endian header + JSON payload (crates/ferryx-ptykeeper/src/protocol.rs)
// ---------------------------------------------------------------------------------------------

export function encodeKeeperFrame(kind, requestId, sequence, payload) {
  const id = Buffer.from(requestId, 'ascii');
  if (id.length !== 36) throw new GuardError(`keeper request id must be 36 bytes, got ${id.length}`);
  const body = Buffer.from(JSON.stringify(payload ?? {}), 'utf8');
  const header = Buffer.alloc(KEEPER_HEADER_BYTES);
  KEEPER_MAGIC.copy(header, 0);
  header.writeUInt16BE(KEEPER_FRAMING_VERSION, 4);
  header.writeUInt16BE(kind, 6);
  header.writeUInt32BE(body.length, 8);
  id.copy(header, 12);
  header.writeBigUInt64BE(BigInt(sequence), 48);
  header.writeUInt16BE(0, 56); // no attached descriptors
  header.writeUInt16BE(0, 58); // reserved flags must be 0
  return Buffer.concat([header, body]);
}

/** Decodes one frame from the front of `buffer`; returns null while more bytes are needed. */
export function decodeKeeperFrame(buffer) {
  if (buffer.length < KEEPER_HEADER_BYTES) return null;
  if (!buffer.subarray(0, 4).equals(KEEPER_MAGIC)) throw new Error('keeper frame magic is not FXKP');
  const framing = buffer.readUInt16BE(4);
  if (framing !== KEEPER_FRAMING_VERSION) throw new Error(`keeper framing version ${framing}`);
  const kind = buffer.readUInt16BE(6);
  const payloadLength = buffer.readUInt32BE(8);
  if (buffer.length < KEEPER_HEADER_BYTES + payloadLength) return null;
  const requestId = buffer.toString('ascii', 12, 48);
  const frameSequence = Number(buffer.readBigUInt64BE(48));
  const fdCount = buffer.readUInt16BE(56);
  if (fdCount !== 0) throw new Error(`keeper frame carried ${fdCount} descriptors; none expected`);
  const body = buffer.subarray(KEEPER_HEADER_BYTES, KEEPER_HEADER_BYTES + payloadLength).toString('utf8');
  let payload;
  try {
    payload = JSON.parse(body);
  } catch {
    payload = null;
  }
  return {
    frame: { kind, requestId, frameSequence, payload },
    rest: buffer.subarray(KEEPER_HEADER_BYTES + payloadLength),
  };
}

/**
 * One request/response against the keeper. Replies are correlated on `request_id`, never on "the
 * next frame": the keeper originates OUTPUT frames with a padding id, so position would be wrong.
 */
export function connectKeeper(socketPath, timeoutMs = 15_000) {
  return new Promise((resolve, reject) => {
    const socket = net.connect({ path: socketPath });
    let buffer = Buffer.alloc(0);
    let settled = false;
    const pending = new Map();
    socket.setNoDelay(true);

    const failAll = (error) => {
      for (const waiter of pending.values()) waiter.reject(error);
      pending.clear();
    };
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      socket.destroy();
      reject(new Error(`timed out connecting to the keeper at ${socketPath}`));
    }, timeoutMs);

    socket.on('data', (chunk) => {
      buffer = Buffer.concat([buffer, chunk]);
      for (;;) {
        const decoded = decodeKeeperFrame(buffer);
        if (!decoded) break;
        buffer = decoded.rest;
        const waiter = pending.get(decoded.frame.requestId);
        if (!waiter) continue; // a keeper-originated frame (padded id) is nobody's reply
        pending.delete(decoded.frame.requestId);
        waiter.resolve(decoded.frame);
      }
    });
    socket.on('error', (error) => {
      if (!settled) {
        settled = true;
        clearTimeout(timer);
        reject(error);
        return;
      }
      failAll(error);
    });
    socket.on('close', () => failAll(new Error(`keeper connection to ${socketPath} closed`)));
    socket.once('connect', () => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      let sequence = 0;
      resolve({
        call: (kind, payload, deadlineMs = 15_000) => new Promise((resolveCall, rejectCall) => {
          if (socket.destroyed) {
            rejectCall(new Error(`keeper connection to ${socketPath} is closed`));
            return;
          }
          const requestId = randomUUID();
          sequence += 1;
          const watchdog = setTimeout(() => {
            pending.delete(requestId);
            rejectCall(new Error(`keeper request ${kind} timed out after ${deadlineMs}ms`));
          }, deadlineMs);
          pending.set(requestId, {
            resolve: (frame) => { clearTimeout(watchdog); resolveCall(frame); },
            reject: (error) => { clearTimeout(watchdog); rejectCall(error); },
          });
          socket.write(encodeKeeperFrame(kind, requestId, sequence, payload));
        }),
        close: () => socket.destroy(),
      });
    });
  });
}

/** HELLO_ACK.pid is the keeper's own statement of identity; it must equal the pid we spawned. */
export async function keeperHello(keeper, pid) {
  const frame = await keeper.call(KEEPER_KIND.hello, { protocolMajor: 1 });
  if (frame.kind === KEEPER_KIND.error) {
    throw new UnverifiedError('KEEPER_HANDSHAKE_FAILED', `keeper Error ${JSON.stringify(frame.payload)}`);
  }
  if (frame.kind !== KEEPER_KIND.helloAck) {
    throw new UnverifiedError('KEEPER_HANDSHAKE_FAILED', `unexpected keeper frame kind ${frame.kind}`);
  }
  if (frame.payload?.pid !== pid) {
    throw new UnverifiedError(
      'KEEPER_IDENTITY_UNVERIFIED',
      `keeper socket answered as pid ${frame.payload?.pid}, expected the keeper we started (${pid})`,
    );
  }
  return frame.payload;
}

export async function keeperOwnership(keeper) {
  const frame = await keeper.call(KEEPER_KIND.ownershipQuery, {});
  if (frame.kind === KEEPER_KIND.error) {
    throw new UnverifiedError('KEEPER_OWNERSHIP_FAILED', `keeper Error ${JSON.stringify(frame.payload)}`);
  }
  if (frame.kind !== KEEPER_KIND.ownershipList) {
    throw new UnverifiedError('KEEPER_OWNERSHIP_FAILED', `unexpected keeper frame kind ${frame.kind}`);
  }
  const sessions = Array.isArray(frame.payload?.sessions) ? frame.payload.sessions : [];
  return sessions;
}

/**
 * Decides whether a Shutdown may be sent at all. `Kind::Shutdown` retires every session the keeper
 * owns, so sending it to the wrong keeper would be destructive to somebody else's terminals. The
 * keeper wire has no bearer token to present, so the proof is structural and is checked here:
 *
 * 1. `proof` exists, i.e. HELLO completed. No proof, no request.
 * 2. The pid in that HELLO reply is the pid this run spawned. A keeper that answers as a different
 *    process is not ours, even if it is listening on our socket path.
 * 3. The socket the request travels over is inside this run's private root. That is what rules out
 *    the production keeper and any other daemon's keeper: their sockets are elsewhere.
 * 4. That socket is not inside the production runtime dir, checked a second time so a mis-built
 *    layout cannot pass merely because `root` was also mis-built.
 *
 * Anything not proven is refused. Refusal is recorded in the summary and keeps the root; it is never
 * an exception, because this runs inside teardown.
 */
export function keeperShutdownGate({ proof, layout, env = process.env } = {}) {
  if (!proof) return { allowed: false, reason: 'keeper-identity-never-verified' };
  if (!Number.isInteger(proof.pid) || proof.pid !== proof.spawnedPid) {
    return { allowed: false, reason: 'keeper-hello-pid-does-not-match-the-pid-this-run-spawned' };
  }
  if (!proof.socketPath || !layout?.root || !isInside(proof.socketPath, layout.root)) {
    return { allowed: false, reason: 'keeper-socket-is-outside-the-run-root' };
  }
  const production = productionRuntimeDir(env);
  if (production && (proof.socketPath === production || isInside(proof.socketPath, production))) {
    return { allowed: false, reason: 'keeper-socket-is-inside-the-production-runtime' };
  }
  return { allowed: true };
}

/**
 * Asks the keeper to retire everything it owns, but only through `keeperShutdownGate`: the only
 * per-session shutdown seam that exists (`crates/ferryx-ptykeeper/src/server.rs`,
 * `Kind::Shutdown = 0x00f1` refuses later spawns and retires owned sessions). It does NOT replace
 * `stopRecorded` for the two children this run spawned - those are stopped by recorded identity - and
 * it never signals a pane pid: the 71 panes are keeper-owned, so the keeper is what releases them.
 */
export async function requestKeeperShutdown(keeperConnection, { proof, layout, env } = {}) {
  const gate = keeperShutdownGate({ proof, layout, env });
  if (!gate.allowed) return { requested: false, refused: true, reason: gate.reason };
  if (!keeperConnection) return { requested: false, reason: 'no-keeper-connection' };
  try {
    const frame = await keeperConnection.call(KEEPER_KIND.shutdown, {});
    if (frame?.kind === KEEPER_KIND.shutdown) return { requested: true, acknowledged: true };
    if (frame?.kind === KEEPER_KIND.error) {
      return { requested: true, acknowledged: false, reason: JSON.stringify(frame.payload ?? {}) };
    }
    return { requested: true, acknowledged: false, reason: `unexpected frame kind ${frame?.kind}` };
  } catch (error) {
    return { requested: true, acknowledged: false, reason: String(error?.message ?? error) };
  }
}

// ---------------------------------------------------------------------------------------------
// routing reconciliation (pure)
// ---------------------------------------------------------------------------------------------

/**
 * Reconciles the three independent views of "which sessions exist and who owns them". They must
 * agree exactly and the keeper must own every one: a session the daemon still owns is not protected
 * by the keeper at all, and a combined footprint that counts an idle keeper would be a false pass.
 *
 * Returns { ok, reasons, keeperOwned, daemonOwned, unknown }.
 */
export function reconcileRouting({ expectedIds, liveSessions, keeperSessions, inventory, sessions }) {
  const reasons = [];
  const expected = new Set(expectedIds);

  const described = new Map();
  for (const entry of liveSessions ?? []) {
    if (!entry || typeof entry.backendId !== 'string') continue;
    described.set(entry.backendId, entry);
  }
  const missingFromDaemon = [...expected].filter((id) => !described.has(id));
  if (missingFromDaemon.length > 0) {
    reasons.push(`daemon describeLiveSessions is missing ${missingFromDaemon.length} session(s)`);
  }
  const extraFromDaemon = [...described.keys()].filter((id) => !expected.has(id));
  if (extraFromDaemon.length > 0) {
    reasons.push(`daemon describeLiveSessions listed ${extraFromDaemon.length} session(s) this run did not spawn`);
  }
  const nonLocal = [...described.values()].filter((entry) => entry.kind !== 'local');
  if (nonLocal.length > 0) reasons.push(`${nonLocal.length} described session(s) are not local`);
  const noChild = [...described.values()].filter(
    (entry) => !Number.isInteger(entry.localChildPid) || entry.localChildPid <= 0,
  );
  if (noChild.length > 0) reasons.push(`${noChild.length} local session(s) report no child pid`);

  const keeperById = new Map();
  for (const entry of keeperSessions ?? []) {
    if (!entry || typeof entry.id !== 'string') continue;
    keeperById.set(entry.id, entry);
  }
  const keeperOwned = [...keeperById.values()].filter((entry) => entry.owner === 'keeper');
  const daemonOwned = [...keeperById.values()].filter((entry) => entry.owner !== 'keeper');
  const missingFromKeeper = [...expected].filter((id) => !keeperById.has(id));
  if (missingFromKeeper.length > 0) {
    reasons.push(`keeper OwnershipList is missing ${missingFromKeeper.length} of ${sessions} session(s)`);
  }
  if (daemonOwned.length > 0) {
    reasons.push(`${daemonOwned.length} session(s) are still owner="${daemonOwned[0].owner}", not "keeper"`);
  }
  const extraFromKeeper = [...keeperById.keys()].filter((id) => !expected.has(id));
  if (extraFromKeeper.length > 0) {
    reasons.push(`keeper OwnershipList listed ${extraFromKeeper.length} session(s) this run did not spawn`);
  }

  if (!inventory || inventory.liveLocal !== sessions) {
    reasons.push(`control Inventory live_local=${inventory?.liveLocal}, expected ${sessions}`);
  }
  if (inventory && (inventory.liveRemote !== 0 || inventory.livePaired !== 0)) {
    reasons.push(
      `control Inventory reports ${inventory.liveRemote} remote and ${inventory.livePaired} paired session(s) in an isolated run`,
    );
  }

  return {
    ok: reasons.length === 0,
    reasons,
    keeperOwned: keeperOwned.length,
    daemonOwned: daemonOwned.length,
    unknown: expected.size - keeperById.size,
  };
}

/** The typed, named failure for a routing shortfall: this is the seam, stated in full. */
export function routingError(reconciliation, sessions) {
  return new UnverifiedError(
    'KEEPER_ROUTING_UNVERIFIED',
    `${reconciliation.keeperOwned} of ${sessions} session(s) are keeper-owned ` +
      `(${reconciliation.daemonOwned} still daemon-owned; ${reconciliation.unknown} unknown), so a ` +
      `"combined" footprint would be a false pass. On macOS the payload gate is already satisfied ` +
      `(the daemon ran from a staged Contents/MacOS bundle with Contents/Resources/keeper), so the ` +
      `shortfall is in the product's routing, not in where this script found the binary. ` +
      `Details: ${reconciliation.reasons.join('; ')}`,
  );
}

// ---------------------------------------------------------------------------------------------
// acceptance arithmetic (pure)
// ---------------------------------------------------------------------------------------------

/** Every sample instant the plan requires, inclusive of t=0 and t=duration. */
export function expectedSampleElapsedMs({ durationMin, intervalMs }) {
  const durationMs = durationMin * 60_000;
  const instants = [];
  for (let t = 0; t < durationMs; t += intervalMs) instants.push(t);
  instants.push(durationMs);
  return instants;
}

/**
 * The rev-5 verdict, from samples alone. Nothing here can see the clock, so the test file can pin
 * the arithmetic exactly. Precedence: unverified > breach > pass, because a run that cannot prove
 * its own coverage must never be reported as a target breach (and certainly never as a pass).
 */
export function evaluateSoak({ samples, options, fullSpec }) {
  const instants = expectedSampleElapsedMs(options);
  const tolerance = options.intervalMs / 2;
  const reasons = [];
  const unverified = [];
  const breaches = [];

  const byElapsed = [...samples].sort((left, right) => left.elapsedMs - right.elapsedMs);
  const nearest = (instant) => {
    let best = null;
    for (const sample of byElapsed) {
      const distance = Math.abs(sample.elapsedMs - instant);
      if (distance <= tolerance && (best === null || distance < Math.abs(best.elapsedMs - instant))) {
        best = sample;
      }
    }
    return best;
  };

  for (const instant of instants) {
    const sample = nearest(instant);
    if (!sample) {
      // Name the sampler's own timing on the miss, so "late" and "never measured" cannot be confused
      // and a slow measurement is visible instead of being blamed on the interval.
      const closest = byElapsed.reduce(
        (best, candidate) =>
          best === null || Math.abs(candidate.elapsedMs - instant) < Math.abs(best.elapsedMs - instant)
            ? candidate
            : best,
        null,
      );
      const detail = closest === null
        ? 'no sample at all'
        : `nearest was ${Math.round(closest.elapsedMs - instant)} ms off (lag ${closest.sampleLagMs ?? 'n/a'} ms, ` +
          `measurement ${closest.sampleCostMs ?? 'n/a'} ms, tick ${closest.tickCostMs ?? 'n/a'} ms)`;
      unverified.push(`no sample within ${tolerance} ms of t=${Math.round(instant / 1000)} s: ${detail}`);
      continue;
    }
    if (!Number.isFinite(sample.daemonRssBytes) || sample.daemonRssBytes <= 0) {
      unverified.push(`sample at t=${Math.round(instant / 1000)} s has no daemon RSS`);
    }
    if (!Number.isFinite(sample.keeperRssBytes) || sample.keeperRssBytes <= 0) {
      unverified.push(`sample at t=${Math.round(instant / 1000)} s has no keeper RSS`);
    }
    if (sample.identityVerified !== true) {
      unverified.push(`sample at t=${Math.round(instant / 1000)} s did not verify both process identities`);
    }
    if (sample.routingVerified !== true) {
      unverified.push(`sample at t=${Math.round(instant / 1000)} s did not verify keeper ownership`);
    }
  }

  // The reference is the sample at t=40 min. Measured on run13, it is not taken too early: 61.02 MiB
  // there against a 60.88 MiB median over every sample from t=10 min on (+0.2 %), so the daemon was
  // already at its steady footprint when the reference was read. It is left as the single instant the
  // plan names; the noise in this measurement is addressed where it is actually introduced, by
  // measuring phys_footprint instead of resident size (see `measureFootprintBytes`).
  const reference = nearest(TREND_FROM_MS);
  if (!reference) {
    unverified.push(`no sample within ${tolerance} ms of t=40 min, which the trend compares against`);
  }

  // Every sample in the last 20 minutes is compared against the reference exactly as the plan states
  // it, band and tolerance untouched: defaulting either would be a policy change, not a fix. What
  // changed is the quantity being compared - phys_footprint, the plan's own basis, not resident size.
  const trend = byElapsed.filter((sample) => sample.elapsedMs >= TREND_FROM_MS);
  if (reference) {
    const limit = reference.totalBytes * TREND_TOLERANCE;
    for (const sample of trend) {
      const deviation = sample.totalBytes - reference.totalBytes;
      if (Math.abs(deviation) > limit) {
        breaches.push({
          kind: 'upward-trend',
          at: sample.at,
          elapsedMs: sample.elapsedMs,
          totalBytes: sample.totalBytes,
          referenceBytes: reference.totalBytes,
          deviation,
          limit,
        });
      }
    }
  }
  for (const sample of byElapsed) {
    if (sample.totalBytes > TARGET_BYTES) {
      breaches.push({
        kind: 'target-exceeded',
        at: sample.at,
        elapsedMs: sample.elapsedMs,
        totalBytes: sample.totalBytes,
        targetBytes: TARGET_BYTES,
      });
    }
  }

  if (!fullSpec) {
    unverified.push(
      'this run is not the plan\'s soak (71 sessions, 60 minutes, 30 s sampling), so it cannot pass',
    );
  }

  const verdict = unverified.length > 0 ? 'unverified' : breaches.length > 0 ? 'breach' : 'pass';
  const exitCode = verdict === 'unverified' ? 4 : verdict === 'breach' ? 1 : 0;
  return { verdict, exitCode, unverified, breaches, trendSamples: trend.length, reference };
}

// ---------------------------------------------------------------------------------------------
// runtime helpers
// ---------------------------------------------------------------------------------------------

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Sleeps toward an absolute deadline in short steps, so a process that dies mid-run is noticed
 * within `stepMs` instead of after the whole interval. `onTick` may throw (e.g. ProcessGoneError).
 */
async function sleepUntil(deadline, stepMs = 250, onTick = null) {
  while (Date.now() < deadline) {
    const remaining = deadline - Date.now();
    await sleep(Math.min(stepMs, remaining));
    if (onTick) onTick();
  }
}

/**
 * Signals `pid` only while it is still exactly the recorded process. Never a name match, and never
 * on a bare pid: a falsy `recorded` means the spawn-time identity was never captured, which is a
 * refusal rather than a licence to signal whatever now holds that pid.
 *
 * `identity` is an identity *reader* (`(pid) => identity | null`), the same seam
 * `survivedTeardown` takes, so both teardown predicates are injectable one way and a test cannot
 * accidentally hand a reader where a `spawnSync` result was expected. Production default is
 * `processIdentity`, which is the `ps` read; `stopRecorded` binds its own `run` into that reader.
 */
export function signalRecorded(recorded, signal, identity = processIdentity, kill = process.kill.bind(process)) {
  if (!recorded) return false;
  const current = identity(recorded.pid);
  if (!identityMatches(recorded, current)) return false;
  try {
    kill(recorded.pid, signal);
    return true;
  } catch {
    return false;
  }
}

/**
 * Graceful stop of recorded processes: SIGTERM, a bounded wait for the recorded identities to go,
 * then SIGKILL on whatever still matches. Nothing else is ever signalled, and nothing is signalled
 * twice: the SIGKILL pass re-checks identity, so a pid reused during the wait is left alone.
 *
 * Every side effect is injectable, because the escalation is the part of this file that must not be
 * taken on faith: `waitMs` bounds the wait, `now` and `sleep` make that bound testable without
 * spending five real seconds, and `run`/`kill` keep `ps` and the signal out of the test.
 */
export async function stopRecorded(
  recorded,
  { waitMs = 5_000, run = spawnSync, kill = process.kill.bind(process), sleep: nap = sleep, now = Date.now } = {},
) {
  const identityOf = (pid) => processIdentity(pid, run);
  const signalled = (recorded ?? []).filter((entry) => signalRecorded(entry, 'SIGTERM', identityOf, kill));
  if (signalled.length === 0) return { signalled: [], escalated: [] };
  const deadline = now() + waitMs;
  while (now() < deadline) {
    if (signalled.every((entry) => !identityMatches(entry, processIdentity(entry.pid, run)))) {
      return { signalled, escalated: [] };
    }
    await nap(100);
  }
  const escalated = signalled.filter((entry) => signalRecorded(entry, 'SIGKILL', identityOf, kill));
  return { signalled, escalated };
}

/**
 * Describes a spawned child whose identity could not be read, for the operator to clear up by hand.
 * It has no start stamp, so it is never signalled - not by pid, and not by parentage either: ppid is
 * evidence about who forked a process, not proof that the process we are looking at is the one we
 * spawned, and a run that cannot prove identity must not act destructively at all.
 *
 * Only the pid, a fixed label and a fixed reason leave this function. No environment, no run root, no
 * socket path and nothing from the runtime dir, so reporting a retained process cannot leak
 * production data.
 */
export function retainedProcess(child, label, reason = 'spawn-time-identity-unreadable') {
  const pid = child?.pid;
  if (!pid) return null;
  return { label, pid, reason };
}

/**
 * The children that may still be alive after this run: exactly those that were spawned but whose
 * identity was never recorded, so they are absent from the teardown list. A child that *was* recorded
 * is stopped by `stopRecorded` and is not retained - reporting it would keep every ordinary run's root
 * forever and downgrade its verdict, which is why recordedness, not the handle, decides.
 */
export function collectRetained({ keeper, keeperRecorded, daemon, daemonRecorded }) {
  return [
    keeperRecorded ? null : retainedProcess(keeper, 'keeper'),
    daemonRecorded ? null : retainedProcess(daemon, 'daemon'),
  ].filter(Boolean);
}

/**
 * A recorded child that is still the same live process after teardown: SIGKILL is not a guarantee, so
 * surviving after `stopRecorded` is possible and must be treated exactly like an unrecorded child -
 * reported, never re-signalled blindly, and never having its root deleted underneath it. Only a
 * *matching* identity counts: a pid that no longer matches means our child is gone and something else
 * inherited the number, which is not ours to report.
 */
export function survivedTeardown(recorded, identity = processIdentity) {
  if (!recorded) return false;
  return identityMatches(recorded, identity(recorded.pid));
}

/**
 * Whether the per-run root may be deleted. Three independent reasons to keep it, any one sufficient:
 *
 * 1. `--keep-root` - the operator asked for the tree.
 * 2. A retained child - it is still alive with its cwd, HOME, TMPDIR and its `--socket` files inside
 *    the root, so deleting it would pull the filesystem out from under a process this run declined to
 *    signal, and its supervisor's sockets would vanish mid-life.
 * 3. The run did not pass - a breach or an unverified run is exactly when the daemon/keeper logs
 *    inside the root are worth reading, and they are not re-creatable.
 *
 * So the root is deleted only for a clean pass with nothing retained. That is deliberately stricter
 * than "delete unless asked": losing 60 minutes of run to a tidy-up is the worse failure.
 */
export function shouldRemoveRoot({
  keepRoot = false,
  retained = [],
  verdict,
  panesConfirmedGone,
  subtreeAbsence,
} = {}) {
  if (keepRoot) return false;
  if (retained.length > 0) return false;
  // Fail closed on panes: a pass whose pane children were never proven gone keeps its root, because
  // those shells hold their cwd, HOME and TMPDIR inside it. Omission means "not proven", not "fine".
  if (panesConfirmedGone !== true) return false;
  // And fail closed on the subtree too, which is the stronger requirement. Knowing the panes are gone
  // does not prove nothing else is still using the root: a child forked by a pane is reparented when
  // its pane dies, so no post-hoc `ps` walk can see it, and a shell forked after the last sample was
  // never observed at all. Absence of a reparented descendant is only provable by pre-established
  // containment (a process group or cgroup this harness owns and can observe empty), which does not
  // exist here: the panes are spawned by the daemon, not by this script. So unless a caller can pass
  // an explicit `'proven'`, the root is retained. Deleting a root a live shell is still sitting in is
  // unrecoverable; keeping a root costs a directory.
  if (subtreeAbsence !== 'proven') return false;
  return verdict === 'pass';
}

/**
 * Session ids whose pane pid is missing or not a positive integer. A pane pid is *third-party
 * metadata*: the daemon reports it, so it is useful for accounting ("did the 71 children exist and
 * did they go away") but it is never treated as spawn-time identity and is never used as authority to
 * signal anything. A pid the daemon hands us could belong to any process by the time we read it.
 */
export function accountPanes(liveSessions, expectedIds) {
  const byId = new Map();
  for (const entry of liveSessions ?? []) {
    if (!entry || typeof entry.backendId !== 'string') continue;
    byId.set(entry.backendId, entry);
  }
  const panes = [];
  const unaccounted = [];
  for (const id of expectedIds ?? []) {
    const entry = byId.get(id);
    const pid = entry?.localChildPid;
    if (Number.isInteger(pid) && pid > 0) panes.push({ sessionId: id, panePid: pid });
    else unaccounted.push(id);
  }
  return { panes, unaccounted, accounted: unaccounted.length === 0 };
}

/**
 * The post-shutdown pane census: for each pane pid the daemon reported, was that process actually
 * gone once the keeper was shut down?
 *
 * This exists because `accountPanes` runs *before* teardown. A pre-teardown census says the shells
 * were alive and accounted for; it says nothing about whether they were reaped afterwards, so a pass
 * could delete a root that live shells were still sitting in. This answers the after-the-fact
 * question, and answers it three ways rather than two:
 *
 *   - `gone`     - `ps` found no such pid. The child is confirmed reaped.
 *   - `alive`    - `ps` found a process at that pid. Treated as *possibly ours and possibly not*: the
 *                  pid is third-party metadata, so it is never signalled and never killed. It keeps
 *                  the root and blocks a pass.
 *   - `unknown`  - the read failed, or the pid is not a positive integer. Not proof of anything, so it
 *                  also keeps the root and blocks a pass.
 *
 * Only all-gone yields `confirmedGone: true`. "Unknown is not gone" is the whole contract: the one
 * unsafe inference would be reading a failed `ps` as an empty process table.
 *
 * `probe` must be the tri-state `probeProcess`, not `processIdentity`: the latter returns `null` for
 * both "no such process" and "the read failed", and treating those alike here is exactly the defect
 * that let a surviving pane's root be deleted. A probe that reports only two states is rejected
 * loudly rather than quietly interpreted, and a probe that *throws* is a failed read on the same
 * terms: it is recorded as `unknown` (never `gone`) instead of escaping teardown, because a throw
 * here would abandon the summary that explains why the root was kept.
 *
 * `descendants` is the set of descendant pids observed while the panes were still alive (captured by
 * `captureDescendants`). It is **blocking evidence only, never a licence**. A descendant that is alive
 * or unreadable blocks the pass, because the root is then still in use or we cannot tell. Its absence
 * proves nothing about the subtree: a parent-chain walk cannot see a shell that was reparented after
 * its pane died, nor one forked in the window after the last sample, so this census never claims the
 * subtree is gone. `subtreeAbsence` is therefore always reported as `'unproven'` here, and the caller
 * must retain the run root on that basis.
 *
 * `probe` is injectable so the classification is testable without touching real pids.
 */
export function censusPanesAfterShutdown(panes, probe = probeProcess, descendants = []) {
  const gone = [];
  const alive = [];
  const unknown = [];
  const classify = (pid, extra) => {
    // A probe that throws is a *failed read*, exactly like an unreadable `ps`: it says nothing about
    // whether the process exists, so it must land in `unknown` and keep the root. This runs inside
    // teardown, where an escaping throw would abandon the summary and leave the root in place with no
    // explanation of why, so the failure is classified rather than propagated.
    let result;
    try {
      result = probe(pid);
    } catch (error) {
      unknown.push({ ...extra, reason: `pane-liveness-probe-threw: ${String(error?.message ?? error)}` });
      return;
    }
    if (!result || typeof result.state !== 'string') {
      unknown.push({ ...extra, reason: 'pane-liveness-probe-invalid' });
      return;
    }
    if (result.state === 'alive') {
      // May be a reused pid unrelated to this run, but a shell this run started is exactly what must
      // not be deleted out from under, so it counts as alive either way. Recorded, never signalled.
      alive.push({ ...extra, reason: extra.reason ?? 'pane-still-live-after-shutdown' });
      return;
    }
    if (result.state === 'gone') {
      gone.push({ ...extra });
      return;
    }
    // 'unknown' and any unrecognised state both land here: a state this code does not understand is
    // not permission to delete anything.
    unknown.push({ ...extra, reason: `pane-liveness-unreadable: ${String(result.reason ?? result.state)}` });
  };

  for (const pane of panes ?? []) {
    const pid = pane?.panePid;
    if (!Number.isInteger(pid) || pid <= 0) {
      unknown.push({ ...pane, reason: 'pane-pid-missing' });
      continue;
    }
    classify(pid, { ...pane });
  }

  // Descendants are checked on the same terms as panes, and are never signalled. They are keyed
  // separately so a surviving grandchild is visible in the summary even when every pane is gone.
  const descendantGone = [];
  for (const entry of descendants ?? []) {
    const pid = entry?.pid;
    const extra = { line: entry?.line ?? 'descendant', pid, sessionId: entry?.sessionId };
    if (!Number.isInteger(pid) || pid <= 0) {
      unknown.push({ ...extra, reason: 'descendant-pid-missing' });
      continue;
    }
    const before = gone.length;
    classify(pid, extra);
    if (gone.length > before) descendantGone.push(extra);
  }

  return {
    checkedAt: new Date().toISOString(),
    gone,
    alive,
    unknown,
    descendantsChecked: (descendants ?? []).length,
    descendantsGone: descendantGone,
    // Every *known* pane proven gone, and no known descendant blocking. This is a statement about the
    // pids this run was told about, not about the process subtree - a parent-chain walk cannot prove
    // absence of a reparented or late-forked child. An empty census is not confirmation either: a run
    // that never learned its pane pids has proven nothing, so it fails closed.
    confirmedGone: alive.length === 0 && unknown.length === 0 && gone.length > 0,
    // Hard-coded, not derived: see the note above. Claiming proof here is the defect this field exists
    // to prevent, so no combination of panes and descendants may ever turn this into 'proven'.
    subtreeAbsence: 'unproven',
  };
}

/**
 * Descendant pids of the panes, read while they are still alive.
 *
 * This has to happen before shutdown: once a pane is reaped its children are reparented to init and
 * the link back to this run is lost forever, so `ps` afterwards cannot prove the subtree is gone. The
 * reading is `ps -A` filtered by parentage, and it is best-effort by design: a failure yields an empty
 * list, which does not weaken the post-shutdown verdict because `censusPanesAfterShutdown` already
 * fails closed on panes alone.
 */
export function captureDescendants(panePids, run = spawnSync) {
  const roots = new Set((panePids ?? []).filter((pid) => Number.isInteger(pid) && pid > 0));
  if (roots.size === 0) return [];
  let result;
  try {
    result = run('ps', ['-A', '-o', 'pid=,ppid='], { encoding: 'utf8', env: { ...process.env, ...PS_ENV_PIN } });
  } catch {
    return [];
  }
  if (!result || typeof result.stdout !== 'string' || result.status !== 0) return [];
  const childrenOf = new Map();
  for (const line of result.stdout.split('\n')) {
    const match = /^\s*(\d+)\s+(\d+)\s*$/.exec(line);
    if (!match) continue;
    const pid = Number(match[1]);
    const ppid = Number(match[2]);
    if (!childrenOf.has(ppid)) childrenOf.set(ppid, []);
    childrenOf.get(ppid).push(pid);
  }
  // Transitive closure, with a visited set so a pid cycle (or a mid-read reparenting race) cannot
  // spin. Depth is bounded by `visited`, not by a counter.
  const seen = new Set();
  const queue = [...roots];
  while (queue.length > 0) {
    const parent = queue.shift();
    for (const child of childrenOf.get(parent) ?? []) {
      if (seen.has(child) || roots.has(child)) continue;
      seen.add(child);
      queue.push(child);
    }
  }
  return [...seen].map((pid) => ({ pid, line: 'descendant' }));
}

/**
 * Registers the only cleanup that can run on Ctrl-C / SIGTERM / SIGHUP. A signal handler cannot make
 * cleanup a guarantee: `SIGKILL` and `SIGSTOP` are not deliverable to a handler, and the harness being
 * OOM-killed or killed with `SIGKILL` bypasses this entirely, leaving children orphaned with the root
 * in place. That is stated in the run's own report rather than papered over - see the summary's
 * `interruption` field. What this does buy: the ordinary interrupts (Ctrl-C, `timeout`, CI cancel,
 * SIGHUP from a closed terminal) reach the same bounded teardown as every other exit path.
 *
 * `handlers` and `target` are injectable so the wiring is testable without a real signal.
 */
export function registerInterruptionHandlers({
  target = process,
  handlers = ['SIGINT', 'SIGTERM', 'SIGHUP'],
  onInterrupt,
  onRepeat,
}) {
  let fired = false;
  let count = 0;
  const listener = (signal) => {
    count += 1;
    // A repeat never starts a second teardown, but it must not be ignored either: the listener stays
    // attached through teardown and *absorbs* the signal, which is the only thing that keeps the
    // default disposition (immediate death mid-cleanup) away from a second Ctrl-C.
    if (fired) {
      if (onRepeat) {
        try { onRepeat(signal, count); } catch { /* recording a repeat must not break teardown */ }
      }
      return;
    }
    fired = true;
    onInterrupt(signal);
  };
  const installed = handlers.filter((signal) => {
    try {
      target.on(signal, listener);
      return true;
    } catch {
      return false; // a platform without SIGHUP still gets the others
    }
  });
  return {
    installed,
    dispose: () => {
      for (const signal of installed) {
        try { target.removeListener(signal, listener); } catch { /* already gone */ }
      }
    },
  };
}

/**
 * Copies the daemon/keeper logs out of the run root into the evidence dir, so a breach or an
 * interrupted run still has the stderr that explains it. The root is kept for every non-pass anyway
 * (see shouldRemoveRoot), so this is the second layer: the evidence dir is the durable one, it is
 * outside the root, and it survives a root that is later cleaned by hand. Returns the copied names, or
 * `{ copied: [], error }` - never throws, because it runs inside teardown.
 */
export function preserveLogs(outDir, layout, copy = copyFileSync, exists = existsSync) {
  const copied = [];
  try {
    for (const name of ['daemon.stdout.log', 'daemon.stderr.log', 'keeper.stderr.log']) {
      const source = path.join(layout.logs, name);
      if (!exists(source)) continue;
      copy(source, path.join(outDir, name));
      copied.push(name);
    }
    return { copied };
  } catch (error) {
    return { copied, error: String(error?.message ?? error) };
  }
}

/**
 * Serialises "may a new child be started?" against "teardown has begun" (R1).
 *
 * The bug this closes is a check-then-spawn race, so the fix must not itself be a check-then-spawn
 * race: `admit` runs the closed check and the spawn in the same synchronous turn, with no `await`
 * between them, so a signal handler delivered on this turn cannot interleave. That is the whole
 * contract, and the reason `admit` returns a plain object rather than a promise - an async admit
 * would reintroduce exactly the window it exists to close.
 *
 * `close` is called synchronously at the top of `finish`, before teardown awaits anything, so the
 * gate is shut the moment teardown is decided rather than when it completes. `spawnFn` is injected
 * so the ordering is testable without starting a process.
 *
 * Why an admitted child cannot be missed by `collectRetained`: the assignment of the returned child
 * to `keeper`/`daemon` is the very next statement, with no `await` between. Node runs signal handlers
 * on the event loop, never preempting synchronous code, so teardown cannot observe "gate admitted, handle
 * not yet assigned". The reachable interleavings are all at `await` points, and those are the ones the
 * gate refuses.
 */
export function createSpawnGate({ spawnFn, clock = () => Date.now() } = {}) {
  const spawned = [];
  let closedReason = null;
  return {
    get closed() {
      return closedReason !== null;
    },
    /** Shut the gate. First reason wins; later calls do not overwrite it. */
    close(reason) {
      if (closedReason === null) closedReason = reason ?? 'teardown';
      return { closed: true, reason: closedReason };
    },
    /** Synchronous by contract - see the doc comment. Never awaited, never returns a promise. */
    admit(label, file, args, options) {
      if (closedReason !== null) return { ok: false, label, reason: closedReason, child: null };
      if (typeof spawnFn !== 'function') return { ok: false, label, reason: 'no-spawn-function', child: null };
      const child = spawnFn(file, args, options);
      spawned.push({ label, pid: child?.pid ?? null, at: clock() });
      return { ok: true, label, reason: null, child };
    },
    get spawned() {
      return spawned.map((entry) => ({ ...entry }));
    },
  };
}

/**
 * The command this tick types into one pane: a line that makes the *child* print `bytes` bytes.
 *
 * Why not send the bytes themselves: run14d typed 12 KiB per pane per tick straight at the tty and
 * the line discipline made the harness pay for it twice - a canonical line is capped at MAX_CANON
 * (1024 B on macOS) and the tty echoes every byte back - so tick cost went from ~15 s to 139-366 s,
 * every 30 s sample was missed and the run could not be verified. Here the harness types ~38 B and
 * the child produces the volume, which is exactly what the OUTPUT ring, the keeper's replay log and
 * the checkpoint hold, so the footprint being measured is the product's.
 *
 * `tr` maps NUL to `x` and emits no newline, so ONLCR cannot turn one byte into two and the produced
 * count is exactly the number asked for; `head -c` is both BSD and GNU. Returns null for a zero
 * budget, where the caller writes nothing at all.
 */
export function outputCommand(bytes) {
  if (!(bytes > 0)) return null;
  return `head -c ${bytes} /dev/zero | tr '\\000' x`;
}

/** Bytes the workload types into one pane in one tick, including the line's terminating CR. */
export function workloadInputBytes(bytesPerTick) {
  const command = outputCommand(bytesPerTick);
  return command === null ? 0 : Buffer.byteLength(command, 'utf8') + 1;
}

/**
 * Refuse a workload that would put the metric's own volume through the tty write path.
 *
 * The soak exists to measure what the daemon and keeper retain, so its per-tick input must stay a
 * short command: past MAX_CANON the terminal driver changes behaviour and the echo cost dominates
 * the measurement - that is run14d, whose 366 s ticks were the harness, not the product. Cheap to
 * check, and it is the exact regression that made the previous run unverifiable.
 */
export function assertInputStaysSmall(options) {
  const bytes = workloadInputBytes(options.bytesPerTick);
  if (bytes > MAX_INPUT_BYTES_PER_TICK) {
    throw new GuardError(
      `the per-tick workload types ${bytes} B into each pane, above the ${MAX_INPUT_BYTES_PER_TICK} B ` +
        `input budget: --bytes-per-tick is OUTPUT the child must produce, not input the harness writes`,
    );
  }
}

/**
 * Produced output per session between two Keeper `OwnershipList` reads.
 *
 * `sequenceOffset` is the keeper replay log's absolute head (`replay.rs::head_offset`, advanced by
 * `push` for every byte the child prints), so the delta between two samples is the volume the
 * workload actually produced - measured through the same verb the run already reads each tick, with
 * no extra RPC and no trust in the harness's own write accounting. A backwards offset is a reset or
 * a replaced session, and is reported rather than silently counted as zero output.
 */
export function outputDelta(keeperSessions, previousOffsets, expectedIds) {
  const offsets = new Map();
  let bytes = 0;
  let reason = null;
  for (const id of expectedIds) {
    const entry = (keeperSessions ?? []).find((row) => row?.id === id);
    const offset = entry?.sequenceOffset;
    if (!Number.isInteger(offset) || offset < 0) {
      reason ??= `${id} has no readable sequenceOffset`;
      continue;
    }
    offsets.set(id, offset);
    const previous = previousOffsets?.get(id);
    // First sighting of this session: there is no earlier head to subtract, so it contributes no
    // delta. The baseline read after spawning makes every measured sample a real first delta.
    if (previous === undefined) continue;
    if (offset < previous) {
      reason ??= `${id} sequenceOffset went backwards (${previous} -> ${offset})`;
      continue;
    }
    bytes += offset - previous;
  }
  return { offsets, bytes, reason };
}

// ---------------------------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------------------------

/**
 * `deps` exists only so the startup ordering is testable: `spawnFn` and `signalTarget` are the two
 * things R1-a is about (does a handler exist at the moment a child is started?). Production passes
 * neither and gets the real `spawn` and `process`.
 */
async function main(argv = process.argv.slice(2), env = process.env, deps = {}) {
  const spawnChild = deps.spawnFn ?? ((...callArgs) => spawn(...callArgs));
  const signalTarget = deps.signalTarget ?? process;
  // Injectable for the same reason: a test that provokes a real interrupt must observe the exit code
  // rather than have `process.exit` tear down the test runner underneath it.
  const exitProcess = deps.exit ?? ((code) => process.exit(code));
  const options = parseArgs(argv, env);
  if (options.help) {
    console.log(
      'usage: node scripts/soak-pty-footprint.mjs --daemon-bin <path> --keeper-bin <path>\n' +
        '         [--sessions 71] [--duration-min 60] [--interval-ms 30000]\n' +
        '         [--bytes-per-tick 12288] [--out <dir>] [--keep-root]\n\n' +
        'Runs the plan rev-5 footprint soak in a private root. Pass requires exactly\n' +
        '71 sessions, 60 minutes and 30 s sampling; any other shape exits 4 (unverified).\n' +
        '--bytes-per-tick is the OUTPUT each pane must produce per tick; the harness types\n' +
        'only a short command (at most 256 B) to make the child print it.',
    );
    return 0;
  }

  const runId = `soak-${Date.now().toString(36)}-${process.pid}`;
  // Same rule as the phase3/phase4 acceptance runs: a UDS path is capped at 103 bytes, and on macOS
  // `os.tmpdir()` is the long `/var/folders/.../T`, which leaves no room for the sockets under this
  // root. `/tmp` is short. Elsewhere `os.tmpdir()` stays.
  const root = mkdtempSync(path.join(process.platform === 'darwin' ? '/tmp' : os.tmpdir(), 'ferryx-soak-'));
  const layout = ensureLayout(assertIsolated(makeLayout(root)));
  const childEnv = isolatedEnv(layout, runId, env, options.sessions);
  // The same pair `scrollbackEnv` put in `childEnv`, kept as numbers so the summary can state the
  // ceiling the run actually configured rather than re-deriving it from a string. Named for the bound,
  // not `retained`: `finish` has its own `retained` (the process list it could not account for), and a
  // shadow there would put this declaration in its temporal dead zone.
  const retainedBoundConfig = retainedBudgetFor(options.sessions);
  for (const key of ['FERRYX_RUNTIME_DIR', 'FERRYX_LEGACY_RUNTIME_DIR', 'FERRYX_DATA_DIR', 'FERRYX_SESSION_DIR', 'HOME']) {
    if (!isInside(childEnv[key], layout.root)) {
      throw new GuardError(`${key}=${childEnv[key]} escaped the run root ${layout.root}`);
    }
  }
  assertIsolated(layout, childEnv);

  const requestedDaemonBin = path.resolve(options.daemonBin);
  const keeperBin = path.resolve(options.keeperBin);
  // The daemon must run from `Contents/MacOS` with the keeper at `Contents/Resources/keeper`, or
  // `ensure_keeper` answers `PayloadAbsent` and no session can ever be keeper-owned. `daemonBin` below
  // is the executable this run actually spawns; `requestedDaemonBin` is what the operator supplied and
  // is kept in the evidence so the two are never confused.
  const daemonBin = stageSoakDaemon(layout, requestedDaemonBin, keeperBin);
  const outDir = path.resolve(options.out ?? path.join(process.cwd(), '.omo', 'evidence', 'pty-footprint-soak', runId));
  if (isInside(outDir, layout.root)) {
    throw new GuardError(
      `--out ${outDir} is inside the run root ${layout.root}, which teardown deletes; evidence would be destroyed`,
    );
  }
  mkdirSync(outDir, { recursive: true, mode: 0o700 });
  const samplesPath = path.join(outDir, 'footprint-samples.jsonl');
  const summaryPath = path.join(outDir, 'footprint-summary.json');
  writeFileSync(samplesPath, '');
  writeFileSync(path.join(outDir, 'isolation.json'), `${JSON.stringify({ runId, layout, envKeys: Object.keys(childEnv).filter((key) => key.startsWith('FERRYX_') || key === 'HOME' || key.startsWith('XDG_') || key === 'TMPDIR').sort(), daemonBin, requestedDaemonBin, daemonBundled: daemonBin !== requestedDaemonBin, keeperBin, options }, null, 2)}\n`);

  const startedAt = new Date();
  const startedMs = startedAt.getTime();
  const samples = [];
  let keeperRecorded = null;
  let daemonRecorded = null;
  let driveStatus = null;
  let finishing = null;
  let interruption = null;
  let paneState = { panes: [], unaccounted: [], accounted: false, checkedAt: null };
  // Descendant pids observed while the panes were still alive, accumulated across samples. A pane can
  // be reaped while a shell it forked keeps running inside the run root, so the panes alone do not
  // describe the subtree; this is captured before shutdown because afterwards the link is lost.
  const descendantState = [];
  // Produced-output accounting, declared here rather than beside the sampling loop because `finish`
  // can run before that loop is ever reached (a daemon that never became ready still writes a
  // summary) and the summary reports these. `producedBytes` is what the panes actually printed, read
  // from the keeper's replay heads; `ticksWritten` is how many commands were typed. A run whose panes
  // produced nothing must not pass on an empty - trivially flat - ring.
  let producedBytes = 0;
  let ticksWritten = 0;
  let lastOffsets = new Map();
  // Declared here, not at first use, because `finish` closes over them and can run before the keeper is
  // even spawned ("keeper-spawn-failed"); a `let` below would be a temporal-dead-zone throw in teardown.
  let keeperConnection = null;
  let keeperProof = null;
  let interruptionHandlers = null;
  // The soak clock's origin, assigned when the sampling phase begins. Declared here for the same
  // reason as the three above: `finish` closes over it and can run long before sampling starts
  // ("keeper-spawn-failed", "workload-failed"), where a `const` below would make reading it a
  // temporal-dead-zone throw inside teardown and lose the summary this run exists to leave behind.
  let samplingStartedAt = null;
  let samplingStartedMs = null;
  // The one gate every spawn passes through. It exists because the interrupt handlers are installed
  // before the daemon is spawned: without it, a signal landing in that window would start teardown
  // and then the code would go on to spawn a daemon that teardown had already snapshotted as absent,
  // leaving an orphan nobody reports. Shut synchronously at the top of `finish`.
  const spawnGate = createSpawnGate({ spawnFn: spawnChild });

  const daemonOut = createWriteStream(path.join(layout.logs, 'daemon.stdout.log'), { flags: 'a' });
  const daemonErr = createWriteStream(path.join(layout.logs, 'daemon.stderr.log'), { flags: 'a' });
  const keeperErr = createWriteStream(path.join(layout.logs, 'keeper.stderr.log'), { flags: 'a' });
  let daemon = null;
  let keeper = null;
  let daemonTail = '';
  let keeperTail = '';

  const finish = async (verdict, exitCode, exitReason, extra = {}) => {
    // Interruption keeps the first verdict that reaches teardown. A Ctrl-C that lands mid-run asks for
    // cleanup, not for a second, competing cleanup: the second call waits on the first and reports it.
    if (finishing) return finishing;
    finishing = (async () => {
    // Shut the spawn gate synchronously, before this teardown awaits anything. Everything below runs
    // after at least one await, and the retention snapshot below is only honest if no child can be
    // started after it. Both the direct `finish` calls and the signal handler pass through here.
    spawnGate.close(exitReason);
    // The handlers deliberately stay attached for the whole of teardown, and are disposed only at the
    // very end of this function. Disposing them here was a defect: after `dispose` a second Ctrl-C hit
    // the default disposition and killed the process mid-cleanup, leaving children unsignalled and the
    // summary half-written. Keeping them attached makes a repeat signal *absorbed* (`onRepeat` below)
    // rather than fatal, and `finish` is idempotent, so it can never start a competing teardown.
    // `repeats` is a plain array because `interruption` is written into the summary as JSON, where a
    // Set would silently serialise as `{}` and lose the record it exists to keep.
    if (interruption) interruption.repeats = [];
    // Children absent from the teardown list because their identity was never captured. They are
    // never signalled, so they outlive the run and are handed to the operator.
    const unrecorded = collectRetained({ keeper, keeperRecorded, daemon, daemonRecorded });
    let finalVerdict = verdict;
    let finalExitCode = exitCode;
    let finalReason = exitReason;
    // A run that leaves a live process behind is not a clean run, so a pass is downgraded below.
    if (unrecorded.length > 0 && verdict === 'pass') {
      finalVerdict = 'unverified';
      finalExitCode = 4;
      finalReason = 'process-retained';
    }
    const peak = samples.reduce(
      (acc, sample) => ({
        totalBytes: Math.max(acc.totalBytes, sample.totalBytes),
        daemonRssBytes: Math.max(acc.daemonRssBytes, sample.daemonRssBytes ?? 0),
        keeperRssBytes: Math.max(acc.keeperRssBytes, sample.keeperRssBytes ?? 0),
        daemonFootprintBytes: Math.max(acc.daemonFootprintBytes, sample.daemonFootprintBytes ?? 0),
        keeperFootprintBytes: Math.max(acc.keeperFootprintBytes, sample.keeperFootprintBytes ?? 0),
      }),
      { totalBytes: 0, daemonRssBytes: 0, keeperRssBytes: 0, daemonFootprintBytes: 0, keeperFootprintBytes: 0 },
    );
    const summary = {
      schemaVersion: SAMPLE_SCHEMA_VERSION,
      runId,
      verdict: finalVerdict,
      exitReason: finalReason,
      exitCode: finalExitCode,
      fullSpec: isFullSpec(options),
      targetBytes: TARGET_BYTES,
      plan: {
        sessions: PLAN_SESSIONS,
        durationMin: PLAN_DURATION_MIN,
        intervalMs: PLAN_INTERVAL_MS,
        trendFromMs: TREND_FROM_MS,
        tolerance: TREND_TOLERANCE,
      },
      options,
      startedAt: startedAt.toISOString(),
      endedAt: new Date().toISOString(),
      durationMs: Date.now() - startedMs,
      isolation: {
        root: layout.root,
        runtimeDir: layout.runtime,
        daemonSocket: layout.daemonSocket,
        controlSocket: layout.controlSocket,
        keeperSocket: layout.keeperSocket,
        dataDir: layout.data,
        sessionsDir: layout.sessions,
        home: layout.home,
        productionRuntimeDir: productionRuntimeDir(env),
        // The build the operator supplied, and the executable this run actually spawned. They differ on
        // macOS by design: the daemon only finds its keeper payload inside `Contents/MacOS`.
        requestedDaemonBin,
        daemonBin,
        daemonBundled: daemonBin !== requestedDaemonBin,
      },
      processes: { keeper: keeperRecorded, daemon: daemonRecorded },
      retainedProcesses: unrecorded,
      panes: paneState,
      interruption,
      peak,
      sampleCount: samples.length,
      // What the workload did, stated as measured rather than as configured: `producedBytes` comes from
      // the keeper's replay heads, `inputBytesPerTick` from the command actually typed. A reader can
      // see that the 12 KiB per tick was printed by the child and not pushed through the tty.
      workload: {
        bytesPerTick: options.bytesPerTick,
        inputBytesPerTick: workloadInputBytes(options.bytesPerTick),
        inputCommand: outputCommand(options.bytesPerTick),
        ticksWritten,
        producedBytes,
      },
      // The bound the run gave the daemon, so the evidence states the reduced ceiling the trend was
      // judged under instead of leaving a reader to assume the 1536 MiB default. `retainedFillMs` is
      // the time this rate needs to fill the session's retained TEXT share, which is the number
      // `assertSaturatesBeforeTrend` holds to the deadline.
      retainedBound: {
        budgetBytes: retainedBoundConfig.budgetBytes,
        minSessionHistoryBytes: retainedBoundConfig.minSessionHistoryBytes,
        perSessionCeilingBytes: retainedBoundConfig.perSessionCeilingBytes,
        retainedTextCeilingBytes: retainedBoundConfig.retainedTextCeilingBytes,
        retainedFillMs: retainedFillMs(options),
        ringFillMs: ringFillMs(options),
        saturationDeadlineMs: TREND_FROM_MS * SATURATION_DEADLINE_RATIO,
      },
      // Which number the acceptance was judged on, stated on the summary rather than inferred from a
      // log. `phys_footprint` is the plan's basis; `rss` means the tool was unavailable and the run
      // measured something weaker, which the reader is entitled to know before trusting the verdict.
      measurement: samples.some((sample) => sample.measurement === 'phys_footprint')
        ? 'phys_footprint'
        : 'rss',
      sampler: {
        startedAt: samplingStartedAt ? samplingStartedAt.toISOString() : null,
        pendingWorkloadSetupMs: samplingStartedMs === null ? null : samplingStartedMs - startedMs,
        maxLagMs: samples.reduce((acc, sample) => Math.max(acc, sample.sampleLagMs ?? 0), 0),
        maxCostMs: samples.reduce((acc, sample) => Math.max(acc, sample.sampleCostMs ?? 0), 0),
        maxTickCostMs: samples.reduce((acc, sample) => Math.max(acc, sample.tickCostMs ?? 0), 0),
      },
      driveStatus,
      ...extra,
    };

    // First write before teardown: if stopping a child throws, the evidence and the pids we know about
    // are already durable. It is rewritten once more below with the definitive retained set.
    writeFileSync(summaryPath, `${JSON.stringify(summary, null, 2)}\n`);

    // The keeper owns the 71 panes, so the keeper is what releases them: one Shutdown request, sent
    // only through keeperShutdownGate, which proves the peer is the keeper this run spawned (the pid
    // in its HELLO reply) on a socket inside this run's root. No pane pid is ever signalled by this
    // script - the daemon reports those pids, which makes them metadata, not identity.
    const keeperShutdown = await requestKeeperShutdown(keeperConnection, {
      proof: keeperProof,
      layout,
      env,
    });
    summary.keeperShutdown = keeperShutdown;

    try { daemonOut.end(); daemonErr.end(); keeperErr.end(); } catch { /* best effort */ }
    // Teardown is identity-only. `stopRecorded` re-reads each recorded process and signals it solely
    // while the recorded start stamp and executable still match, escalating SIGTERM to SIGKILL; a
    // process with no recorded identity is not in the list and is never signalled, because a bare pid
    // (or a ppid) cannot distinguish our child from whatever inherited that pid. Such a child is
    // instead reported for a human, and the run says so in its verdict rather than leaving a process
    // running silently. There is deliberately no catch-all `child.kill()` sweep here.
    await stopRecorded([daemonRecorded, keeperRecorded].filter(Boolean));
    // The pre-teardown census (`paneState`) only says the shells were alive and accounted for. What
    // decides whether the root may be deleted is whether they are gone *now*, after the keeper that
    // owned them was shut down. Pane pids are third-party metadata, so this only ever reads them: a
    // live-but-unproven pane is reported and keeps the root, and is never signalled.
    const paneCensus = censusPanesAfterShutdown(paneState.panes, probeProcess, descendantState);
    summary.paneCensus = paneCensus;
    // A pane that is alive or of unknown fate is *reported* on the same terms as a retained child: the
    // shells hold cwd, HOME and TMPDIR inside the root. Nothing here is ever signalled - the pid is
    // third-party metadata. Built directly rather than through `retainedProcess`, which returns null
    // for a missing pid and would silently drop exactly the unknown case this is meant to surface.
    const survivingPanes = [...paneCensus.alive, ...paneCensus.unknown].map((pane) => ({
      label: `pane:${pane.sessionId ?? 'unknown'}`,
      pid: Number.isInteger(pane.panePid) && pane.panePid > 0 ? pane.panePid : null,
      reason: pane.reason,
    }));
    // SIGKILL is not a guarantee, so a *recorded* child that is still the identical process counts as
    // retained too: it must be reported, never re-signalled blind, and must keep its root.
    const survivors = [
      survivedTeardown(keeperRecorded) ? retainedProcess(keeper, 'keeper', 'survived-teardown') : null,
      survivedTeardown(daemonRecorded) ? retainedProcess(daemon, 'daemon', 'survived-teardown') : null,
    ].filter(Boolean);
    const retained = [...unrecorded, ...survivors, ...survivingPanes];
    if (retained.length > 0) {
      summary.retainedProcesses = retained;
      if (summary.verdict === 'pass') {
        summary.verdict = 'unverified';
        summary.exitCode = 4;
        summary.exitReason = survivingPanes.length > 0 ? 'panes-retained' : 'process-retained';
        finalVerdict = summary.verdict;
        finalExitCode = summary.exitCode;
        finalReason = summary.exitReason;
      }
      writeFileSync(summaryPath, `${JSON.stringify(summary, null, 2)}\n`);
    }

    // The pre-teardown census is kept as its own recorded fact rather than as a verdict: it says the
    // shells were visible *before* shutdown, which is a different claim from the post-shutdown one
    // above. It still fails a pass closed when it found sessions it could not attribute to a pid,
    // because that is an accounting hole this run cannot see past.
    if (!paneState.accounted && summary.verdict === 'pass') {
      summary.verdict = 'unverified';
      summary.exitCode = 4;
      summary.exitReason = 'panes-unaccounted';
      finalVerdict = summary.verdict;
      finalExitCode = summary.exitCode;
      finalReason = summary.exitReason;
    }
    // Post-shutdown, a pass additionally requires every known pane proven gone. `alive` and `unknown`
    // both already reached `retained`, so this is the belt to that suspenders: it states the reason
    // precisely on the summary instead of leaving the operator to infer it from a retention list.
    if (summary.verdict === 'pass' && !paneCensus.confirmedGone) {
      summary.verdict = 'unverified';
      summary.exitCode = 4;
      summary.exitReason = 'panes-not-proven-gone';
      finalVerdict = summary.verdict;
      finalExitCode = summary.exitCode;
      finalReason = summary.exitReason;
    }
    // Evidence outlives a deleted root: on a non-pass the daemon/keeper logs inside the root are the
    // only explanation of the result, so they are copied into the evidence dir before any deletion.
    summary.logs = preserveLogs(outDir, layout);
    writeFileSync(summaryPath, `${JSON.stringify(summary, null, 2)}\n`);

    for (const entry of retained) {
      console.log(`RETAINED ${entry.label} pid ${entry.pid}: ${entry.reason}; not signalled, clear it manually`);
    }

    console.log(`${finalVerdict.toUpperCase()} ${summaryPath}`);
    console.log(
      `peak total ${(peak.totalBytes / MIB).toFixed(1)} MiB ` +
        `(daemon ${(peak.daemonRssBytes / MIB).toFixed(1)} + keeper ${(peak.keeperRssBytes / MIB).toFixed(1)}), ` +
        `${samples.length} samples, target ${(TARGET_BYTES / MIB).toFixed(0)} MiB, reason ${finalReason}`,
    );
    if (extra.reasons?.length > 0) for (const reason of extra.reasons) console.log(`  - ${reason}`);
    // The only place the run root is ever deleted. Kept for --keep-root, for any retained child, and
    // for every non-pass verdict, because the logs that explain a breach live inside it.
    const removeRoot = shouldRemoveRoot({
      keepRoot: options.keepRoot,
      retained,
      verdict: finalVerdict,
      panesConfirmedGone: paneCensus.confirmedGone,
      subtreeAbsence: paneCensus.subtreeAbsence,
    });
    if (removeRoot) {
      try { rmSync(layout.root, { recursive: true, force: true }); } catch { /* best effort */ }
    } else if (options.keepRoot) console.log(`run root kept: ${layout.root}`);
    else console.log(`run root kept: ${layout.root} (${retained.length > 0 ? 'a retained child may still be using it' : `verdict ${finalVerdict} keeps its logs`})`);
    // Stated unconditionally, because it is a property of the harness rather than of this run: no
    // amount of pane accounting proves the process subtree empty, so the root is never reclaimed.
    console.log(`run root retention: subtree absence is unproven (no containment to observe empty), so the root is kept`);
    // Teardown is complete and the exit code is decided, so this is the first point at which the
    // handlers may be detached: everything before it that a second signal could have interrupted is
    // done, and here there is nothing left to orphan.
    if (interruptionHandlers) interruptionHandlers.dispose();
    return finalExitCode;
    })();
    return finishing;
  };

  // ---- interruption: the only cleanup that can run when the harness is told to stop -----------
  // Registered BEFORE the first spawn (R1-a). It used to sit after the keeper start, which left the
  // keeper spawn, its identity read and the HELLO handshake with no handler installed: a SIGTERM
  // landing in that window took the default disposition, so this process died immediately, the
  // already-spawned keeper was orphaned inside its isolated root, and no summary was written at all.
  // Registering first makes every interrupted run reach the same bounded teardown, including one
  // interrupted before the keeper exists. This still cannot cover SIGKILL/OOM of this process: no
  // handler runs then, children are orphaned and the root stays behind. That limit is recorded in
  // every summary as `interruption.limits` instead of being implied away.
  interruptionHandlers = registerInterruptionHandlers({
    target: signalTarget,
    onInterrupt: (signal) => {
      interruption = {
        signal,
        at: new Date().toISOString(),
        limits: 'a SIGKILL or OOM kill of this process runs no handler, so children and the run root survive',
      };
      // Fire and forget: the process must stay alive while teardown runs. `finish` is idempotent, so a
      // second signal during teardown cannot start a competing cleanup.
      finish('unverified', 4, 'interrupted', { reasons: [`received ${signal} before the run completed`] })
        .then((code) => exitProcess(code))
        .catch((error) => {
          console.error(`teardown after ${signal} failed: ${String(error?.message ?? error)}`);
          exitProcess(2);
        });
    },
    // A repeat signal is absorbed, not acted on: teardown is already running and a second one would
    // compete with it. Recording it keeps the fact visible in the summary instead of discarding it.
    onRepeat: (signal) => {
      if (interruption?.repeats) {
        interruption.repeats.push({ signal, at: new Date().toISOString() });
      }
    },
  });

  // ---- keeper first: it is only ours because we started it and it says so --------------------
  const keeperArgs = keeperArgv(layout.keeperSocket);
  // Admission and spawn are one synchronous step: if teardown has begun, no keeper is started at all.
  const keeperAdmission = spawnGate.admit('keeper', keeperBin, keeperArgs, {
    env: childEnv,
    cwd: layout.root,
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  if (!keeperAdmission.ok) {
    return finish('unverified', 4, 'spawn-refused', {
      reasons: [`keeper not started: teardown already began (${keeperAdmission.reason})`],
    });
  }
  keeper = keeperAdmission.child;
  keeper.stderr.on('data', (chunk) => {
    keeperTail = (keeperTail + chunk.toString('utf8')).slice(-8192);
    keeperErr.write(chunk);
  });
  const keeperSpawned = keeper.pid;
  if (!keeperSpawned) {
    return finish('unverified', 4, 'keeper-spawn-failed', { reasons: ['the keeper process could not be started'] });
  }
  // Recorded immediately, before anything else can be observed: pid + OS start stamp + executable.
  const keeperIdentity = processIdentity(keeperSpawned);
  if (!keeperIdentity) {
    return finish('unverified', 4, 'keeper-identity-unreadable', {
      reasons: [`could not read the identity of keeper pid ${keeperSpawned}`],
    });
  }
  keeperRecorded = { ...keeperIdentity, socketPath: layout.keeperSocket, argv: keeperArgs };
  let keeperGone = false;
  keeper.once('exit', () => { keeperGone = true; });

  const keeperDeadline = Date.now() + 15_000;
  let keeperHelloReply = null;
  while (Date.now() < keeperDeadline) {
    if (keeperGone) break;
    if (existsSync(layout.keeperSocket)) {
      try {
        keeperConnection = await connectKeeper(layout.keeperSocket);
        keeperHelloReply = await keeperHello(keeperConnection, keeperSpawned);
        keeperProof = { pid: keeperHelloReply.pid, spawnedPid: keeperSpawned, socketPath: layout.keeperSocket };
        break;
      } catch (error) {
        if (keeperConnection) { keeperConnection.close(); keeperConnection = null; }
        // Only a connection refused/timed out is worth retrying until the deadline. Anything else -
        // an identity mismatch from HELLO, a transport error - is final, and it must still reach
        // teardown: rethrowing here would escape the startup window with a live keeper and no
        // summary. Fail closed instead, through the same `finish` as every other startup failure.
        const retryable = error instanceof Error && /timed out connecting/.test(error.message);
        if (!retryable) {
          return finish('unverified', 4, 'keeper-handshake-failed', {
            reasons: [String(error?.message ?? error)],
          });
        }
      }
    }
    await sleep(100);
  }
  if (!keeperHelloReply) {
    return finish('unverified', keeperGone ? 3 : 4, keeperGone ? 'keeper-exited' : 'keeper-unreachable', {
      reasons: [
        `the keeper at ${layout.keeperSocket} never answered HELLO with the pid we spawned (${keeperSpawned})`,
        keeperTail.trim().split('\n').slice(-5).join(' | '),
      ].filter(Boolean),
    });
  }

  // ---- daemon: `--daemon` is what makes parse_launch_mode pick the daemon ---------------------
  const daemonArgs = daemonArgv();
  // R1's exact window: this is the spawn that used to run after a signal had already started teardown.
  // The gate is rechecked here, synchronously, so a signal delivered at any point before this line
  // means the daemon is never started and teardown's snapshot of "no daemon" stays true.
  const daemonAdmission = spawnGate.admit('daemon', daemonBin, daemonArgs, {
    env: childEnv,
    cwd: layout.root,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  if (!daemonAdmission.ok) {
    return finish('unverified', 4, 'spawn-refused', {
      reasons: [`daemon not started: teardown already began (${daemonAdmission.reason})`],
    });
  }
  daemon = daemonAdmission.child;
  daemon.stdout.on('data', (chunk) => {
    daemonTail = (daemonTail + chunk.toString('utf8')).slice(-8192);
    daemonOut.write(chunk);
  });
  daemon.stderr.on('data', (chunk) => {
    daemonTail = (daemonTail + chunk.toString('utf8')).slice(-8192);
    daemonErr.write(chunk);
  });
  const daemonPid = daemon.pid;
  if (!daemonPid) {
    return finish('unverified', 3, 'daemon-spawn-failed', { reasons: ['the daemon process could not be started'] });
  }
  const daemonIdentity = processIdentity(daemonPid);
  if (!daemonIdentity) {
    return finish('unverified', 4, 'daemon-identity-unreadable', {
      reasons: [`could not read the identity of daemon pid ${daemonPid}`],
    });
  }
  daemonRecorded = { ...daemonIdentity, socketPath: layout.daemonSocket, argv: daemonArgs };
  let daemonGone = false;
  daemon.once('exit', () => { daemonGone = true; });

  let control = null;
  let data = null;
  let dataHandshake = null;
  try {
    const readyDeadline = Date.now() + 30_000;
    while (Date.now() < readyDeadline) {
      if (daemonGone) throw new ProcessGoneError('the daemon exited before announcing readiness');
      if (daemonTail.includes(DAEMON_READY_MARKER) && existsSync(layout.daemonSocket)) break;
      await sleep(100);
    }
    if (!daemonTail.includes(DAEMON_READY_MARKER)) {
      throw new UnverifiedError(
        'DAEMON_NOT_READY',
        `no ${DAEMON_READY_MARKER} on stdout within 30 s (tail: ${daemonTail.slice(-800).trim()})`,
      );
    }
    control = await connectJsonLines(layout.controlSocket);
    await controlHello(control, daemonPid);
    data = await connectJsonLines(layout.daemonSocket);
    dataHandshake = await handshakeWith(data, daemonPid);
  } catch (error) {
    if (control) control.close();
    if (data) data.close();
    return finish('unverified', error instanceof ProcessGoneError ? 3 : 4, 'daemon-unreachable', {
      reasons: [String(error?.message ?? error)],
    });
  }

  // ---- workload: 71 local panes, scripted output in every one --------------------------------
  const spawnedIds = [];
  let routing = null;
  try {
    const registered = await data.call({
      type: 'registerWorkspace',
      workspaceId: 'soak-workspace',
      repoRoot: layout.project,
    });
    if (registered?.type !== 'registerWorkspaceOk') {
      throw new UnverifiedError('DAEMON_WORKLOAD_FAILED', `registerWorkspace: ${JSON.stringify(registered)}`);
    }
    for (let index = 0; index < options.sessions; index += 1) {
      const reply = await data.call({
        type: 'spawn',
        clientRequestId: `${runId}-spawn-${index}`,
        workspaceId: 'soak-workspace',
        cwd: layout.project,
        cols: 120,
        rows: 40,
      });
      if (reply?.type !== 'spawnOk') {
        throw new UnverifiedError(
          'DAEMON_WORKLOAD_FAILED',
          `spawn ${index + 1}/${options.sessions}: ${JSON.stringify(reply)}`,
        );
      }
      spawnedIds.push(reply.sessionId);
    }
    routing = await readRouting({ control, data, keeperConnection, spawnedIds, sessions: options.sessions });
    if (!routing.reconciliation.ok) throw routingError(routing.reconciliation, options.sessions);
    // The produced-output baseline, taken after every pane exists and before the first tick. Without
    // it the first tick's output would be invisible (there is no earlier head to subtract) and the
    // floor below would be short by exactly one tick, which is a claim the run cannot support. An
    // unreadable head here is fatal rather than skipped: a session with no baseline would drop out of
    // every later delta and the shortfall check would never see that pane.
    const baseline = outputDelta(routing.keeperSessions, new Map(), spawnedIds);
    if (baseline.reason) throw new UnverifiedError('WORKLOAD_OUTPUT_UNMEASURED', baseline.reason);
    lastOffsets = baseline.offsets;
  } catch (error) {
    const unverified = error instanceof UnverifiedError;
    return finish('unverified', unverified ? 4 : 3, 'workload-failed', {
      reasons: [String(error?.message ?? error)],
      sessionsSpawned: spawnedIds.length,
      sessions: options.keepRoot ? spawnedIds : undefined,
    });
  }

  // ---- sampling: absolute instants, every sample gated on verified identity and routing -------
  // The soak clock starts HERE, not at process start. `startedAt` predates spawning 71 sessions and
  // the routing read, which on the Darwin lanes took ~15.7 s; anchoring the instants there made t=0
  // unsamplable by construction and pushed every later instant a fixed ~13 s late, so the run
  // reported misses that were an artefact of the clock's origin rather than of the sampler. The 60
  // measured minutes are the soak; workload setup is not part of them.
  const samplingBeganAt = new Date();
  const samplingBeganMs = samplingBeganAt.getTime();
  samplingStartedAt = samplingBeganAt;
  samplingStartedMs = samplingBeganMs;
  const instants = expectedSampleElapsedMs(options);
  const durationMs = options.durationMin * 60_000;
  let exitReason = 'duration-elapsed';
  let tick = 0;
  try {
    for (const instant of instants) {
      await sleepUntil(samplingBeganMs + instant, 250, () => {
        if (daemon.exitCode !== null) throw new ProcessGoneError('the daemon exited during the soak');
        if (keeper.exitCode !== null) throw new ProcessGoneError('the keeper exited during the soak');
      });
      // When this sample's measurement actually began. Compared with `instant` below and published on
      // the sample, so a late sample names its own lateness instead of leaving it to be inferred.
      const sampleBeganMs = Date.now();
      const nowIdentityDaemon = processIdentity(daemonPid);
      const nowIdentityKeeper = processIdentity(keeperSpawned);
      const identitiesOk =
        identityMatches(daemonRecorded, nowIdentityDaemon) && identityMatches(keeperRecorded, nowIdentityKeeper);
      // The plan states its acceptance in footprint - the "Measured facts" row behind the 2.5 GiB
      // target is a `footprint -p` reading - so that is what is measured here: phys_footprint on
      // Darwin, resident size elsewhere. Both are recorded, and `measurement` says which one the
      // sample's `totalBytes` came from, so the evidence never has to be guessed at.
      const daemonFootprintBytes = measureFootprintBytes(daemonPid, spawnSync, process.platform);
      const keeperFootprintBytes = measureFootprintBytes(keeperSpawned, spawnSync, process.platform);
      const measuredOnFootprint = daemonFootprintBytes !== null && keeperFootprintBytes !== null;
      // How long the two footprint reads cost. Published per sample, because a sampler that cannot
      // keep its deadline has to be distinguishable from one that simply missed an instant.
      const measureCostMs = Date.now() - sampleBeganMs;
      // The sample's timestamp is taken HERE, at the moment its bytes were read, not at the end of the
      // tick. Recording the end of the tick made every sample look seconds later than it was - the
      // 71 writes, the routing read and the descendant scan all came before the stamp - so a coverage
      // miss could be manufactured by the harness's own post-measurement work. Coverage is a claim
      // about when the footprint was measured, which is this instant.
      const measuredAtMs = Date.now();

      tick += 1;
      // The workload, written before the sample so this tick's work is counted. What is typed is a
      // ~38 B command, not the volume: the child prints `bytesPerTick` and the tty write path (echo,
      // MAX_CANON) never sees the payload it is being measured on. run14d typed the 12 KiB itself and
      // paid 139-366 s per tick for it, which is what made it unverifiable.
      let writeFailures = 0;
      const command = outputCommand(options.bytesPerTick);
      if (command !== null) {
        const encoded = Buffer.from(`${command}\r`, 'utf8').toString('base64');
        for (let index = 0; index < spawnedIds.length; index += 1) {
          const reply = await data.call({ type: 'write', sessionId: spawnedIds[index], data: encoded });
          if (reply?.type !== 'writeOk') writeFailures += 1;
        }
        ticksWritten += 1;
      }

      const sampledRouting = await readRouting({
        control,
        data,
        keeperConnection,
        spawnedIds,
        sessions: options.sessions,
      });
      // How much output this tick actually produced, and cumulatively since the run began. Read from
      // the keeper's replay heads, which advance once per byte the child prints - so this is the
      // product's own count, not the harness's belief about what it asked for. A head that cannot be
      // read or that goes backwards makes the run unverified instead of contributing a zero.
      const produced = outputDelta(sampledRouting.keeperSessions, lastOffsets, spawnedIds);
      if (produced.reason) throw new UnverifiedError('WORKLOAD_OUTPUT_UNMEASURED', produced.reason);
      lastOffsets = produced.offsets;
      producedBytes += produced.bytes;

      // Accounting only: these pane pids come from the daemon, so they are third-party metadata and are
      // never used to signal. They exist so the summary can state whether all 71 children were visible,
      // and the run fails closed if they were not - it never claims a clean teardown it cannot see.
      paneState = {
        ...accountPanes(sampledRouting.liveSessions, spawnedIds),
        checkedAt: new Date().toISOString(),
      };
      // Capture the subtree *now*, while the panes are alive: once they are reaped their children are
      // reparented away and the link back to this run cannot be reconstructed. Merged across samples so
      // a descendant seen once is still checked at teardown. Best effort - a failed read yields nothing
      // and cannot weaken the verdict, which already fails closed on the panes alone.
      for (const entry of captureDescendants(paneState.panes.map((pane) => pane.panePid))) {
        if (!descendantState.some((known) => known.pid === entry.pid)) descendantState.push(entry);
      }

      const sample = {
        at: new Date(measuredAtMs).toISOString(),
        elapsedMs: measuredAtMs - samplingBeganMs,
        // The instant this sample was required at, its lateness, and what the tick spent its time on.
        // A miss is only evidence about the sampler once these are separable: `sampleLagMs` is the
        // deadline miss, `sampleCostMs` the identity+footprint reads, `tickCostMs` the work after
        // the measurement.
        targetElapsedMs: instant,
        sampleLagMs: sampleBeganMs - (samplingBeganMs + instant),
        sampleCostMs: measureCostMs,
        tickCostMs: Date.now() - measuredAtMs,
        index: tick,
        daemonPid,
        keeperPid: keeperSpawned,
        identityVerified: identitiesOk,
        routingVerified: sampledRouting.reconciliation.ok,
        keeperOwned: sampledRouting.reconciliation.keeperOwned,
        daemonOwned: sampledRouting.reconciliation.daemonOwned,
        liveLocal: sampledRouting.inventory?.liveLocal ?? null,
        panesAccounted: paneState.accounted,
        panesUnaccounted: paneState.unaccounted.length,
        writeFailures,
        measurement: measuredOnFootprint ? 'phys_footprint' : 'rss',
        daemonRssBytes: nowIdentityDaemon?.rssBytes ?? null,
        keeperRssBytes: nowIdentityKeeper?.rssBytes ?? null,
        daemonFootprintBytes,
        keeperFootprintBytes,
        totalBytes: measuredOnFootprint
          ? daemonFootprintBytes + keeperFootprintBytes
          : (nowIdentityDaemon?.rssBytes ?? 0) + (nowIdentityKeeper?.rssBytes ?? 0),
      };
      samples.push(sample);
      appendFileSync(samplesPath, `${JSON.stringify(sample)}\n`);

      if (!identitiesOk) {
        exitReason = 'process-identity-lost';
        return finish('unverified', 3, exitReason, {
          reasons: [
            `sample at t=${Math.round(sample.elapsedMs / 1000)} s could not re-verify the daemon or keeper identity ` +
              `recorded at spawn (pid reuse or replacement)`,
          ],
          samplesPath,
        });
      }
      if (!sampledRouting.reconciliation.ok) {
        exitReason = 'routing-lost';
        return finish('unverified', 4, exitReason, {
          reasons: sampledRouting.reconciliation.reasons,
          samplesPath,
        });
      }
      const evaluation = evaluateSoak({ samples, options, fullSpec: isFullSpec(options) });
      if (evaluation.verdict === 'breach') {
        exitReason = evaluation.breaches[0]?.kind === 'target-exceeded' ? 'target-breached' : 'upward-trend';
        return finish('breach', 1, exitReason, {
          reasons: evaluation.breaches.map((breach) => `${breach.kind} at ${breach.at}: ${breach.totalBytes} bytes`),
          breaches: evaluation.breaches,
          samplesPath,
        });
      }
      if (Date.now() - samplingBeganMs >= durationMs) break;
    }
  } catch (error) {
    if (control) control.close();
    if (data) data.close();
    if (keeperConnection) keeperConnection.close();
    if (error instanceof ProcessGoneError) {
      return finish('unverified', 3, 'process-exited', { reasons: [error.message], samplesPath });
    }
    if (error instanceof UnverifiedError) {
      return finish('unverified', 4, 'unverified', { reasons: [error.message], samplesPath });
    }
    // Everything else - and this is the common case, not the exotic one: connectJsonLines and
    // connectKeeper reject with plain Error ('connection to ... closed', 'no reply ... within N ms'),
    // so a daemon or keeper that dies mid-soak arrives here. Rethrowing used to skip teardown and
    // leave both children running with no summary. It now takes the same bounded cleanup as any other
    // failure: fail closed as unverified, stop only recorded identities, retain, and report.
    return finish('unverified', 3, 'sample-error', {
      reasons: [`sampling stopped: ${String(error?.message ?? error)}`],
      samplesPath,
    });
  }

  if (control) control.close();
  if (data) data.close();
  if (keeperConnection) keeperConnection.close();

  // Before the verdict, the premise it rests on: did the panes produce the volume the arithmetic
  // assumes? Checked here as well as in the loop's own accounting so a run that never wrote cannot
  // reach `evaluateSoak` with an empty, and therefore perfectly flat, ring.
  const shortfall = workloadShortfall({
    sessions: options.sessions,
    bytesPerTick: options.bytesPerTick,
    ticksWritten,
    producedBytes,
  });
  if (shortfall) {
    return finish('unverified', 4, 'workload-shortfall', { reasons: [shortfall], samplesPath });
  }

  const evaluation = evaluateSoak({ samples, options, fullSpec: isFullSpec(options) });
  return finish(evaluation.verdict, evaluation.exitCode, exitReason, {
    reasons: [...evaluation.unverified, ...evaluation.breaches.map((breach) => `${breach.kind} at ${breach.at}`)],
    unverified: evaluation.unverified,
    breaches: evaluation.breaches,
    trendSamples: evaluation.trendSamples,
    reference: evaluation.reference
      ? { at: evaluation.reference.at, totalBytes: evaluation.reference.totalBytes }
      : null,
    routing: routing ? { sessions: spawnedIds.length, keeperOwned: routing.reconciliation.keeperOwned } : null,
    handshake: dataHandshake ? { pid: dataHandshake.pid, version: dataHandshake.version } : null,
    samplesPath,
  });
}

/** The three views of "who owns what", read together so they can be reconciled atomically. */
async function readRouting({ control, data, keeperConnection, spawnedIds, sessions }) {
  const inventory = await controlInventory(control);
  const described = await data.call({ type: 'describeLiveSessions' });
  if (described?.type !== 'describeLiveSessionsOk') {
    throw new UnverifiedError('DAEMON_ROUTING_UNREADABLE', `describeLiveSessions: ${JSON.stringify(described)}`);
  }
  const keeperSessions = await keeperOwnership(keeperConnection);
  return {
    inventory,
    liveSessions: described.sessions ?? [],
    keeperSessions,
    reconciliation: reconcileRouting({
      expectedIds: spawnedIds,
      liveSessions: described.sessions ?? [],
      keeperSessions,
      inventory,
      sessions,
    }),
  };
}

const invokedDirectly =
  process.argv[1] !== undefined && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url));

if (invokedDirectly) {
  main().then(
    (code) => process.exit(code),
    (error) => {
      if (error instanceof GuardError) {
        console.error(`GUARD ${error.message}`);
        process.exit(2);
      }
      console.error(error);
      process.exit(2);
    },
  );
}

export { main };
