/**
 * Spawn-time PTY identity producer for the Herdr reference-chat acceptance harness (task 14).
 *
 * WHY THIS EXISTS
 *   Neither the gateway's RemoteSessionDetails nor the daemon wire carries a shell child PID
 *   (see notes/facts/ferryx-original-pty-pid-seam-2026-10-07.md). The in-process
 *   TerminalService::get_session -> PtySession::pid seam exists, but exposing it on the remote
 *   wire would be a production protocol change this task is not authorized to make.
 *
 *   The gateway PID must never stand in for the shell PID, and a descendant / process-table
 *   scan is NOT spawn-recorded identity. So the PTY child RECORDS ITS OWN IDENTITY, using a
 *   mechanism the daemon already supports.
 *
 * MECHANISM (no protocol expansion, no scan)
 *   The daemon's Spawn request carries `shell: Option<String>`, and
 *   `terminal::shell::resolve_shell_command_pure` uses a non-empty preference as the program
 *   verbatim on macOS, Linux and Windows. The provisioner therefore passes the path of a
 *   generated wrapper as that preference, and the daemon spawns the wrapper as the DIRECT PTY
 *   child. The wrapper records its own identity, then becomes the real shell.
 *
 * ROLES — never conflated
 *   Every record states the EXACT executable recorded at launch and that process's role:
 *
 *     ptyChild     the direct PTY child the daemon spawned. Executable = the WRAPPER.
 *                  role "pty-child-wrapper". Recorded by the wrapper itself, at launch.
 *     sessionShell the shell the pane actually runs. Executable = the real shell.
 *                  role "session-shell". On unix the wrapper `exec`s it, so the PID is
 *                  PRESERVED across the exec and `execPreservesPid` is true; the executable
 *                  changes, and the wrapper is never relabelled as the shell.
 *     provider     the provider process, when the provisioner resolved one. role
 *                  "provider-process". Never inferred from a label.
 *
 *   On Windows `cmd.exe` cannot `exec`, so the wrapper stays as the PTY child and the real
 *   shell runs as its child. The wrapper records its own PID exactly; it does NOT guess the
 *   child's PID, so `sessionShell.pidKnown` is false there and the receipt says so rather than
 *   inventing a value. No scan is performed to fill that gap.
 *
 * NOT VERIFIED: this module is authored, not executed.
 */

import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  unlinkSync,
  writeFileSync,
  watch,
} from "node:fs";
import { dirname, join, resolve } from "node:path";

export const PTY_IDENTITY_SCHEMA = "ferryx-herdr-reference.pty-identity/1";

/** The role names a receipt may carry. */
export const PTY_IDENTITY_ROLES = ["pty-child-wrapper", "session-shell", "provider-process"];

/** Quote a value for a POSIX single-quoted string. */
function shQuote(value) {
  return "'" + String(value).split("'").join("'\\''") + "'";
}

/** Quote a value for a Windows cmd argument. */
function cmdQuote(value) {
  return '"' + String(value).split('"').join('""') + '"';
}

/** Quote a value for a PowerShell single-quoted string. */
function psQuote(value) {
  return "'" + String(value).split("'").join("''") + "'";
}

// Split so the literal does not collide with JavaScript template interpolation.
const SIZE_ROWS = "$" + "{size%% *}";
const SIZE_COLS = "$" + "{size##* }";
const DOLLAR = "$";

/**
 * Generate the wrapper for one session.
 *
 * `realShell` is the shell the pane must actually run; the caller resolves it from host config
 * and never lets this module guess. `providerCommand` is optional and, when present, is
 * recorded as the provider process's launch command rather than being run by the wrapper.
 */
export function writePtyIdentityWrapper(options) {
  const platform = options.platform || (process.platform === "win32" ? "windows" : "unix");
  const dir = options.dir;
  const outputPath = options.outputPath;
  const realShell = options.realShell;
  if (!dir || !outputPath || !realShell) {
    throw new Error("writePtyIdentityWrapper needs dir, outputPath and realShell");
  }
  mkdirSync(dir, { recursive: true });
  mkdirSync(dirname(outputPath), { recursive: true });
  const tempPath = outputPath + ".tmp";
  const providerCommand = options.providerCommand || null;

  if (platform === "windows") {
    const wrapperPath = join(dir, "pty-identity-wrapper.cmd");
    const loginArgs = options.loginArgs || [];
    // The cmd process IS the direct PTY child. Its own PID is exact; the shell it starts is a
    // child whose PID cmd cannot obtain without a scan, so pidKnown is recorded as false and
    // no value is fabricated.
    const record = [
      DOLLAR + "o=[ordered]@{",
      "schema=" + psQuote(PTY_IDENTITY_SCHEMA) + ";",
      "platform='windows';",
      "ptyChild=[ordered]@{",
      "pid=" + DOLLAR + "PID;",
      "executable=" + psQuote(wrapperPath) + ";",
      "role='pty-child-wrapper';",
      "recordedBy='wrapper-self'",
      "};",
      "sessionShell=[ordered]@{",
      "pid=" + DOLLAR + "null;",
      "pidKnown=" + DOLLAR + "false;",
      "executable=" + psQuote(realShell) + ";",
      "role='session-shell';",
      "recordedBy='wrapper-launch-command';",
      "execPreservesPid=" + DOLLAR + "false",
      "};",
      "provider=" + (providerCommand
        ? "[ordered]@{ executable=" + psQuote(providerCommand) + "; role='provider-process'; pid=" + DOLLAR + "null; pidKnown=" + DOLLAR + "false }"
        : DOLLAR + "null") + ";",
      "cols=" + DOLLAR + "Host.UI.RawUI.WindowSize.Width;",
      "rows=" + DOLLAR + "Host.UI.RawUI.WindowSize.Height;",
      "recordedAt=(Get-Date).ToUniversalTime().ToString('o')",
      "};",
      DOLLAR + "o | ConvertTo-Json -Compress -Depth 5 | Set-Content -LiteralPath " + cmdQuote(tempPath) + " -Encoding ascii;",
      "Move-Item -Force -LiteralPath " + cmdQuote(tempPath) + " -Destination " + cmdQuote(outputPath),
    ].join(" ");
    const body = [
      "@echo off",
      "rem Herdr reference-chat QA PTY identity wrapper (task14). Generated per run.",
      "rem This cmd process is the DIRECT PTY child the daemon spawned. It records its own",
      "rem identity exactly, then runs the real shell with inherited stdio so interactive",
      "rem semantics are unchanged. It never relabels itself as the shell.",
      "powershell -NoProfile -Command \"" + record + "\"",
      cmdQuote(realShell) + (loginArgs.length > 0 ? " " + loginArgs.map(cmdQuote).join(" ") : ""),
      "",
    ].join("\r\n");
    writeFileSync(wrapperPath, body);
    return { schema: PTY_IDENTITY_SCHEMA, platform, wrapperPath, outputPath, realShell, execPreservesPid: false };
  }

  const wrapperPath = join(dir, "pty-identity-wrapper.sh");
  const loginArgs = options.loginArgs || ["-l"];
  const body = [
    "#!/bin/sh",
    "# Herdr reference-chat QA PTY identity wrapper (task14). Generated per run.",
    "# Records this process's own identity as the DIRECT PTY child (the wrapper), then execs",
    "# the real shell. exec replaces the process image, not the process, so the recorded PID",
    "# is preserved and remains the surviving session PID -- while the executable CHANGES from",
    "# the wrapper to the shell. The two are recorded as separate roles and never conflated.",
    "out=" + shQuote(outputPath),
    "tmp=" + shQuote(tempPath),
    "{",
    "  printf '{\"schema\":\"%s\",\"platform\":\"unix\",\"ptyChild\":{\"pid\":%s,\"executable\":\"%s\",\"role\":\"pty-child-wrapper\",\"recordedBy\":\"wrapper-self\"},\"sessionShell\":{\"pid\":%s,\"pidKnown\":true,\"executable\":\"%s\",\"role\":\"session-shell\",\"recordedBy\":\"wrapper-exec-target\",\"execPreservesPid\":true}' \\",
    "    " + shQuote(PTY_IDENTITY_SCHEMA) + " \"$$\" " + shQuote(wrapperPath) + " \"$$\" " + shQuote(realShell),
    "  if [ -n " + shQuote(providerCommand || "") + " ]; then",
    "    printf ',\"provider\":{\"executable\":\"%s\",\"role\":\"provider-process\",\"pid\":null,\"pidKnown\":false}' " + shQuote(providerCommand || ""),
    "  else",
    "    printf ',\"provider\":null'",
    "  fi",
    "  size=$(stty size 2>/dev/null)",
    "  if [ -n \"$size\" ]; then",
    "    printf ',\"rows\":%s,\"cols\":%s' \"" + SIZE_ROWS + "\" \"" + SIZE_COLS + "\"",
    "  fi",
    "  printf ',\"recordedAt\":\"%s\"}\\n' \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\"",
    "} > \"$tmp\" 2>/dev/null",
    "if [ -s \"$tmp\" ]; then mv -f \"$tmp\" \"$out\"; else rm -f \"$tmp\"; fi",
    "exec " + shQuote(realShell) + (loginArgs.length > 0 ? " " + loginArgs.map(shQuote).join(" ") : ""),
    "",
  ].join("\n");
  writeFileSync(wrapperPath, body);
  try {
    chmodSync(wrapperPath, 0o755);
  } catch {
    /* A filesystem without POSIX modes still records; the spawn reports any failure. */
  }
  return { schema: PTY_IDENTITY_SCHEMA, platform, wrapperPath, outputPath, realShell, execPreservesPid: true };
}

/**
 * Wait for the wrapper's identity record.
 *
 * Event-driven with a bounded deadline: the directory is watched for the rename the wrapper
 * performs, plus exactly one immediate re-check for a record that landed before the watch was
 * armed. No fixed sleep and no polling loop.
 */
export function readPtyIdentity(outputPath, timeoutMs) {
  const limit = typeof timeoutMs === "number" ? timeoutMs : 20000;
  const attempt = () => {
    if (!existsSync(outputPath)) return null;
    try {
      return JSON.parse(readFileSync(outputPath, "utf8"));
    } catch {
      return null;
    }
  };
  const immediate = attempt();
  if (immediate) return Promise.resolve(immediate);
  return new Promise((resolveRead, rejectRead) => {
    let watcher = null;
    let timer = null;
    const finish = (value, error) => {
      if (watcher) {
        try {
          watcher.close();
        } catch {
          /* Already closed. */
        }
      }
      if (timer) clearTimeout(timer);
      if (error) rejectRead(error);
      else resolveRead(value);
    };
    const check = () => {
      const value = attempt();
      if (value) finish(value, null);
    };
    timer = setTimeout(
      () => finish(null, new Error("the PTY identity wrapper did not record within " + limit + "ms")),
      limit,
    );
    try {
      watcher = watch(dirname(outputPath), () => check());
    } catch {
      /* An unwatchable directory still gets the bounded re-check below. */
    }
    setTimeout(check, 0);
  });
}

/**
 * Validate a recorded identity.
 *
 * The wrapper's own record is checked for shape and role; the session shell is checked to be a
 * DIFFERENT executable from the wrapper (an identical path would mean the wrapper was
 * relabelled as the shell, which this contract forbids). When a probe is supplied, the live
 * executable of the recorded session PID must still match — that is what stops a recycled PID
 * from being mistaken for the original session.
 */
export function validatePtyIdentity(record, expected, probe) {
  const errors = [];
  if (!record || typeof record !== "object") {
    return { ok: false, errors: ["no PTY identity was recorded"] };
  }
  if (record.schema !== PTY_IDENTITY_SCHEMA) {
    errors.push("PTY identity schema is " + String(record.schema));
  }
  const ptyChild = record.ptyChild;
  if (!ptyChild || typeof ptyChild !== "object") {
    errors.push("PTY identity records no ptyChild block");
  } else {
    if (!Number.isInteger(ptyChild.pid) || ptyChild.pid <= 0) {
      errors.push("PTY identity ptyChild carries no usable pid: " + String(ptyChild.pid));
    }
    if (typeof ptyChild.executable !== "string" || ptyChild.executable.trim().length === 0) {
      errors.push("PTY identity ptyChild names no executable");
    }
    if (ptyChild.role !== "pty-child-wrapper") {
      errors.push("PTY identity ptyChild role is " + String(ptyChild.role) + ", expected pty-child-wrapper");
    }
  }
  const sessionShell = record.sessionShell;
  if (!sessionShell || typeof sessionShell !== "object") {
    errors.push("PTY identity records no sessionShell block");
  } else {
    if (sessionShell.role !== "session-shell") {
      errors.push("PTY identity sessionShell role is " + String(sessionShell.role) + ", expected session-shell");
    }
    if (typeof sessionShell.executable !== "string" || sessionShell.executable.trim().length === 0) {
      errors.push("PTY identity sessionShell names no executable");
    }
    // The wrapper must never be relabelled as the shell.
    if (ptyChild && ptyChild.executable && sessionShell.executable === ptyChild.executable) {
      errors.push("the wrapper executable was recorded as the session shell as well");
    }
    if (expected && expected.shell && sessionShell.executable !== expected.shell) {
      errors.push(
        "PTY identity sessionShell " + sessionShell.executable + " is not the requested " + expected.shell,
      );
    }
    // A PID may only be trusted where the record says it is known and how it survived.
    if (sessionShell.pidKnown === true) {
      if (sessionShell.execPreservesPid === true && ptyChild && sessionShell.pid !== ptyChild.pid) {
        errors.push(
          "PTY identity claims exec preserved the pid but sessionShell.pid " +
            String(sessionShell.pid) + " differs from ptyChild.pid " + String(ptyChild.pid),
        );
      }
    } else if (sessionShell.pid !== null && sessionShell.pid !== undefined) {
      errors.push("PTY identity reports a session pid while claiming pidKnown false");
    }
  }
  const live = typeof probe === "function" && ptyChild && ptyChild.pid ? probe(ptyChild.pid) : null;
  if (live && !live.alive) {
    errors.push("the recorded PTY child pid " + ptyChild.pid + " is not alive");
  }
  if (live && live.alive && sessionShell && sessionShell.execPreservesPid === true) {
    const recorded = String(sessionShell.executable);
    const actual = String(live.executable || "");
    const matches = actual === recorded ||
      actual.endsWith("/" + recorded.split("/").pop()) ||
      actual.endsWith("\\" + recorded.split("\\").pop());
    if (!matches) {
      errors.push(
        "the recorded PTY child pid " + ptyChild.pid + " now runs " + actual + ", not " + recorded,
      );
    }
  }
  return { ok: errors.length === 0, errors, live, execPreservesPid: Boolean(sessionShell && sessionShell.execPreservesPid) };
}

/** Remove a stale record so a previous run's identity can never be read as this run's. */
export function clearPtyIdentity(outputPath) {

/**
 * The survivor assertion: the original session's PTY child must still be the SAME process,
 * running the SAME executable, after an operation that must not replace it.
 *
 * This is the check that gives "the original session survived" its meaning. It compares the
 * live probe against the spawn-time record, so a replaced pane (a new PID) and a recycled PID
 * (same number, different executable) both fail. A record with no usable PID cannot assert
 * survival and says so rather than passing vacuously.
 */
export function ptyIdentitySurvived(record, probe) {
  if (!record || typeof record !== "object") {
    return { survived: false, reason: "no spawn-time PTY identity was recorded, so survival cannot be asserted" };
  }
  const ptyChild = record.ptyChild;
  if (!ptyChild || !Number.isInteger(ptyChild.pid) || ptyChild.pid <= 0) {
    return { survived: false, reason: "the spawn-time record carries no usable PTY child pid" };
  }
  if (typeof probe !== "function") {
    return { survived: false, reason: "no process probe was supplied, so survival cannot be asserted" };
  }
  const live = probe(ptyChild.pid);
  if (!live || !live.alive) {
    return { survived: false, reason: "the original PTY child pid " + ptyChild.pid + " is gone", pid: ptyChild.pid };
  }
  // Where the wrapper exec'd the shell the PID is preserved and the executable changed from
  // the wrapper to the shell; where it did not, the executable must still be the wrapper.
  const expectedExecutable = record.sessionShell && record.sessionShell.execPreservesPid === true
    ? record.sessionShell.executable
    : ptyChild.executable;
  const actual = String(live.executable || "");
  const base = String(expectedExecutable).split(/[\\/]/).pop();
  const matches = actual === expectedExecutable || (base.length > 0 && actual.endsWith("/" + base)) ||
    (base.length > 0 && actual.endsWith("\\" + base));
  if (!matches) {
    return {
      survived: false,
      reason: "pid " + ptyChild.pid + " is alive but runs " + actual + ", not " + expectedExecutable,
      pid: ptyChild.pid,
      expectedExecutable,
      actual,
    };
  }
  return {
    survived: true,
    pid: ptyChild.pid,
    executable: actual,
    expectedExecutable,
    execPreservesPid: Boolean(record.sessionShell && record.sessionShell.execPreservesPid),
  };
}

  for (const path of [outputPath, outputPath + ".tmp"]) {
    try {
      if (existsSync(path)) unlinkSync(path);
    } catch {
      /* Absent is the goal. */
    }
  }
}

/**
 * The identity the provisioner writes when a host supplies its own already-verified identity
 * (for example a paired or account-relay host that spawned the session itself). The caller is
 * responsible for having verified it; this function only persists the record.
 */
export function writePtyIdentity(outputPath, record) {
  const target = resolve(outputPath);
  mkdirSync(dirname(target), { recursive: true });
  const temp = target + ".tmp";
  writeFileSync(temp, JSON.stringify(record, null, 2));
  renameSync(temp, target);
  return target;
}

/**
 * A receipt block for a host-supplied identity: the PTY child and the session shell are the
 * same process and the same executable, so both roles are stated explicitly rather than
 * leaving the reader to infer that no wrapper exists on this path.
 */
/** The only accepted source kind for a session this run did not spawn itself. */
export const HOST_SUPPLIED_SOURCE_KIND = "owner-host-spawn-receipt";

/**
 * Validate the owner host's own spawn receipt.
 *
 * A PID that merely appears in a manifest is NOT ownership proof: it could be stale, could
 * belong to another session, or could have been typed by hand. So an externally supplied
 * identity is accepted ONLY when it is bound to a receipt the OWNING HOST produced, and that
 * receipt must agree with this run on the host, the transport, the session, the incarnation and
 * the candidate provenance. Anything absent or mismatched is rejected.
 */
export function validateOwnerHostSpawnReceipt(receipt, expected) {
  const errors = [];
  if (!receipt || typeof receipt !== "object") {
    return { ok: false, errors: ["no owner-host spawn receipt was supplied"] };
  }
  if (receipt.sourceKind !== HOST_SUPPLIED_SOURCE_KIND) {
    errors.push("owner-host spawn receipt sourceKind is " + String(receipt.sourceKind));
  }
  for (const field of ["hostId", "transport", "backendSessionId", "epoch", "pid", "executable", "spawnedAt"]) {
    const value = receipt[field];
    if (value === undefined || value === null || String(value).trim() === "") {
      errors.push("owner-host spawn receipt is missing " + field);
    }
  }
  if (!Number.isInteger(receipt.pid) || receipt.pid <= 0) {
    errors.push("owner-host spawn receipt carries no usable pid: " + String(receipt.pid));
  }
  if (!receipt.candidate || typeof receipt.candidate !== "object") {
    errors.push("owner-host spawn receipt carries no candidate provenance");
  } else {
    for (const field of ["candidateId", "sourceManifestSha256", "binarySha256"]) {
      if (!receipt.candidate[field]) errors.push("owner-host spawn receipt candidate is missing " + field);
    }
  }
  if (expected) {
    for (const [field, want] of [
      ["hostId", expected.hostId],
      ["transport", expected.transport],
      ["backendSessionId", expected.backendSessionId],
    ]) {
      if (want !== undefined && want !== null && receipt[field] !== want) {
        errors.push(
          "owner-host spawn receipt " + field + " is " + String(receipt[field]) + ", expected " + String(want),
        );
      }
    }
    if (expected.epoch !== undefined && expected.epoch !== null && String(receipt.epoch) !== String(expected.epoch)) {
      errors.push(
        "owner-host spawn receipt epoch is " + String(receipt.epoch) + ", expected " + String(expected.epoch),
      );
    }
    if (expected.candidateId && receipt.candidate && receipt.candidate.candidateId !== expected.candidateId) {
      errors.push("owner-host spawn receipt belongs to a different candidate");
    }
    if (
      expected.sourceManifestSha256 &&
      receipt.candidate &&
      receipt.candidate.sourceManifestSha256 !== expected.sourceManifestSha256
    ) {
      errors.push("owner-host spawn receipt belongs to a different candidate source manifest");
    }
  }
  return { ok: errors.length === 0, errors };
}

/**
 * Bind an externally spawned session to the owning host's own receipt.
 *
 * There is no path here that accepts a bare pid: the receipt is mandatory, it must validate,
 * and the roles it produces say plainly that the value came from the host rather than from a
 * wrapper this run installed. An absent or mismatched receipt is an error, never a stamped
 * identity.
 */
export function bindHostSuppliedIdentity(options) {
  const receipt = options.receipt;
  const validated = validateOwnerHostSpawnReceipt(receipt, {
    hostId: options.hostId,
    transport: options.transport,
    backendSessionId: options.backendSessionId,
    epoch: options.epoch,
    candidateId: options.candidateId,
    sourceManifestSha256: options.sourceManifestSha256,
  });
  if (!validated.ok) {
    throw new Error("owner-host spawn receipt rejected: " + validated.errors.join("; "));
  }
  const recordedBy = "owner-host-spawn-receipt:" + String(options.receiptPath || receipt.receiptPath || "unspecified");
  return {
    schema: PTY_IDENTITY_SCHEMA,
    platform: options.platform || receipt.platform || (process.platform === "win32" ? "windows" : "unix"),
    ptyChild: {
      pid: receipt.pid,
      executable: receipt.executable,
      role: "pty-child-wrapper",
      recordedBy,
      ownershipProof: "owner-host-spawn-receipt",
      note:
        "the owning host spawned this session; this run did not install a wrapper on that path, " +
        "so the PID is bound to the host's receipt rather than asserted by this run",
    },
    sessionShell: {
      pid: receipt.pid,
      pidKnown: true,
      executable: receipt.executable,
      role: "session-shell",
      recordedBy,
      execPreservesPid: false,
    },
    provider: options.providerCommand
      ? { executable: options.providerCommand, role: "provider-process", pid: null, pidKnown: false }
      : null,
    cols: options.cols === undefined ? null : options.cols,
    rows: options.rows === undefined ? null : options.rows,
    recordedAt: new Date().toISOString(),
  };
}
