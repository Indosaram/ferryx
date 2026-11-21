#!/usr/bin/env bun
// Phase 0 acceptance harness: a forced SIGSEGV of the RELEASE binary must symbolicate to a named
// function through the archived dSYM, and a mismatched dSYM must be refused and name nothing.
//
//   bun scripts/qa/dsym-symbolication-acceptance.mjs \
//     --binary ~/ferryx-releases/<run>/bundle/macos/Ferryx.app/Contents/MacOS/ferryx \
//     --dsym  ~/ferryx-releases/<run>/dsyms \
//     --artifact ~/ferryx-releases/<run>/artifacts/darwin/Ferryx.app.tar.gz \
//     [--out <evidence dir>] [--ips <existing .ips>] [--json] [--dry-run]
//
// `--ips` exists for triage only: a report handed in after the fact can never produce a PASS,
// because the forced-crash check requires a pid THIS run spawned and signalled.
//
// Why a real crash and not a fixture: the plan's acceptance is "symbolicate the release binary's
// forced SIGSEGV and get a named function". A unit test can prove the gate's UUID logic; only a
// crash the kernel wrote, run through Apple's own `symbolicatecrash`, proves the archive names a
// frame. So this drives the real path end to end and records what it observed.
//
// Isolation: the crashed process is one THIS script spawned, under a private runtime root
// (`FERRYX_RUNTIME_DIR`/`FERRYX_DATA_DIR`/`FERRYX_SESSION_DIR` plus HOME/TMPDIR), so the user's own
// daemon, keeper and sessions are never signalled. Crash reports are written by the OS to
// `~/Library/Logs/DiagnosticReports` and cannot be redirected; the harness creates that directory if
// the host has never crashed anything, snapshots it before the crash and claims only the new report
// whose bytes name the target UUID.
//
// This script never builds anything: it needs an already-built release binary and its dSYM archive.
//
// Exit codes:
//   0 accepted (both the positive and the negative check passed)
//   1 rejected (the binary was read but a check failed: the release cannot symbolicate)
//   2 usage or unsafe-target error
//   3 no verdict (the host could not produce the evidence: no ReportCrash, no symbolicatecrash)

import { spawn, spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  watch,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";

import { DsymGateError, assertDsymGate, normalizeUuid } from "../lib/release-dsym.mjs";
import {
  APP_BINARY_MEMBER,
  SYMBOLICATION_SCHEMA,
  buildBinaryIdentity,
  buildForcedCrashRecord,
  evaluateBinaryIdentity,
  evaluateMismatchedDsym,
  evaluateForcedCrashProvenance,
  evaluatePackagedBinaryIdentity,
  evaluateSymbolication,
  machOUuids,
  readTarGzMember,
  renderSymbolicationSummary,
  resolveSymbolicationBundles,
  sha256Hex,
} from "../lib/dsym-symbolication.mjs";

export const DAEMON_READY_TOKEN = "FERRYX_DAEMON_READY";
/** How long a freshly launched daemon may take to announce readiness. */
export const READY_DEADLINE_MS = 60_000;
/** How long the crashed process may take to leave a report before the host is called unable. */
export const CRASH_REPORT_DEADLINE_MS = 60_000;
/** Upper bound on waiting for the crashed process to exit after the signal. */
export const EXIT_DEADLINE_MS = 30_000;
/**
 * How long the first SIGSEGV is given to end the process before ONE more is delivered.
 *
 * Rust's std swallows a `kill(2)`-delivered SIGSEGV (see `forceSegv`), so on a normal host the
 * first signal alone is not enough. The grace is short on purpose: a daemon that is going to die of
 * the signal dies at once, and the acceptance would otherwise wait out `EXIT_DEADLINE_MS`.
 */
export const SEGV_RETRY_DELAY_MS = 2_000;
/**
 * Temp root prefix. Deliberately NOT `ferryx-*`: production runtimes live at `/tmp/ferryx-<uid>`,
 * and a QA root that reads like one invites an operator (or a later check) to confuse the two.
 */
export const QA_ROOT_PREFIX = "ferryx-qa-dsym-";

/** A refusal the operator must act on; `code` leads the message and `exit` is the process code. */
export class HarnessError extends Error {
  constructor(code, message, detail = {}, exit = 2) {
    super(`${code}: ${message}`);
    this.name = "HarnessError";
    this.code = code;
    this.detail = detail;
    this.exit = exit;
  }
}

/** The host cannot produce the evidence (missing reporter or symbolication tool); not a verdict. */
export class EnvironmentGap extends HarnessError {
  constructor(code, message, detail = {}) {
    super(code, message, detail, 3);
    this.name = "EnvironmentGap";
  }
}

function usage() {
  return (
    "usage: bun scripts/qa/dsym-symbolication-acceptance.mjs --binary <release binary> " +
      "--dsym <archive dir> --artifact <Ferryx.app.tar.gz> [--out <dir>] [--ips <report.ips>] [--json] [--dry-run]"
  );
}

/** Parses argv. Every flag takes a value except the three switches. */
export function parseArgs(argv) {
  const args = { binary: null, dsym: null, artifact: null, out: null, ips: null, json: false, dryRun: false };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    switch (arg) {
      case "--json":
        args.json = true;
        break;
      case "--dry-run":
        args.dryRun = true;
        break;
      case "--binary":
      case "--dsym":
      case "--artifact":
      case "--out":
      case "--ips": {
        const value = argv[index + 1];
        if (!value || value.startsWith("--")) throw new HarnessError("USAGE", `${arg} requires a value\n${usage()}`);
        args[arg.slice(2)] = value;
        index += 1;
        break;
      }
      default:
        throw new HarnessError("USAGE", `unknown argument \`${arg}\`\n${usage()}`);
    }
  }
  if (!args.binary) throw new HarnessError("USAGE", `--binary is required\n${usage()}`);
  if (!args.dsym) throw new HarnessError("USAGE", `--dsym is required\n${usage()}`);
  if (!args.artifact) throw new HarnessError("USAGE", `--artifact is required\n${usage()}`);
  return args;
}

/**
 * Refuses to run against the user's live installation.
 *
 * A crash of the wrong binary proves nothing, and signalling an installed daemon would destroy real
 * sessions, so pointing the harness at `/Applications/Ferryx.app` is a structural refusal rather
 * than a warning. The runtime environment is NOT checked here: the harness always constructs its
 * own (`isolatedEnv`), so an inherited production `FERRYX_RUNTIME_DIR` in the operator's shell can
 * never reach the crashed process -- refusing on it would reject a normal Ferryx terminal.
 */
export function assertSafeTarget({ binary }) {
  const real = path.resolve(binary);
  // The installed-app refusal comes before the existence check: on a host where Ferryx IS installed,
  // that path exists and would otherwise be accepted and crashed.
  if (real.startsWith("/Applications/")) {
    throw new HarnessError(
      "BINARY_IS_INSTALLED_APP",
      `${real} is the installed application; point --binary at the release artifact's copy, not the live install`,
    );
  }
  if (!existsSync(binary)) throw new HarnessError("BINARY_MISSING", `no binary at ${binary}`);
  return real;
}

/**
 * Refuses a hand-supplied crash report for a real (non-dry-run) acceptance.
 *
 * The release gate's provenance check requires a pid THIS harness spawned and signalled, so an
 * `--ips` run can never produce evidence the gate accepts. Failing here states that once, instead
 * of emitting a FAIL whose cause each operator would have to re-derive.
 */
export function assertReportSource({ ips, binary, dryRun = false }) {
  if (ips && !dryRun) {
    throw new HarnessError(
      "IPS_NOT_ACCEPTABLE",
      `--ips analyses a report this harness did not force; the release gate requires a forced SIGSEGV of ${binary}. ` +
        "Drop --ips to run the forced-crash acceptance",
      { ips },
      1,
    );
  }
  return ips ?? null;
}

/** The private environment the forced-crash daemon runs under. */
export function isolatedEnv(root, base = process.env) {
  const runtime = path.join(root, "runtime");
  const data = path.join(root, "data");
  const sessions = path.join(root, "sessions");
  const home = path.join(root, "home");
  const tmp = path.join(root, "tmp");
  for (const dir of [runtime, data, sessions, home, tmp]) mkdirSync(dir, { recursive: true });
  const env = {
    ...base,
    HOME: home,
    TMPDIR: tmp,
    FERRYX_RUNTIME_DIR: runtime,
    FERRYX_LEGACY_RUNTIME_DIR: path.join(root, "legacy-runtime"),
    FERRYX_DATA_DIR: data,
    FERRYX_SESSION_DIR: sessions,
  };
  // `FERRYX_DAEMON_SUPERVISOR_NAME` is deliberately NOT set, and any inherited value is removed:
  // that override is what lifts the isolation policy and lets the daemon install a real OS job, so
  // leaving it in place would let this crash lane write a LaunchAgent.
  delete env.FERRYX_DAEMON_SUPERVISOR_NAME;
  return env;
}

/**
 * Where `symbolicatecrash` can live under one Xcode, in the order it should be tried.
 *
 * `developerDir` is `<Xcode.app>/Contents/Developer`, so the frameworks that ship the tool are its
 * sibling `../SharedFrameworks`; the older DTDeviceKitBase location is still tried first because an
 * operator may run a host whose Xcode predates the move. Measured on current Xcode: the tool is at
 * `<Xcode.app>/Contents/SharedFrameworks/DVTFoundation.framework/Versions/A/Resources/symbolicatecrash`
 * (with an `AssetRuntime/DVTFoundation.framework` copy alongside it), which the DTDeviceKitBase path
 * alone never finds.
 */
function symbolicatecrashCandidates(developerDir) {
  if (!developerDir) return [];
  return [
    path.join(
      developerDir,
      "Platforms/MacOSX.platform/Developer/Library/PrivateFrameworks/DTDeviceKitBase.framework/Versions/A/Resources/symbolicatecrash",
    ),
    path.join(developerDir, "..", "SharedFrameworks", "DVTFoundation.framework", "Versions", "A", "Resources", "symbolicatecrash"),
    path.join(
      developerDir,
      "..",
      "SharedFrameworks",
      "AssetRuntime",
      "DVTFoundation.framework",
      "Versions",
      "A",
      "Resources",
      "symbolicatecrash",
    ),
  ];
}

/**
 * Locates Apple's `symbolicatecrash`.
 *
 * `xcrun --find` is authoritative when a full Xcode is installed; the framework paths are the
 * fallback because the tool ships inside Xcode's frameworks rather than on PATH. A Command Line
 * Tools-only install has no symbolicatecrash at all, which is a host gap, not a release defect.
 * `exists` is injected so the candidate order can be pinned by a test without that Xcode on disk.
 */
export function findSymbolicatecrash(options = {}) {
  return resolveSymbolicatecrash(options).tool;
}

/**
 * The active Xcode: the inherited `DEVELOPER_DIR`, else `xcode-select -p`.
 *
 * `symbolicatecrash` itself refuses to run without it (`Error: "DEVELOPER_DIR" is not defined`),
 * so the value is needed at RUN time as well as for discovery: a host with Xcode selected but no
 * exported `DEVELOPER_DIR` finds the tool and then fails to execute it.
 */
export function activeDeveloperDir({ env = process.env, run = defaultRun } = {}) {
  if (env.DEVELOPER_DIR) return env.DEVELOPER_DIR;
  const selected = run("xcode-select", ["-p"]);
  const value = String(selected.stdout ?? "").trim();
  return value || null;
}

/**
 * The tool path AND the developer dir that tool needs in its environment.
 *
 * Both come out of one call because they must agree: the framework candidates are derived from the
 * developer dir, and that same dir is the `DEVELOPER_DIR` the located script refuses to start
 * without. `findSymbolicatecrash` stays as the path-only view for callers that need nothing else.
 */
export function resolveSymbolicatecrash({ env = process.env, run = defaultRun, exists = existsSync } = {}) {
  const developerDir = activeDeveloperDir({ env, run });
  const tool = locateSymbolicatecrash({ env, developerDir, run, exists });
  // Resolved AFTER the text tool, so the candidate order an existing host depends on is untouched:
  // `CrashSymbolicator.py` is an additional probe, never one that displaces a symbolicatecrash path.
  return { tool, developerDir, crashSymbolicator: resolveCrashSymbolicator({ env, developerDir, exists }) };
}

/** The `symbolicatecrash` half of {@link resolveSymbolicatecrash}: env override, xcrun, frameworks. */
function locateSymbolicatecrash({ env, developerDir, run, exists }) {
  if (env.SYMBOLICATECRASH) return env.SYMBOLICATECRASH;
  const found = run("xcrun", ["--find", "symbolicatecrash"]);
  if (found.status === 0 && found.stdout.trim()) return found.stdout.trim();
  for (const candidate of symbolicatecrashCandidates(developerDir)) {
    if (exists(candidate)) return candidate;
  }
  return null;
}

/** The tool name recorded in the evidence, so a report states which symbolicator produced frames. */
export const SYMBOLICATECRASH_TOOL = "symbolicatecrash";
/** Apple's JSON crash-log symbolicator: the one that can read a modern `.ips`. */
export const CRASH_SYMBOLICATOR_TOOL = "CrashSymbolicator.py";

/**
 * Where `CrashSymbolicator.py` sits inside one Xcode, relative to `<Xcode.app>/Contents/Developer`.
 *
 * `symbolicatecrash` is a Perl script that only understands the legacy `.crash` text form; given a
 * modern `.ips` it exits 255 with `No crash report version in <path>`. Xcode ships the replacement
 * in CoreSymbolicationDT, reachable only by this framework path (never on PATH).
 */
export const CRASH_SYMBOLICATOR_RELATIVE_PATH =
  "SharedFrameworks/CoreSymbolicationDT.framework/Versions/A/Resources/CrashSymbolicator.py";

/** `python3` from the OS: the tool imports Xcode's own modules, so an interpreter of ours is wrong. */
export const CRASH_SYMBOLICATOR_PYTHON = "/usr/bin/python3";

/**
 * Locates `CrashSymbolicator.py` under the developer dir that already located `symbolicatecrash`.
 *
 * `CRASH_SYMBOLICATOR` overrides it outright; `null` means this host cannot read a JSON report and
 * only the legacy text path is available.
 */
export function resolveCrashSymbolicator({ env = process.env, developerDir = null, exists = existsSync } = {}) {
  if (env.CRASH_SYMBOLICATOR) return env.CRASH_SYMBOLICATOR;
  if (!developerDir) return null;
  const candidate = path.join(developerDir, "..", CRASH_SYMBOLICATOR_RELATIVE_PATH);
  return exists(candidate) ? candidate : null;
}

function defaultRun(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", ...options });
  return { status: result.status ?? -1, stdout: result.stdout ?? "", stderr: result.stderr ?? "", signal: result.signal ?? null };
}

/** ReportCrash writes into `dir`, so it must exist before it is snapshotted or watched. */
function ensureReportsDir(dir) {
  mkdirSync(dir, { recursive: true });
}

/** Reads one crash-report candidate; injectable so the rename race can be pinned by a test. */
function defaultReadCrashReport(file) {
  return readFileSync(file, "utf8");
}

/**
 * `.ips` files in `dir`, oldest first.
 *
 * ReportCrash writes a hidden `.<name>.ips` and then renames it into place, so a dot-name is a
 * temporary that may be gone by the time anything reads it: it is never a candidate. A file that
 * vanishes mid-listing is the same race, and equally skipped rather than thrown on.
 */
export function listCrashReports(dir) {
  if (!existsSync(dir)) return [];
  const reports = [];
  for (const name of readdirSync(dir)) {
    if (name.startsWith(".") || !name.endsWith(".ips")) continue;
    const file = path.join(dir, name);
    let mtimeMs;
    try {
      mtimeMs = statSync(file).mtimeMs;
    } catch {
      continue;
    }
    reports.push({ file, mtimeMs });
  }
  return reports.sort((a, b) => a.mtimeMs - b.mtimeMs).map((entry) => entry.file);
}

/** True when `text` names any of `uuids`, in either dashed or undashed form. */
export function reportNamesUuid(text, uuids) {
  const upper = String(text ?? "").toUpperCase();
  return uuids.some((uuid) => upper.includes(uuid) || upper.includes(uuid.replace(/-/g, "")));
}

/**
 * Waits for a crash report that did not exist before the crash and names one of `uuids`.
 *
 * A crash report is written by ReportCrash, not by the crashed process, so it lands after the
 * process exits. The wait is event-driven (`fs.watch`) with a bounded deadline, never a poll loop.
 *
 * The directory is created first: `fs.watch` on a missing path throws ENOENT, and a host that has
 * never crashed anything has no `DiagnosticReports` at all.
 */
export function awaitNewCrashReport({
  dir,
  before,
  uuids,
  deadlineAt,
  watchImpl = watch,
  now = () => Date.now(),
  readText = defaultReadCrashReport,
  ensureDir = ensureReportsDir,
}) {
  ensureDir(dir);
  const known = new Set(before);
  const matches = () => {
    if (!existsSync(dir)) return null;
    for (const file of listCrashReports(dir)) {
      if (known.has(file)) continue;
      let text = "";
      try {
        text = readText(file);
      } catch {
        // A candidate that vanished between the listing and the read is the rename race, not a
        // failure: skip it and keep waiting for the file that was renamed into place.
        continue;
      }
      if (uuids.length === 0 || reportNamesUuid(text, uuids)) return file;
    }
    return null;
  };
  return new Promise((resolve) => {
    const immediate = matches();
    if (immediate) return resolve(immediate);
    if (now() >= deadlineAt) return resolve(null);
    let done = false;
    const settle = (value) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      try {
        watcher.close();
      } catch {
        // Already closed; nothing to release.
      }
      resolve(value);
    };
    // A watcher event that matches nothing is not the deadline: the report may still be mid-write,
    // so keep waiting for the next event or the deadline timer instead of resolving null early.
    const evaluate = () => {
      const found = matches();
      if (found) return settle(found);
      if (now() >= deadlineAt) return settle(null);
    };
    const timer = setTimeout(() => settle(matches()), Math.max(0, deadlineAt - now()));
    const watcher = watchImpl(dir, { persistent: false }, evaluate);
    watcher.on?.("error", evaluate);
  });
}

/**
 * Spawns the release binary as a headless daemon in `env`, forces SIGSEGV once it is ready, and
 * resolves once the process has exited. The pid is recorded from the spawn, never discovered.
 *
 * Why a second SIGSEGV exists at all: the Rust runtime installs a SIGSEGV/SIGBUS handler to detect
 * stack overflow. When the faulting address is not a guard page, that handler restores the default
 * disposition (SIG_DFL) and RETURNS, relying on the faulting instruction to re-fault as soon as the
 * handler returns. A `kill(2)`-delivered SIGSEGV has no faulting instruction behind it, so the
 * handler returns into ordinary code and the daemon keeps running: measured on macOS, the first
 * signal was swallowed and the process only left when the exit deadline SIGKILLed it. The restored
 * SIG_DFL does take effect, so exactly ONE more SIGSEGV to the same pid terminates it as a genuine
 * SIGSEGV crash -- which is what the acceptance and its crash report require.
 */
export function forceSegv({
  binary,
  env,
  readyDeadlineMs = READY_DEADLINE_MS,
  exitDeadlineMs = EXIT_DEADLINE_MS,
  segvRetryMs = SEGV_RETRY_DELAY_MS,
  spawnImpl = spawn,
}) {
  return new Promise((resolve, reject) => {
    const child = spawnImpl(binary, ["--daemon"], { env, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    let signalled = false;
    let readyAt = null;
    let readyTimer = null;
    let hasExited = false;
    let retryTimer = null;
    // Every SIGSEGV this run delivered to the child, in order. Recorded so the evidence shows
    // whether the crash needed the re-delivery and when each signal was sent, instead of leaving an
    // operator to infer it from a process that took two seconds to die.
    const signalDeliveries = [];
    // Assigned once the exit watcher exists; a readiness failure must cancel it, or a caller that
    // never sees an exit keeps a live timer and the process alive with it.
    let clearExitTimer = () => {};
    // Only ever `child.kill` on the ChildProcess object: that signals the pid this run spawned and
    // nothing else, so no other process can be hit by the retry either.
    const deliverSegv = (attempt) => {
      // Recorded BEFORE the call, so a listener that reacts synchronously cannot snapshot the list
      // without the delivery it is reacting to. A throwing kill keeps the entry and names the error.
      const delivery = { signal: "SIGSEGV", attempt, at: new Date().toISOString(), pid: child.pid };
      signalDeliveries.push(delivery);
      try {
        child.kill("SIGSEGV");
      } catch (error) {
        delivery.error = String(error?.message ?? error);
        throw error;
      }
      return delivery;
    };
    const clearRetry = () => {
      if (retryTimer) clearTimeout(retryTimer);
      retryTimer = null;
    };
    /** Arms the one-shot re-delivery described on this function. */
    const armSegvRetry = () => {
      if (hasExited) return;
      retryTimer = setTimeout(() => {
        retryTimer = null;
        if (hasExited) return;
        try {
          deliverSegv(2);
        } catch {
          // The process is gone or unsignalable: the attempt and its error are already recorded, and
          // the exit watcher reports what actually happened, so there is no second error path.
        }
      }, segvRetryMs);
    };
    const ready = new Promise((resolveReady, rejectReady) => {
      const settleReady = (error) => {
        clearTimeout(readyTimer);
        if (error) rejectReady(error);
        else resolveReady();
      };
      const onData = (chunk) => {
        const text = chunk.toString();
        stdout += text;
        if (!signalled && stdout.includes(DAEMON_READY_TOKEN)) {
          signalled = true;
          readyAt = new Date().toISOString();
          try {
            deliverSegv(1);
          } catch (error) {
            settleReady(new HarnessError("SIGNAL_FAILED", `could not signal pid ${child.pid}: ${error.message}`, {}, 1));
            return;
          }
          armSegvRetry();
          settleReady(null);
        }
      };
      child.stdout.on("data", onData);
      child.stderr.on("data", (chunk) => {
        stderr += chunk.toString();
      });
      child.once("error", (error) => settleReady(new EnvironmentGap("DAEMON_SPAWN_FAILED", String(error.message ?? error))));
      // A daemon that dies during boot (missing dylib, refused socket) is the likeliest real
      // failure here. Without this the harness would sit on the readiness timer for its whole
      // window before reporting something the exit already proved.
      child.once("exit", (code, signal) => {
        if (signalled) return;
        settleReady(
          new EnvironmentGap(
            "DAEMON_EXITED_BEFORE_READY",
            `the daemon left with ${signal ?? `code ${code}`} before ${DAEMON_READY_TOKEN}`,
            { stderr: stderr.slice(-2000) },
          ),
        );
      });
      readyTimer = setTimeout(
        () => settleReady(new EnvironmentGap("DAEMON_NOT_READY", `no ${DAEMON_READY_TOKEN} in ${readyDeadlineMs}ms`, { stderr: stderr.slice(-2000) })),
        readyDeadlineMs,
      );
    });
    const exited = new Promise((resolveExit) => {
      const timer = setTimeout(() => {
        try {
          child.kill("SIGKILL");
        } catch {
          // Already gone.
        }
      }, exitDeadlineMs);
      clearExitTimer = () => clearTimeout(timer);
      child.once("exit", (code, signal) => {
        hasExited = true;
        clearRetry();
        clearTimeout(timer);
        // `spawnedBinary` and `readyToken` are recorded from THIS spawn, so the evidence names the
        // process the harness actually started rather than a path the operator typed in later.
        resolveExit({
          pid: child.pid,
          code,
          signal,
          stdout,
          stderr,
          spawnedBinary: binary,
          readyToken: signalled ? DAEMON_READY_TOKEN : null,
          readyAt,
          signalDeliveries: signalDeliveries.map((delivery) => ({ ...delivery })),
          signalCount: signalDeliveries.length,
        });
      });
    });
    ready.then(
      () =>
        exited.then((outcome) => {
          // The acceptance is about a SIGSEGV because that is the failure the plan exists for; any
          // other ending leaves no crash report and must not be mistaken for one.
          if (outcome.signal !== "SIGSEGV") {
            reject(
              new HarnessError(
                "UNEXPECTED_EXIT",
                `the daemon left with ${outcome.signal ?? `code ${outcome.code}`} instead of SIGSEGV`,
                { pid: outcome.pid, stderr: outcome.stderr.slice(-2000) },
                1,
              ),
            );
            return;
          }
          resolve(outcome);
        }),
      (error) => {
        try {
          child.kill("SIGKILL");
        } catch {
          // Already gone.
        }
        clearRetry();
        clearExitTimer();
        reject(error);
      },
    );
  });
}

/**
 * Runs the release's own collector so the bundle on disk is exactly what a reporter would get.
 *
 * `env` is the isolated environment the crash ran under and is REQUIRED for a forced crash: the
 * collector reads `<data>/logs/daemon.log{,1..3}` and `<data>/daemon.log` from the environment it
 * runs in, so without it the collected log tail belongs to the operator's live install instead of
 * the daemon that crashed. Left null only for an operator-supplied `--ips`, where the crash being
 * analysed was captured by that same install.
 */
export function collectBundle({ binary, ips, dsym, out, env = null, run = defaultRun }) {
  const options = env ? { env } : {};
  const result = run(binary, ["daemon", "crash-report", "--ips", ips, "--dsym", dsym, "--out", out, "--json"], options);
  if (result.status !== 0) {
    throw new HarnessError(
      "CRASH_REPORT_FAILED",
      `${binary} daemon crash-report exited ${result.status}: ${(result.stderr || result.stdout).trim()}`,
      {},
      1,
    );
  }
  try {
    return JSON.parse(result.stdout);
  } catch {
    throw new HarnessError("CRASH_REPORT_UNREADABLE", `crash-report printed no JSON: ${result.stdout.trim().slice(0, 400)}`, {}, 1);
  }
}

/**
 * The environment `symbolicatecrash` runs under: the caller's own, with `DEVELOPER_DIR` set.
 *
 * Apple's script sources no toolchain path of its own and dies at its first line of real work when
 * `DEVELOPER_DIR` is missing, which is what made a host with Xcode selected but nothing exported
 * report a symbolication failure instead of symbolicated frames. Everything else (`PATH`, `HOME`)
 * is kept, because the script shells out to `xcrun`, `atos` and `dwarfdump`.
 */
export function symbolicationEnv({ developerDir = null, base = process.env } = {}) {
  if (!developerDir) return base;
  return { ...base, DEVELOPER_DIR: developerDir };
}

/**
 * The payload of a crash report: the JSON body after an optional one-line `.ips` header.
 *
 * Tried as a whole document first, then after the first line, because CrashSymbolicator's output is
 * a bare body that still contains newlines (`json.dumps(indent=0)`), and an `.ips` is header plus
 * body. `null` when neither reads as JSON, which is what a legacy text report does.
 */
function parseCrashBody(text) {
  const source = String(text ?? "");
  try {
    return JSON.parse(source);
  } catch {
    // Not one document; the header/body split below is the other real shape.
  }
  const newline = source.indexOf("\n");
  if (newline === -1) return null;
  try {
    return JSON.parse(source.slice(newline + 1));
  } catch {
    return null;
  }
}

/**
 * Whether `text` is a modern `.ips`: a one-line JSON header followed by a JSON body.
 *
 * On a current macOS every ReportCrash report is this shape, and `symbolicatecrash` cannot parse
 * it at all (`No crash report version in <path>`, exit 255). Both proofs are accepted: a header
 * object carrying `bug_type` (ReportCrash's own marker), or a body carrying `threads`/`usedImages`
 * (`--no-ips-header` output, and reports written by tools that omit the header).
 */
export function isJsonCrashReport(text) {
  const source = String(text ?? "");
  // The `.ips` header is the first line, so it is probed on its own first: a header that parses is
  // ReportCrash's own marker, whatever the body turns out to be.
  const newline = source.indexOf("\n");
  if (newline !== -1) {
    try {
      const header = JSON.parse(source.slice(0, newline));
      if (header && typeof header === "object" && (header.bug_type !== undefined || header.app_name !== undefined)) return true;
    } catch {
      // Not a JSON header; the document probes below decide.
    }
  }
  // Then the document itself, as a whole and (for a header plus body) after the first line: a body
  // written by `json.dumps(indent=0)` spans many lines but has no header at all.
  const document = parseCrashBody(source);
  return Boolean(document && typeof document === "object" && (document.threads !== undefined || document.usedImages !== undefined));
}

/** A bare address or displacement: what an unnamed frame carries instead of a symbol. */
function isOffsetOnlySymbol(symbol) {
  // `<deduplicated_symbol>` is ReportCrash's placeholder for a name it withheld, not a function.
  return /^0x[0-9a-fA-F]+$/.test(symbol) || /^\+?\s*\d+$/.test(symbol) || /^<.*>$/.test(symbol);
}

/** The image label the verdict functions select on, for a frame of the binary under acceptance. */
export const BINARY_FRAME_LABEL = "ferryx";

/**
 * Where `CrashSymbolicator.py` is told to write its output, inside the evidence directory.
 *
 * The path is RETURNED and kept (never a temp dir that is deleted) because the report records the
 * tool's exact argv: a recorded `-o /tmp/...` would name a file an operator can no longer read, so
 * the replay could not be compared with what this run judged. `kind` separates the two runs, whose
 * outputs are different and both worth keeping.
 */
export function symbolicatedJsonDir(reportDir, kind) {
  const dir = path.join(reportDir, `symbolicated-${kind}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

/**
 * Frames of one JSON crash report, rendered as `symbolicatecrash`'s own column form.
 *
 * The report is read through the SAME classifiers the text path uses (`parseSymbolicatedFrames`,
 * `classifyBody`) rather than a second, parallel rule: rendering each `threads[].frames[]` entry as
 * `<index>  <image>  0x<address>  <body>` means "named" and "offset-only" keep one definition across
 * both tools, and the rendered text is useful evidence on its own.
 *
 * `image` is the frame's `usedImages[imageIndex]` name (else the last path component). When that
 * image's UUID is one of `binaryUuids` the label is `ferryx`, because the UUID -- not the string
 * ReportCrash wrote -- is what identifies the binary under test, and `binaryFrames` selects on
 * exactly that label.
 */
export function renderJsonCrashFrames({ text, binaryUuids = [] } = {}) {
  const document = parseCrashBody(text);
  if (!document || typeof document !== "object") return null;
  const wanted = new Set((binaryUuids ?? []).map((uuid) => normalizeUuid(uuid)).filter(Boolean));
  const images = Array.isArray(document.usedImages) ? document.usedImages : [];
  const labelFor = (imageIndex) => {
    const image = Number.isInteger(imageIndex) ? images[imageIndex] : null;
    if (!image || typeof image !== "object") return null;
    const uuid = normalizeUuid(image.uuid ?? "");
    // A UUID match is authoritative: the same dSYM is keyed to it, whatever the image is called.
    if (uuid && wanted.has(uuid)) return BINARY_FRAME_LABEL;
    const name = typeof image.name === "string" && image.name ? image.name : null;
    if (name) return name;
    const imagePath = typeof image.path === "string" ? image.path : "";
    return imagePath ? path.basename(imagePath) : null;
  };
  const lines = [];
  const threads = Array.isArray(document.threads) ? document.threads : [];
  for (const thread of threads) {
    const frames = Array.isArray(thread?.frames) ? thread.frames : [];
    for (const frame of frames) {
      if (!frame || typeof frame !== "object") continue;
      const imageIndex = Number.isInteger(frame.imageIndex) ? frame.imageIndex : null;
      // Whitespace inside an image name would split the column form the text tool's output is read
      // in, so it is folded here; the label only ever feeds the `ferryx` comparison.
      const label = (labelFor(imageIndex) ?? "unknown").replace(/\s+/g, "_");
      const offset = Number.isFinite(frame.imageOffset) ? frame.imageOffset : 0;
      const base = (() => {
        const image = imageIndex === null ? null : images[imageIndex];
        const value = image && Number.isFinite(image.base) ? image.base : null;
        return value === null ? null : value;
      })();
      const address = base === null ? `0x${offset.toString(16)}` : `0x${(base + offset).toString(16)}`;
      const symbol = typeof frame.symbol === "string" ? frame.symbol.trim() : "";
      const named = symbol !== "" && !isOffsetOnlySymbol(symbol);
      const location = Number.isFinite(frame.symbolLocation) ? frame.symbolLocation : 0;
      // A symbol renders as `<symbol> + <displacement>`; an unnamed frame keeps the offset pair so
      // `classifyBody` reads it as anonymous, exactly as the text tool's own output does.
      const frameBody = named ? `${symbol} + ${location}` : `0x${(base ?? 0).toString(16)} + ${offset}`;
      lines.push(`${Number.isInteger(frame.frameIndex) ? frame.frameIndex : lines.length}   ${label}   ${address}   ${frameBody}`);
    }
  }
  return { frames: lines, text: lines.join("\n") };
}

/** The per-frame fields a symbolicator fills in; removed before the tool judges the archive. */
export const SYMBOL_FRAME_FIELDS = ["symbol", "symbolLocation", "sourceFile", "sourceLine", "inline"];

/** Name of the de-symbolicated copy, inside the directory a run keeps its symbolication output in. */
export const DESYMBOLICATED_REPORT_NAME = "desymbolicated.ips";

/**
 * A copy of a JSON crash report with ReportCrash's own names removed from the binary's frames.
 *
 * Measured on a fully stripped release binary: `nm` showed a single `T` symbol, yet the RAW `.ips`
 * ReportCrash wrote already carried `symbol` on all 129 ferryx frames. ReportCrash/CoreSymbolication
 * names frames itself, from a cache keyed by image UUID -- and the UUID survives `strip`. So a
 * symbolicator handed the raw report keeps those names: `CrashSymbolicator.py` only fills what is
 * missing, and under a MISMATCHED dSYM its output still had 129 named frames, which made
 * `mismatchNamesNothing` permanently fail. A correct dSYM adds names to hundreds more frames
 * (inlines and source locations included), so the dSYM is what the acceptance must measure.
 *
 * Only frames of the binary under acceptance are stripped. A frame's image is
 * `usedImages[frame.imageIndex]`, and its identity is that image's UUID matched against
 * `binaryUuids` in canonical form (`normalizeUuid` absorbs case and dashes). A system image keeps
 * its symbol: naming libsystem proves nothing about the archive either way.
 *
 * The `.ips` header line is kept byte-for-byte and the body is re-emitted as JSON with every field
 * other than the stripped ones untouched. `null` when the text is not a JSON report, when no binary
 * UUID was supplied, or when no frame of the binary was found in it: in all three cases this cannot
 * say which image is under acceptance, so the caller must keep using the report as it is.
 *
 * `preNamedFrames` counts binary frames that already carried a name, which is the number of frames
 * a verdict would otherwise score without any help from the dSYM. It is published for transparency.
 */
export function desymbolicateReport({ text, binaryUuids = [] } = {}) {
  const source = String(text ?? "");
  if (!isJsonCrashReport(source)) return null;
  const wanted = new Set((binaryUuids ?? []).map((uuid) => normalizeUuid(uuid)).filter(Boolean));
  if (wanted.size === 0) return null;
  // Both real shapes are accepted, exactly as `parseCrashBody` reads them: a one-line header plus a
  // body, and a bare body (what `--no-ips-header` writes, and what a tool-only report carries).
  const newline = source.indexOf("\n");
  const shapes =
    newline === -1
      ? [{ header: null, body: source }]
      : [
          { header: null, body: source },
          { header: source.slice(0, newline), body: source.slice(newline + 1) },
        ];
  let chosen = null;
  for (const shape of shapes) {
    try {
      const document = JSON.parse(shape.body);
      if (document && typeof document === "object") {
        chosen = { ...shape, document };
        break;
      }
    } catch {
      // Not this shape; the other candidate is tried next.
    }
  }
  if (!chosen) return null;
  const document = chosen.document;
  const images = Array.isArray(document.usedImages) ? document.usedImages : [];
  let preNamedFrames = 0;
  let strippedFrames = 0;
  let binaryFrameCount = 0;
  for (const thread of Array.isArray(document.threads) ? document.threads : []) {
    const frames = Array.isArray(thread?.frames) ? thread.frames : [];
    for (const frame of frames) {
      if (!frame || typeof frame !== "object") continue;
      const image = Number.isInteger(frame.imageIndex) ? images[frame.imageIndex] : null;
      if (!image || typeof image !== "object") continue;
      const uuid = normalizeUuid(image.uuid ?? "");
      if (!uuid || !wanted.has(uuid)) continue;
      binaryFrameCount += 1;
      if (typeof frame.symbol === "string" && frame.symbol.trim() !== "") preNamedFrames += 1;
      let stripped = false;
      for (const field of SYMBOL_FRAME_FIELDS) {
        if (field in frame) {
          delete frame[field];
          stripped = true;
        }
      }
      if (stripped) strippedFrames += 1;
    }
  }
  // No frame of the binary was found, so this cannot say which image is under acceptance: the caller
  // keeps the report as it is rather than writing a copy that proves nothing about the binary.
  if (binaryFrameCount === 0) return null;
  const body = JSON.stringify(document);
  return {
    text: chosen.header === null ? body : `${chosen.header}\n${body}`,
    preNamedFrames,
    strippedFrames,
    binaryFrameCount,
  };
}

/**
 * Symbolicates one crash report, choosing the tool the report's own shape requires.
 *
 * A modern `.ips` is JSON: `symbolicatecrash` exits 255 on it and names nothing, so it is handed to
 * Xcode's `CrashSymbolicator.py`, which reads exactly that shape. A legacy text report is still run
 * through `symbolicatecrash`, unchanged. Both routes return the same `{status, stdout, stderr}`, so
 * the verdict functions are unaware of which tool produced the frames, and `symbolicator` states it.
 *
 * `dsymSearchDir` is what `CrashSymbolicator.py` is given instead of `dsym` when the caller has one:
 * its `-d` is a single-value flag whose directory is searched RECURSIVELY for `DWARF/<image name>`,
 * so one archive directory covers every matched bundle -- which a repeated `-d` could not do, because
 * argparse's `store` would keep only the last one. Its `uuid` check is what still refuses a
 * mismatched bundle, so widening the search cannot turn the negative run into a pass.
 *
 * Every dSYM handed to either tool is passed explicitly and recorded by the caller, so the evidence
 * names the bundles that produced the frames instead of the directory they happened to sit in.
 */
export function symbolicate({
  tool,
  crashSymbolicator = null,
  ips,
  dsym,
  dsymSearchDir = null,
  extra = [],
  env = process.env,
  binaryUuids = [],
  run = defaultRun,
  readText = (file) => readFileSync(file, "utf8"),
  // Where `CrashSymbolicator.py` writes its `-o` file. A seam, not necessarily a temp dir: the
  // runner points it inside the evidence directory so the recorded argv still resolves later.
  makeOutDir = () => mkdtempSync(path.join(os.tmpdir(), "ferryx-crash-symbolicator-")),
  removeDir = (dir) => rmSync(dir, { recursive: true, force: true }),
  // Where the de-symbolicated copy is written. A seam like `makeOutDir`: the runner points it at one
  // directory shared by the positive and the negative run, so both are handed the same bytes.
  desymbolicatedDir = null,
  writeText = (file, data) => writeFileSync(file, data),
}) {
  let reportText = null;
  try {
    reportText = readText(ips);
  } catch {
    reportText = null;
  }
  const jsonReport = reportText !== null && isJsonCrashReport(reportText);
  if (jsonReport && crashSymbolicator) {
    const outDir = makeOutDir();
    const outFile = path.join(outDir, "symbolicated.json");
    // ReportCrash names frames from its own UUID-keyed cache, so the RAW report can already carry a
    // symbol on every frame of the binary. Handing that report to the tool would score names this
    // run did not earn: under a mismatched dSYM the frames would stay named and the negative check
    // could never pass. The tool is therefore given a copy whose binary frames were de-symbolicated,
    // written into the run's output directory (kept, so a recorded argv still resolves) -- the raw
    // report is read only, never rewritten.
    const desymbolicated = desymbolicateReport({ text: reportText, binaryUuids });
    let symbolicationInput = ips;
    let desymbolicatedReport = null;
    if (desymbolicated) {
      const copyDir = desymbolicatedDir ?? outDir;
      mkdirSync(copyDir, { recursive: true });
      desymbolicatedReport = path.join(copyDir, DESYMBOLICATED_REPORT_NAME);
      writeText(desymbolicatedReport, desymbolicated.text);
      symbolicationInput = desymbolicatedReport;
    }
    // ONE `-d`. The flag is argparse's `store`, not `append`, so a second `-d` REPLACES the first
    // instead of adding to it -- repeating it for `extra` would silently drop the bundle under test.
    // A single dSYM is fine: the tool resolves it to its CONTAINING directory and searches that
    // recursively for `DWARF/<image name>`, which is where the archive's other bundles sit.
    const dsymTarget = dsymSearchDir ?? dsym;
    const args = [
      crashSymbolicator,
      "-d",
      dsymTarget,
      // `--no-ips-header` keeps the output pure JSON: with the header left in place the result is
      // the same two-document shape this can already read, but the header is reconstructed from a
      // report macOS already redacted, so it adds nothing an operator needs.
      "--no-ips-header",
      "-o",
      outFile,
      // The de-symbolicated copy when one could be written, else the report as it is (a legacy
      // text report, or a JSON one with no binary UUID to identify its frames by).
      symbolicationInput,
    ];
    try {
      const result = run(CRASH_SYMBOLICATOR_PYTHON, args, { env });
      let symbolicatedText = "";
      try {
        symbolicatedText = readText(outFile);
      } catch {
        // Nothing written: the tool's own stdout/stderr says why, and the caller records it.
        symbolicatedText = "";
      }
      const rendered = symbolicatedText ? renderJsonCrashFrames({ text: symbolicatedText, binaryUuids }) : null;
      return {
        symbolicator: CRASH_SYMBOLICATOR_TOOL,
        tool: crashSymbolicator,
        argv: [CRASH_SYMBOLICATOR_PYTHON, ...args],
        status: result?.status ?? -1,
        // The file the tool wrote, else what it printed instead: with `-o` it explains a report it
        // could not handle on stdout (`Crash log is missing bug_type field...`) while still exiting
        // 0, and that explanation is the only thing that distinguishes it from naming nothing.
        stdout: symbolicatedText || result?.stdout || "",
        stderr: result?.stderr ?? "",
        jsonReport: true,
        // The column-form rendering is what the verdict functions read, so both tools are judged by
        // the same named/offset rule.
        framesText: rendered ? rendered.text : "",
        // What the tool was actually handed, and how much of the report was already named before it
        // ran, so a reader can separate the dSYM's contribution from ReportCrash's own cache.
        desymbolicatedReport,
        preNamedFrames: desymbolicated ? desymbolicated.preNamedFrames : 0,
        desymbolicatedFrames: desymbolicated ? desymbolicated.strippedFrames : 0,
      };
    } finally {
      removeDir(outDir);
    }
  }
  const result = run(tool, [ips, dsym, ...extra], { env });
  return {
    symbolicator: SYMBOLICATECRASH_TOOL,
    tool,
    argv: [tool, ips, dsym, ...extra],
    status: result?.status ?? -1,
    stdout: result?.stdout ?? "",
    stderr: result?.stderr ?? "",
    // A JSON report that reached this path means no CrashSymbolicator.py was found; the record says
    // so, because `symbolicatecrash` exits 255 on exactly that shape and names nothing.
    jsonReport,
    // stdout and stderr joined, unchanged from before the JSON route existed: this tool has always
    // been read from both streams, and narrowing it here would change the legacy verdict.
    framesText: `${result?.stdout ?? ""}${result?.stderr ?? ""}`,
  };
}

/**
 * What macOS writes in place of the user's home in an `.ips` path: `<home>/<anything>/<tail>` is
 * published as `/Users/USER/<*>/<tail>` (privacy redaction, applied by ReportCrash itself).
 *
 * The redaction point is the `*`; everything after it is the part ReportCrash keeps, which is what
 * makes a redacted path still comparable with the exact file this run spawned.
 */
export const REDACTED_HOME_PREFIX = "/Users/USER/";

/**
 * Whether `reported` is a redacted spelling of `spawned`, i.e. the same file under a hidden home.
 *
 * Both halves are required: the report path must carry the redaction marker, and the spawned path
 * must sit under the real home and end with the exact tail the report kept. Anything else -- an
 * unredacted path under another user, a redaction with a different tail, a bundle name that only
 * shares a suffix -- is NOT a match, so the strict comparison still refuses it.
 */
export function matchesRedactedProcPath({ reported, spawned, home = os.homedir() } = {}) {
  const named = String(reported ?? "");
  const actual = String(spawned ?? "");
  if (!named || !actual || !home) return null;
  // The redaction marker sits exactly where the home was; what follows it is the part that was kept.
  const marker = `${REDACTED_HOME_PREFIX}*`;
  if (!named.startsWith(marker)) return null;
  const kept = named.slice(marker.length).replace(/^\/+/, "");
  // Nothing kept means no path was preserved, so there is nothing to compare.
  if (!kept) return null;
  // Matched on a separator boundary: `kept` must be a whole component sequence of the spawned path,
  // never a token that merely happens to sit at the end of it.
  const suffix = `${path.sep}${kept}`;
  const spawnedPath = path.resolve(actual);
  const homePath = path.resolve(home);
  if (!spawnedPath.startsWith(`${homePath}${path.sep}`)) return null;
  if (!spawnedPath.endsWith(suffix)) return null;
  return { reported: named, spawned: spawnedPath, tail: suffix, home: homePath };
}

/**
 * Rewrites the report's executables the OS redacted into the path this run spawned.
 *
 * `evaluateForcedCrashProvenance` compares paths EXACTLY on purpose -- a report naming some other
 * executable must never pass -- so the reconciliation happens here, before that comparison, and
 * every substitution is handed back in `matched` for the evidence to state. Exact matches are left
 * alone: this can only ever turn a redacted-consistent path into the spawned one, never a mismatch
 * into a match, and the pid and build-UUID clauses are untouched.
 */
export function reconcileReportProcPaths({ forced = null, acceptedBinary = null, home = os.homedir() } = {}) {
  const named = Array.isArray(forced?.reportProcPaths) ? forced.reportProcPaths : [];
  if (!forced || !acceptedBinary || named.length === 0) return { forced, matched: [] };
  const spawned = path.resolve(acceptedBinary);
  const matched = [];
  const reconciled = named.map((reported) => {
    if (path.resolve(reported) === spawned) return reported;
    const redaction = matchesRedactedProcPath({ reported, spawned, home });
    if (!redaction) return reported;
    matched.push(redaction);
    return spawned;
  });
  if (matched.length === 0) return { forced, matched };
  return { forced: { ...forced, reportProcPaths: reconciled }, matched };
}

/**
 * Identifies the executable INSIDE the shipped tarball: digest plus build UUIDs, read from bytes.
 *
 * This is the half the UUID gate cannot cover. `assertDsymGate` proves the archive matches the
 * binary the operator pointed at; it says nothing about whether that binary is the one in
 * `artifacts/darwin/Ferryx.app.tar.gz`. Reading the member out of the tarball and hashing it closes
 * that gap without unpacking the app to disk or depending on a Darwin tool.
 */
export function inspectPackagedArtifact({ artifactPath, memberPath = APP_BINARY_MEMBER } = {}) {
  const record = {
    artifact: { path: artifactPath ? path.resolve(artifactPath) : null, memberPath },
    sha256: null,
    bytes: 0,
    uuids: [],
    problem: null,
  };
  if (!artifactPath || !existsSync(artifactPath)) {
    record.problem = `no artifact at ${artifactPath ?? "(none given)"}`;
    return record;
  }
  let bytes;
  try {
    bytes = readFileSync(artifactPath);
  } catch (error) {
    record.problem = `could not read ${artifactPath}: ${error.message}`;
    return record;
  }
  // The digest of the tarball itself is recorded too: it is what the release receipt publishes, so
  // an operator can tell WHICH artifact this evidence describes.
  record.artifact.sha256 = sha256Hex(bytes);
  record.artifact.bytes = bytes.length;
  const member = readTarGzMember({ bytes, memberPath });
  if (!member) {
    record.problem = `${artifactPath} holds no member ${memberPath}`;
    return record;
  }
  record.sha256 = member.sha256;
  record.bytes = member.bytes.length;
  record.uuids = machOUuids(member.bytes);
  return record;
}

/**
 * The negative fixture: a REAL dSYM carrying DIFFERENT build UUIDs, so the mismatch is not synthetic.
 *
 * A system binary is useless here -- `/bin/cat` is stripped, so `dsymutil` usually refuses it. A
 * two-line C file compiled with `-g` always yields a dSYM, and any host able to symbolicate has a
 * compiler. The fixture is then verified to carry no UUID of `binaryUuids`: if it accidentally did,
 * the "mismatch" run would not be testing a mismatch at all.
 */
export function produceMismatchedDsym({ outDir, binaryUuids = [], run = defaultRun }) {
  mkdirSync(outDir, { recursive: true });
  const source = path.join(outDir, "mismatch.c");
  // `main` is not optional. `clang -o` links an EXECUTABLE, and the macOS linker refuses one without
  // `_main` ("Undefined symbols for architecture arm64: _main"), which would turn this negative
  // fixture into a permanent host gap. The named function stays so the dSYM still carries a
  // distinguishing symbol of its own alongside its own UUID.
  writeFileSync(
    source,
    "int ferryx_dsym_acceptance_mismatch(int value) { return value + 1; }\n" +
      "int main(int argc, char **argv) { (void)argv; return ferryx_dsym_acceptance_mismatch(argc) == 0; }\n",
  );
  const object = path.join(outDir, "mismatch");
  const compiled = run("xcrun", ["clang", "-g", "-O0", "-o", object, source]);
  if (compiled.status !== 0) {
    throw new EnvironmentGap(
      "MISMATCH_DSYM_UNAVAILABLE",
      `clang could not build the mismatched-dSYM fixture: ${(compiled.stderr || compiled.stdout).trim().slice(-2000)}`,
    );
  }
  const bundle = path.join(outDir, "mismatch.dSYM");
  const made = run("xcrun", ["dsymutil", object, "-o", bundle]);
  if (made.status !== 0 || !existsSync(bundle)) {
    throw new EnvironmentGap(
      "MISMATCH_DSYM_UNAVAILABLE",
      `dsymutil could not archive the mismatched dSYM: ${(made.stderr || made.stdout).trim().slice(-2000)}`,
    );
  }
  // Read the fixture's own UUIDs back. A fixture that happened to collide with the binary would
  // make the negative run vacuous, so that is refused rather than reported as a pass.
  const uuids = run("dwarfdump", ["--uuid", bundle]);
  const carried = String(uuids.stdout ?? "")
    .split("\n")
    .map((line) => /UUID:\s*(\S+)/.exec(line)?.[1])
    .filter(Boolean)
    .map((uuid) => uuid.toUpperCase());
  if (carried.length === 0) {
    throw new EnvironmentGap("MISMATCH_DSYM_UNREADABLE", `dwarfdump read no UUID from ${bundle}`);
  }
  const collisions = carried.filter((uuid) => binaryUuids.includes(uuid));
  if (collisions.length > 0) {
    throw new EnvironmentGap(
      "MISMATCH_DSYM_COLLIDES",
      `the fixture dSYM carries ${collisions.join(", ")}, which is the binary's own UUID; the negative run would prove nothing`,
    );
  }
  return bundle;
}

/** The directory macOS writes crash reports to (the OS decides this; it cannot be overridden). */
export function diagnosticReportsDir(env = process.env) {
  const home = env.HOME || os.homedir();
  return path.join(home, "Library", "Logs", "DiagnosticReports");
}

/** Runs the shipped UUID gate and rethrows its typed refusal as a harness rejection (exit 1). */
function requireDsymGate({ binaryPath, dsymDir }) {
  try {
    return assertDsymGate({ binaryPath, dsymDir });
  } catch (error) {
    if (!(error instanceof DsymGateError)) throw error;
    // The gate already says which UUID is missing and why; the harness only supplies its exit code,
    // because a gate refusal is a release defect (1), not a host gap (3).
    throw new HarnessError(error.code, error.message, { binaryPath, dsymDir }, 1);
  }
}

/**
 * Forces a real SIGSEGV and returns the crash it produced, with its isolated root still on disk.
 *
 * The root is returned LIVE and must be removed by the caller AFTER the collector has run: the
 * crashed daemon's own `<root>/data/logs/daemon.log` lives there, and removing it first made the
 * collected bundle carry the operator's live-install log instead of the crash under test.
 */
export async function acquireForcedCrash({ binary, gate, deps = {} }) {
  const {
    rootPrefix = QA_ROOT_PREFIX,
    reportsDir = diagnosticReportsDir(),
    spawnImpl = spawn,
    watchImpl = watch,
    now = () => Date.now(),
    makeRoot = (prefix) => mkdtempSync(path.join(os.tmpdir(), prefix)),
    envOf = isolatedEnv,
    readBytes = readFileSync,
    readyDeadlineMs = READY_DEADLINE_MS,
    exitDeadlineMs = EXIT_DEADLINE_MS,
    segvRetryMs = SEGV_RETRY_DELAY_MS,
    reportDeadlineMs = CRASH_REPORT_DEADLINE_MS,
    ensureDir = ensureReportsDir,
  } = deps;
  // Before the snapshot: a host that has never crashed anything has no reports directory yet, and
  // ReportCrash cannot write into a directory that does not exist.
  ensureDir(reportsDir);
  const before = listCrashReports(reportsDir);
  const root = makeRoot(rootPrefix);
  try {
    const env = envOf(root);
    // Capture the executable's identity BEFORE it runs. Hashing after the crash would describe
    // whatever file sits at that path then, which is the weakness this binding exists to remove.
    const preImage = readBytes(binary);
    const spawnedSha256 = sha256Hex(preImage);
    const binaryUuids = machOUuids(preImage);
    const spawnedAt = new Date(now()).toISOString();
    // `forceSegv` resolves only on a real SIGSEGV; any other ending is a typed refusal there.
    const forced = await forceSegv({ binary, env, readyDeadlineMs, exitDeadlineMs, segvRetryMs, spawnImpl });
    const crashPath = await awaitNewCrashReport({
      dir: reportsDir,
      before,
      uuids: gate.binaryUuids,
      deadlineAt: now() + reportDeadlineMs,
      watchImpl,
      now,
      ensureDir,
    });
    if (!crashPath) {
      throw new EnvironmentGap(
        "NO_CRASH_REPORT",
        `no new .ips naming ${gate.binaryUuids.join(", ")} in ${reportsDir} within ${reportDeadlineMs}ms; ` +
          "ReportCrash may be disabled on this host",
        { forcedPid: forced.pid },
      );
    }
    // The report did not exist in the snapshot taken before the signal, so it was written after
    // this run started. Read its own identity back so the pid and executable can be compared with
    // the ones this run spawned: that comparison is what a supplied `--ips` cannot satisfy.
    let crashText = "";
    try {
      crashText = readFileSync(crashPath, "utf8");
    } catch {
      crashText = "";
    }
    // Re-verify the digest now the process is gone: an executable replaced mid-run would otherwise
    // leave evidence describing bytes that never crashed. The bytes are kept (not just a digest)
    // so `buildBinaryIdentity` can state both halves of the binding from one read of the file.
    let postImage = null;
    try {
      postImage = readBytes(binary);
    } catch {
      postImage = null;
    }
    const postSha256 = postImage ? sha256Hex(postImage) : null;
    const forcedRecord = buildForcedCrashRecord({
      outcome: {
        ...forced,
        spawnedSha256,
        sha256UnchangedAfterCrash: postSha256 === spawnedSha256,
        binaryUuids,
        spawnedAt,
      },
      crashText,
      reportPath: crashPath,
      reportWrittenAfterCrash: true,
    });
    // The lib builder publishes the crash-binding fields; the SIGSEGV deliveries are this harness's
    // own observation of the child it spawned, so they are attached here. `signal` stays SIGSEGV:
    // the re-delivery is how the same signal is made to land, not a different outcome.
    forcedRecord.signalDeliveries = forced.signalDeliveries ?? [];
    forcedRecord.signalCount = forced.signalCount ?? forcedRecord.signalDeliveries.length;
    // The identity the evidence report publishes as `binary.sha256`: the digest read BEFORE the
    // process ran, with the post-crash digest attached as proof the executable did not move.
    const identity = buildBinaryIdentity({ path: binary, before: preImage, after: postImage });
    return { crashPath, forced: forcedRecord, env, root, identity, binarySha256: spawnedSha256, binaryUuids };
  } catch (error) {
    // A run that never produced a crash owns no evidence, so its root is released here.
    rmSync(root, { recursive: true, force: true });
    throw error;
  }
}

/**
 * The binary identity to publish when the runner did not hand one over.
 *
 * The forced-crash record already carries both halves of the bytes binding (`spawnedSha256` read
 * before the signal, `sha256UnchangedAfterCrash` read after), so the report can be built from it
 * without re-reading a file whose contents may have moved. Re-reading instead would silently
 * describe the post-crash file and defeat the reason the digest is captured before the crash.
 */
export function binaryIdentityFromForced({ binary, forced = null } = {}) {
  if (!forced || !forced.spawnedSha256) return null;
  const unchanged = forced.sha256UnchangedAfterCrash === true;
  return {
    path: path.resolve(forced.spawnedBinary ?? binary),
    sha256: forced.spawnedSha256,
    sha256AfterCrash: unchanged ? forced.spawnedSha256 : null,
    bytes: null,
    uuids: forced.binaryUuids ?? [],
    capturedBeforeCrash: true,
    verifiedUnchangedAfterCrash: unchanged,
  };
}

/**
 * Collects, symbolizes and judges one crash against the archive, then writes the evidence.
 *
 * Everything the acceptance reports comes from an observation here: the collector's own bundle,
 * the tool's output, and the gate's refusal. `env` is the isolated environment the crash ran in
 * (null when the operator supplied the report), so the log tail in the bundle is provably the
 * crashed daemon's rather than whatever log the operator's live install happens to hold.
 *
 * `developerDir` is handed to `symbolicatecrash` as `DEVELOPER_DIR`, which the script requires;
 * `home` is the real home a report path may have been redacted against (`/Users/USER/<*>/...`).
 */
export function runSymbolicationChecks({
  binary,
  gate,
  crashPath,
  dsymArchive,
  reportDir,
  tool,
  crashSymbolicator = null,
  developerDir = null,
  env = null,
  forced = null,
  packaged = null,
  identity = null,
  home = os.homedir(),
  readBytes = readFileSync,
  run = defaultRun,
}) {
  // 3. The release's own collector must find the report's UUIDs and a matching dSYM in the archive.
  const bundleDir = path.join(reportDir, "bundle");
  const collected = collectBundle({ binary, ips: crashPath, dsym: dsymArchive, out: bundleDir, env, run });
  // Record where the collected log came from. A log from another process must never be read as
  // this crash's diagnostics, so the report states which root it was copied out of.
  const daemonLog = typeof collected.daemonLog === "string" ? collected.daemonLog : null;
  let daemonLogBytes = 0;
  if (daemonLog) {
    try {
      daemonLogBytes = statSync(daemonLog).size;
    } catch {
      daemonLogBytes = 0;
    }
  }
  const isolatedData = env?.FERRYX_DATA_DIR ? path.resolve(env.FERRYX_DATA_DIR) : null;
  const daemonLogFromCrashedRoot = Boolean(
    daemonLog && isolatedData && path.resolve(daemonLog).startsWith(`${isolatedData}${path.sep}`),
  );
  const copiedCrashReport = typeof collected.crashReport === "string" ? collected.crashReport : null;
  // The bundle must describe the crash under test, not just any `.ips` the collector found first.
  const collectorCopiedCrashReport = {
    ok: Boolean(copiedCrashReport) && path.basename(copiedCrashReport) === path.basename(crashPath),
    detail: copiedCrashReport
      ? `bundle holds ${path.basename(copiedCrashReport)}`
      : `collector reported no copied crash report for ${crashPath}`,
  };

  // 4. Symbolicate the real crash through the exact archived bundles that carry its UUID. The first
  // path stays the archive dir for `symbolicatecrash`'s own dSYM search, but every matched bundle is
  // passed explicitly and recorded: that record is what an operator replays.
  const dsymTargets = resolveSymbolicationBundles({ bundles: gate.bundles ?? [], uuids: gate.binaryUuids ?? [] });
  // No matched bundle means the gate was bypassed or the archive is unreadable; symbolicating anyway
  // would produce evidence for a bundle nobody can name, so it is refused before the tool runs.
  if (dsymTargets.paths.length === 0) {
    throw new HarnessError(
      "DSYM_TARGETS_UNRESOLVED",
      `no dSYM under ${dsymArchive} carries ${(gate.binaryUuids ?? []).join(", ") || "the binary's UUID"}; the run gate would have refused this archive`,
      { dsymArchive },
      1,
    );
  }
  // `symbolicatecrash` refuses to start without `DEVELOPER_DIR` in its environment, so the dir the
  // tool was found under is passed through instead of relying on the operator having exported it.
  const toolEnv = symbolicationEnv({ developerDir });
  const positive = symbolicate({
    tool,
    crashSymbolicator,
    ips: crashPath,
    dsym: dsymTargets.paths[0],
    // The JSON tool searches its `-d` directory recursively for `DWARF/<image name>`, so handing it
    // the archive covers every matched bundle; `dsym` above is still what the legacy tool gets.
    dsymSearchDir: dsymArchive,
    extra: dsymTargets.paths.slice(1),
    env: toolEnv,
    binaryUuids: gate.binaryUuids ?? [],
    run,
    readText: (file) => readBytes(file, "utf8"),
    // Written under the evidence directory (not /tmp) and kept, so the recorded argv names a file
    // that is still there when an operator replays it against the same report and archive.
    makeOutDir: () => symbolicatedJsonDir(reportDir, "positive"),
    removeDir: () => {},
    // Both runs are handed the SAME de-symbolicated copy, in one directory under the evidence, so
    // the positive and the negative verdict measure the dSYM and not ReportCrash's pre-filled names.
    desymbolicatedDir: reportDir,
  });
  const positiveVerdict = evaluateSymbolication({
    crashText: readBytes(crashPath, "utf8"),
    symbolicatedText: positive.framesText,
  });

  // 5. The negative run: the real gate must refuse a mismatched archive, and symbolication under
  // that archive must name nothing for this binary.
  const mismatchDir = mkdtempSync(path.join(reportDir, "mismatch-"));
  let negativeVerdict;
  let gateCode = null;
  try {
    produceMismatchedDsym({ outDir: mismatchDir, binaryUuids: gate.binaryUuids, run });
    try {
      assertDsymGate({ binaryPath: binary, dsymDir: mismatchDir, exec: run });
    } catch (error) {
      if (!(error instanceof DsymGateError)) throw error;
      gateCode = error.code;
    }
    // The negative run goes through the SAME tool as the positive one. That matters: a mismatch
    // must be proved to name nothing by the symbolicator that just named frames, not by a
    // different tool with different failure modes. It is handed ONLY the mismatched fixture -- the
    // archive that just named frames is deliberately not in its search path, or the run would be
    // given the real dSYM and prove nothing. No special case is needed: CrashSymbolicator only
    // opens a dSYM whose UUID matches the image's, so a mismatched archive names nothing.
    const negative = symbolicate({
      tool,
      crashSymbolicator,
      ips: crashPath,
      dsym: mismatchDir,
      dsymSearchDir: mismatchDir,
      env: toolEnv,
      binaryUuids: gate.binaryUuids ?? [],
      run,
      readText: (file) => readBytes(file, "utf8"),
      makeOutDir: () => symbolicatedJsonDir(reportDir, "negative"),
      removeDir: () => {},
      desymbolicatedDir: reportDir,
    });
    negativeVerdict = evaluateMismatchedDsym({ gateCode, symbolicatedText: negative.framesText });
    writeFileSync(path.join(reportDir, "mismatch-symbolicated.txt"), `${negative.stdout}${negative.stderr}`);
  } finally {
    rmSync(mismatchDir, { recursive: true, force: true });
  }

  // 6. The forced crash must be one THIS run produced, and the symbolicated bytes must be the ones
  // inside the shipped tarball. Both are judged here so they land in `checks` (which the release
  // gate reads) rather than in prose an operator could overlook.
  // macOS redacts the user's home in the report's paths (`/Users/USER/*/...`), so the executable
  // is reconciled with the one this run spawned BEFORE the strict comparison, and every redacted
  // match is recorded: the report states that the path was compared in redacted form. A path that
  // is redacted inconsistently, or unredacted and different, is left as it is and still refused.
  const reconciled = reconcileReportProcPaths({ forced, acceptedBinary: binary, home });
  const provenanceForced = reconciled.forced;
  const provenanceVerdict = evaluateForcedCrashProvenance({ forced: provenanceForced, acceptedBinary: binary });
  const provenanceDetail =
    reconciled.matched.length === 0
      ? provenanceVerdict.detail
      : `${provenanceVerdict.detail}; report path(s) ${reconciled.matched
          .map((match) => `${match.reported} matched in redacted form as ${match.spawned}`)
          .join(", ")}`;
  // `binary.sha256` MUST be the digest captured before the process ran: the gate compares it with
  // the executable inside the shipped tarball, and a hash taken from the path afterwards describes
  // whatever file sits there then. `identity` comes from the pre-execution read; when it is absent
  // the same two observations are recovered from the forced-crash record, never from a fresh read.
  const binaryIdentity = identity ?? binaryIdentityFromForced({ binary, forced });
  const identityVerdict = evaluateBinaryIdentity({ binary: binaryIdentity, forced });
  // `packaged` is either the artifact path or a record `inspectPackagedArtifact` already built.
  const packagedRecord =
    typeof packaged === "string"
      ? inspectPackagedArtifact({ artifactPath: packaged })
      : packaged && typeof packaged === "object" && "sha256" in packaged
        ? packaged
        : null;
  const packagedVerdict = evaluatePackagedBinaryIdentity({
    accepted: {
      path: binaryIdentity?.path ?? binary,
      sha256: binaryIdentity?.sha256 ?? null,
      uuids: binaryIdentity?.uuids?.length ? binaryIdentity.uuids : gate.binaryUuids ?? [],
      capturedBeforeCrash: binaryIdentity?.capturedBeforeCrash === true,
      verifiedUnchangedAfterCrash: binaryIdentity?.verifiedUnchangedAfterCrash === true,
    },
    packaged: packagedRecord,
  });

  writeFileSync(path.join(reportDir, "positive-symbolicated.txt"), `${positive.stdout}${positive.stderr}`);
  writeFileSync(path.join(reportDir, "crash-report-path.txt"), `${crashPath}\n`);
  const checks = {
    ...positiveVerdict.checks,
    ...negativeVerdict.checks,
    collectorCopiedCrashReport,
    forcedCrashProvenance: { ok: provenanceVerdict.ok, detail: provenanceDetail },
    binaryIdentity: { ok: identityVerdict.ok, detail: identityVerdict.detail },
    packagedBinaryIdentity: { ok: packagedVerdict.ok, detail: packagedVerdict.detail },
  };
  const ok = Object.values(checks).every((check) => check.ok);
  const report = {
    schema: SYMBOLICATION_SCHEMA,
    collectedAt: new Date().toISOString(),
    verdict: ok ? "PASS" : "FAIL",
    // Exactly what this run observed: the report's executable paths stay as the OS wrote them, and
    // any redaction that was reconciled away is stated separately by `redactedProcPaths`.
    forcedCrash: forced,
    redactedProcPaths: reconciled.matched,
    // `sha256` is REQUIRED by the release gate: it is the digest captured before the crash, and
    // the gate refuses evidence whose packaged executable hashes to anything else.
    binary: {
      path: binaryIdentity?.path ?? binary,
      sha256: binaryIdentity?.sha256 ?? null,
      sha256AfterCrash: binaryIdentity?.sha256AfterCrash ?? null,
      bytes: binaryIdentity?.bytes ?? null,
      uuids: binaryIdentity?.uuids?.length ? binaryIdentity.uuids : gate.binaryUuids,
      capturedBeforeCrash: binaryIdentity?.capturedBeforeCrash === true,
      verifiedUnchangedAfterCrash: binaryIdentity?.verifiedUnchangedAfterCrash === true,
    },
    dsymArchive: {
      dir: dsymArchive,
      uuids: [...new Set(gate.bundles.flatMap((bundle) => bundle.uuids))],
    },
    // The exact bundles handed to `symbolicatecrash`, so the run is reproducible by hand.
    dsymTargets: {
      paths: dsymTargets.paths,
      bundles: dsymTargets.bundles.map((bundle) => ({ path: bundle.path, uuids: bundle.uuids })),
      missing: dsymTargets.missing,
    },
    packagedBinary: packagedRecord
      ? {
          artifact: packagedRecord.artifact,
          memberPath: packagedRecord.artifact?.memberPath ?? null,
          sha256: packagedRecord.sha256,
          bytes: packagedRecord.bytes,
          uuids: packagedRecord.uuids,
          problem: packagedRecord.problem ?? null,
        }
      : null,
    symbolicatecrash: tool,
    crashSymbolicator,
    // WHICH tool produced the frames. A `.ips` on a current macOS can only be read by
    // CrashSymbolicator.py, so this field is what tells an operator the reported frames were not a
    // legacy-tool result. `symbolicatecrash` is kept above so older evidence stays comparable.
    symbolicator: positive.symbolicator,
    symbolicatorArgv: positive.argv,
    // The environment the tool was given. `symbolicatecrash` will not run without DEVELOPER_DIR, so
    // an operator replaying the recorded command needs this value too, not just the path.
    symbolicatecrashEnv: {
      developerDir,
      env: developerDir ? { DEVELOPER_DIR: developerDir } : {},
    },
    crash: { path: crashPath, uuids: positiveVerdict.crashUuids },
    collector: {
      reportDir: collected.reportDir ?? bundleDir,
      dsym: collected.dsym ?? null,
      crashReport: copiedCrashReport,
      daemonLog,
      daemonLogBytes,
      // True only when the log in the bundle came out of the isolated root the crashed daemon ran
      // under; anything else is some other process's log and is not this crash's diagnostics.
      daemonLogFromCrashedRoot,
    },
    symbolicated: {
      status: positive.status,
      // Which tool ran, and whether the report was the JSON shape that requires CrashSymbolicator.
      symbolicator: positive.symbolicator,
      reportFormat: positive.jsonReport ? "ips-json" : "legacy-text",
      // The copy the tool was given, and how many binary frames the RAW report already named.
      // ReportCrash names frames itself from a UUID-keyed cache that survives `strip`, so a raw
      // report can arrive fully named and a verdict scored on it would owe nothing to the dSYM.
      // These two fields are the transparency that makes a PASS mean the archive did the work.
      desymbolicatedReport: positive.desymbolicatedReport ?? null,
      desymbolicatedFrames: positive.desymbolicatedFrames ?? 0,
      preNamedFrames: positive.preNamedFrames ?? 0,
      frames: positiveVerdict.frames.length,
      binaryFrames: positiveVerdict.binaryFrames.length,
      namedFrames: positiveVerdict.namedFrames.length,
      anonymousFrames: positiveVerdict.anonymousFrames.length,
      // Kept so a zero-frame result can be read as "the tool refused" rather than guessed at.
      stderrTail: String(positive.stderr ?? "").slice(-2000),
    },
    negative: { gateCode, frames: negativeVerdict.frames.length, namedFrames: negativeVerdict.namedFrames.length },
    checks,
    reportDir,
  };
  writeFileSync(path.join(reportDir, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
  return report;
}

async function main(argv) {
  const args = parseArgs(argv);
  const binary = assertSafeTarget({ binary: args.binary });
  if (!existsSync(args.dsym)) throw new HarnessError("DSYM_MISSING", `no dSYM archive at ${args.dsym}`);
  if (!existsSync(args.artifact)) throw new HarnessError("ARTIFACT_MISSING", `no shipped artifact at ${args.artifact}`);
  // A `--ips` that does not exist would otherwise surface later as an unreadable-report bug.
  if (args.ips && !existsSync(args.ips)) throw new HarnessError("USAGE", `no crash report at ${args.ips}`);
  // A supplied report can be triaged, but it can never be the forced crash the release requires.
  assertReportSource({ ips: args.ips, binary, dryRun: args.dryRun });
  const reportDir = path.resolve(args.out ?? path.join(process.cwd(), ".omo/evidence/dsym-symbolication-20261116"));
  const plan = {
    schema: SYMBOLICATION_SCHEMA,
    binary,
    dsymArchive: path.resolve(args.dsym),
    artifact: path.resolve(args.artifact),
    artifactMember: APP_BINARY_MEMBER,
    crashReport: args.ips ? path.resolve(args.ips) : null,
    reportDir,
  };
  if (args.dryRun) {
    process.stdout.write(`${JSON.stringify({ ...plan, dryRun: true }, null, 2)}\n`);
    return 0;
  }

  // The tool AND the developer dir it refuses to run without: one lookup, so the dir the framework
  // candidates came from is the dir handed to the script's own environment. The same lookup also
  // resolves `CrashSymbolicator.py`, the only tool that can read a modern `.ips`.
  const { tool, developerDir, crashSymbolicator } = resolveSymbolicatecrash();
  if (!tool) {
    throw new EnvironmentGap(
      "SYMBOLICATECRASH_UNAVAILABLE",
      "symbolicatecrash not found; install full Xcode (Command Line Tools alone do not ship it) or set SYMBOLICATECRASH",
      { crashSymbolicator },
    );
  }

  // 1. The archive must be keyed to this binary before any crash is read. A gate failure here is the
  // same refusal the release pipeline gives, so the acceptance can never pass on a stale dSYM.
  const gate = requireDsymGate({ binaryPath: binary, dsymDir: plan.dsymArchive });
  mkdirSync(reportDir, { recursive: true });

  // 2-5. Crash first, collect second, inside ONE isolated-root lifetime. The collector reads the
  // crashed daemon's own log out of that root, so the root must still exist when it runs; deleting
  // it first made the bundle carry the operator's live-install log instead of this crash's.
  let root = null;
  let env = null;
  let forced = null;
  let crashPath = null;
  let identity = null;
  let report;
  try {
    // Always forced: the release gate's provenance check requires a pid this run spawned, so a
    // supplied report has no path to PASS and is refused by `parseArgs`/main above.
    const acquired = await acquireForcedCrash({ binary, gate });
    root = acquired.root;
    env = acquired.env;
    forced = acquired.forced;
    crashPath = acquired.crashPath;
    identity = acquired.identity;
    report = runSymbolicationChecks({
      binary,
      gate,
      crashPath,
      dsymArchive: plan.dsymArchive,
      reportDir,
      tool,
      crashSymbolicator,
      developerDir,
      env,
      forced,
      identity,
      packaged: plan.artifact,
    });
  } finally {
    // Only now: the collector copied the log and the .ips out of this root before it is removed.
    if (root) rmSync(root, { recursive: true, force: true });
  }

  if (args.json) process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  else process.stdout.write(`${renderSymbolicationSummary(report)}\n`);
  return report.verdict === "PASS" ? 0 : 1;
}

if (import.meta.main) {
  void (async () => {
    try {
      process.exit(await main(process.argv.slice(2)));
    } catch (error) {
      if (error instanceof HarnessError) {
        process.stderr.write(`${error.message}\n`);
        process.exit(error.exit);
      }
      process.stderr.write(`${error?.stack ?? error}\n`);
      process.exit(1);
    }
  })();
}
