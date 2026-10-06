/**
 * Frozen fixtures, schemas and shared helpers for the Herdr reference-chat acceptance
 * harness (plan task 14). Contract: docs/chat/herdr-port-contract.md (rev 2).
 *
 * AUTHORED, NOT EXECUTED. Every value here is read from an existing source in this
 * repository or from the frozen contract document; nothing is guessed, and a value the
 * harness cannot obtain is reported as missing identity instead of being defaulted.
 *
 * Provenance of the registry table below:
 *   ui/src/lib/agentTitle.ts      KNOWN_AGENT_MATCHERS  (23 classified agent types)
 *   ui/src/lib/agentResume.ts     RESUME_BUILDERS       (adds prime-agent, mimo-code, cursor-agent)
 *   ui/src/lib/agentsSettings.ts  AGENT_CANDIDATES      (launchable candidates)
 *   ui/src/lib/agentIcon.ts       AGENT_LOGO_ALIASES    (display aliases)
 *
 * Per-file id counts read from those files: agentTitle 23, agentResume 17, agentsSettings 10,
 * agentIcon 3 aliases. The union is the 28 ids below.
 *
 * The plan states a "24-row action matrix"; the source union is 28 ids over 24 rows because
 * antigravity/agy/antigravity-cli, mimo-code/mimo and cursor/cursor-agent are alias spellings
 * of one entry each (24 row ids + 4 aliases = 28). The plan says to reconcile its count
 * against the real ids rather than treat the number as proof, so the derivation is recorded
 * here and the reconciliation is a gate obligation.
 */

import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createConnection, createServer } from "node:net";
import { createInterface } from "node:readline";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

export const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

/* ==========================================================================
 * Schemas
 * ========================================================================== */

export const FIXTURE_SCHEMA = "ferryx-herdr-reference.fixtures/1";
export const CANDIDATE_SCHEMA = "ferryx-herdr-reference.candidate/1";
export const REPORT_SCHEMA = "ferryx-herdr-reference.report/1";
export const DEVICE_RECEIPT_SCHEMA = "ferryx-herdr-reference.device/1";
export const SPAWN_LEDGER_SCHEMA = "ferryx-herdr-reference.spawn-ledger/1";

/* ==========================================================================
 * Transports (plan QA-09 / IS-09)
 * ========================================================================== */

export const REFERENCE_TRANSPORTS = ["local", "ssh", "paired", "account-relay"];

/* ==========================================================================
 * Routes (task 13 registered them on the existing gateway; task 14 only consumes them)
 * ========================================================================== */

export const REFERENCE_CHAT_ROUTE_PREFIX = "/api/v1/reference-chat";

export const REFERENCE_ROUTES = [
  { method: "GET", suffix: "history", kind: "read" },
  { method: "GET", suffix: "screen", kind: "read" },
  { method: "GET", suffix: "prompt", kind: "read" },
  { method: "POST", suffix: "submit", kind: "mutation" },
  { method: "POST", suffix: "stop", kind: "mutation" },
  { method: "POST", suffix: "answer", kind: "mutation" },
  { method: "POST", suffix: "files", kind: "mutation" },
  { method: "GET", suffix: "files/{fileId}", kind: "read" },
  { method: "DELETE", suffix: "files/{fileId}", kind: "mutation" },
];

/**
 * The query fields the relay admits, mirrored from src-tauri/src/remote/relay_server.rs
 * validate_http_query. A runner that sends a field outside this table is refused 400 by the
 * relay before the machine sees it, so the harness builds queries from here.
 *
 * CORRECTED IDENTITY CONTRACT (task 12 handoff): a READ OMITS hostId and the gateway resolves
 * its own reference-chat host id. ownerId, epoch and backendSessionId stay required; epoch
 * must equal the gateway's own daemon epoch or the read is refused TARGET_EXPIRED. A MUTATION
 * carries the gateway's own referenceHostId from /api/v1/capabilities — never machineId.
 */
export const REFERENCE_READ_QUERY_FIELDS = {
  history: [
    "hostId",
    "ownerId",
    "epoch",
    "backendSessionId",
    "providerSessionId",
    "registryId",
    "limit",
    "cursor",
    "cursorStream",
  ],
  screen: [
    "hostId",
    "ownerId",
    "epoch",
    "backendSessionId",
    "providerSessionId",
    "registryId",
  ],
  prompt: [
    "hostId",
    "ownerId",
    "epoch",
    "backendSessionId",
    "providerSessionId",
    "registryId",
  ],
  file: [
    "hostId",
    "ownerId",
    "epoch",
    "backendSessionId",
    "providerSessionId",
    "registryId",
  ],
};

/** The history paging bounds, from src-tauri/src/remote/reference_chat/history.rs. */
export const REFERENCE_HISTORY_DEFAULT_LIMIT = 200;
export const REFERENCE_HISTORY_MAX_LIMIT = 1000;

/** The error code that means a mutation may have happened and cannot be confirmed. */
export const REFERENCE_OUTCOME_UNKNOWN_CODE = "OPERATION_OUTCOME_UNKNOWN";

/** The disclosure the reference shows for a pane with no native reader (contract section 4). */
export const REFERENCE_SCROLLBACK_DISCLOSURE =
  "Conversation unavailable — show terminal output";

/* ==========================================================================
 * Registry rows — derived from source, never from a count
 * ========================================================================== */

export const REGISTRY_SOURCE_FILES = [
  "ui/src/lib/agentTitle.ts",
  "ui/src/lib/agentResume.ts",
  "ui/src/lib/agentsSettings.ts",
  "ui/src/lib/agentIcon.ts",
];

/** Every agent id the four source files name, aliases included. */
export const REFERENCE_REGISTRY_IDS_FROM_SOURCE = [
  "antigravity",
  "omo",
  "gjc",
  "claude",
  "codex",
  "opencode",
  "omp",
  "pi",
  "aider",
  "cursor",
  "grok",
  "devin",
  "droid",
  "hermes",
  "kimi",
  "goose",
  "cline",
  "codebuff",
  "rovo",
  "openclaw",
  "copilot",
  "crush",
  "mimo",
  "prime-agent",
  "mimo-code",
  "cursor-agent",
  "agy",
  "antigravity-cli",
];

/**
 * The action matrix. One row per registry entry; aliases stay inside the row so the row
 * count and the id count cannot be confused.
 *
 * nativeReader / detector come from the frozen contract section 1 table; a row with
 * nativeReader "none" is a boundary (this agent has no reader today), not a denial.
 */
export const REFERENCE_REGISTRY_ROWS = [
  { id: "claude", aliases: [], nativeReader: "claude", detector: "claude" },
  { id: "codex", aliases: [], nativeReader: "codex", detector: "codex" },
  { id: "omo", aliases: [], nativeReader: "omo", detector: "omo" },
  { id: "omp", aliases: [], nativeReader: "omp", detector: "omp" },
  { id: "pi", aliases: [], nativeReader: "pi", detector: "pi" },
  { id: "gjc", aliases: [], nativeReader: "gjc", detector: "none" },
  { id: "opencode", aliases: [], nativeReader: "none", detector: "none" },
  { id: "prime-agent", aliases: [], nativeReader: "none", detector: "none" },
  { id: "antigravity", aliases: ["agy", "antigravity-cli"], nativeReader: "none", detector: "none" },
  { id: "mimo-code", aliases: ["mimo"], nativeReader: "none", detector: "none" },
  { id: "droid", aliases: [], nativeReader: "none", detector: "none" },
  { id: "grok", aliases: [], nativeReader: "none", detector: "none" },
  { id: "devin", aliases: [], nativeReader: "none", detector: "none" },
  { id: "kimi", aliases: [], nativeReader: "none", detector: "none" },
  { id: "copilot", aliases: [], nativeReader: "none", detector: "none" },
  { id: "cursor", aliases: ["cursor-agent"], nativeReader: "none", detector: "none" },
  { id: "aider", aliases: [], nativeReader: "none", detector: "none" },
  { id: "crush", aliases: [], nativeReader: "none", detector: "none" },
  { id: "cline", aliases: [], nativeReader: "none", detector: "none" },
  { id: "hermes", aliases: [], nativeReader: "none", detector: "none" },
  { id: "goose", aliases: [], nativeReader: "none", detector: "none" },
  { id: "codebuff", aliases: [], nativeReader: "none", detector: "none" },
  { id: "rovo", aliases: [], nativeReader: "none", detector: "none" },
  { id: "openclaw", aliases: [], nativeReader: "none", detector: "none" },
];

/** The six native readers the reference actually names (contract section 1). */
export const REFERENCE_NATIVE_READER_IDS = ["claude", "codex", "omp", "omo", "gjc", "pi"];

/** The five prompt detector families the reference actually names (contract section 1). */
export const REFERENCE_PROMPT_FAMILY_IDS = ["claude", "codex", "omp", "omo", "pi"];

/**
 * Every row id and alias must exist in the source union, and every source id must appear
 * in some row. A row invented from a count fails here instead of silently widening the
 * matrix, and a source id the matrix forgot fails too.
 */
export function validateRegistryRows() {
  const errors = [];
  const claimed = new Set();
  for (const row of REFERENCE_REGISTRY_ROWS) {
    if (!REFERENCE_REGISTRY_IDS_FROM_SOURCE.includes(row.id)) {
      errors.push("registry row id is not in the source union: " + row.id);
    }
    claimed.add(row.id);
    for (const alias of row.aliases) {
      if (!REFERENCE_REGISTRY_IDS_FROM_SOURCE.includes(alias)) {
        errors.push("registry alias is not in the source union: " + row.id + "/" + alias);
      }
      claimed.add(alias);
    }
    if (row.nativeReader !== "none" && !REFERENCE_NATIVE_READER_IDS.includes(row.nativeReader)) {
      errors.push("row names a reader the reference does not have: " + row.id);
    }
    if (row.detector !== "none" && !REFERENCE_PROMPT_FAMILY_IDS.includes(row.detector)) {
      errors.push("row names a detector the reference does not have: " + row.id);
    }
  }
  for (const id of REFERENCE_REGISTRY_IDS_FROM_SOURCE) {
    if (!claimed.has(id)) errors.push("source registry id is missing from the matrix: " + id);
  }
  return { ok: errors.length === 0, errors, rows: REFERENCE_REGISTRY_ROWS.length,
    ids: REFERENCE_REGISTRY_IDS_FROM_SOURCE.length };
}

/* ==========================================================================
 * Scenarios
 * ========================================================================== */

/**
 * QA-01..QA-11. Each row names its transport lane and the two branches the runner must
 * select; the runner refuses a scenario id or case that is not in this table.
 */
export const REFERENCE_QA_SCENARIOS = [
  {
    id: "QA-01",
    ideal: "IS-01",
    title: "chat is first, and an explicit terminal roundtrip preserves the pane",
    transport: "local",
    happy: "chat renders first at 360x800, 390x844 and 1280x800; chat -> terminal -> chat keeps backendSessionId, daemon epoch, providerSessionId and the original OS PID",
    failure: "a mode switch during composition or send neither duplicates a surface, hides a resize, starts a process nor loses the draft",
  },
  {
    id: "QA-02",
    ideal: "IS-02",
    title: "native history is the original pane's own conversation",
    transport: "local",
    happy: "each resolvable native source renders its own turns with a reader-identified source",
    failure: "a foreign host id is 403, a missing credential is 401/403, incomplete identity produces a refusal, and none of them substitutes another conversation or fabricates an assistant turn",
  },
  {
    id: "QA-03",
    ideal: "IS-03",
    title: "incremental history, cursors and generation fencing",
    transport: "local",
    happy: "append, older cursor, compaction and branch switch preserve order and viewport",
    failure: "a late foreign response, a foreign cursor, rotation/truncation and over-400 histories never cross-paint or duplicate a row",
  },
  {
    id: "QA-04",
    ideal: "IS-04",
    title: "original-PTY submit is one ordered transaction",
    transport: "local",
    happy: "single and multiline paste plus Enter reach the original TUI exactly once, with the reference byte shaping and a truthful receipt",
    failure: "disconnect before queue, disconnect after paste, a duplicate request id, an expired/revoked write and a Stop interleave never auto-retry and never claim providerRead",
  },
  {
    id: "QA-05",
    ideal: "IS-05",
    title: "prompt cards answer the current original screen",
    transport: "local",
    happy: "an approval, a question, a multi-select and a custom answer operate on the original screen and clear the card",
    failure: "a stale card, a replayed answer and a changed screen write zero bytes; a typed approval needs Confirm",
  },
  {
    id: "QA-06",
    ideal: "IS-06",
    title: "Stop is the reference Escape, never a kill",
    transport: "local",
    happy: "Escape stops a supported TUI turn, the same shell and provider PIDs survive and the next send works",
    failure: "a disconnected, read-only or unknown-capability target never becomes Ctrl-C, a kill or a restart, and the explicit terminal Ctrl-C stays separate",
  },
  {
    id: "QA-07",
    ideal: "IS-07",
    title: "attachments stay on the owning host",
    transport: "local",
    happy: "an image and a file stage on the owning host, the mention reaches the agent and the preview opens from the same owner",
    failure: "cancel, oversize, traversal, symlink, remote failure and revoke never produce a wrong-host path and never delete the draft",
  },
  {
    id: "QA-08",
    ideal: "IS-08",
    title: "real-device Korean composition and layout",
    transport: "device",
    happy: "provisioned physical Android and iOS devices compose Korean, scroll, resize and copy through native keyboards",
    failure: "Enter during composition, a mode or target switch and a late ACK never send prematurely, obscure a control or steal focus",
  },
  {
    id: "QA-09",
    ideal: "IS-09",
    title: "every transport preserves the original session",
    transport: "all",
    happy: "local, ssh, paired and account-relay sessions read, send and reconnect without replacing the process",
    failure: "a foreign epoch is 410 and a foreign owner is refused on every transport; disconnect, a grant revoke, relay loss and a desktop selection change stay fenced and never duplicate a retry or steal geometry",
  },
  {
    id: "QA-10",
    ideal: "IS-10",
    title: "every registry entry keeps its original pane",
    transport: "all",
    happy: "every matrix row enters chat and keeps its intentional PTY actions; the six readers and five detector families are exercised together",
    failure: "an alias or unknown source never claims an unsupported native parser and never spawns a Codex replacement",
  },
  {
    id: "QA-11",
    ideal: "IS-11",
    title: "frozen provenance and complete selection",
    transport: "all",
    happy: "frozen source, binary and UI hashes match every receipt and every scenario is selected",
    failure: "altering one artifact or hash, dropping a scenario or device receipt, leaking a fixture or fabricating a PASS makes the runner nonzero",
  },
];

export const REFERENCE_QA_IDS = REFERENCE_QA_SCENARIOS.map((row) => row.id);

export function scenarioById(id) {
  return REFERENCE_QA_SCENARIOS.find((row) => row.id === id) || null;
}

/* ==========================================================================
 * Small helpers
 * ========================================================================== */

export function assert(condition, message) {
  if (!condition) throw new Error(message);
}

export function sha256Bytes(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

export function sha256File(path) {
  return sha256Bytes(readFileSync(path));
}

export function sha256Text(text) {
  return sha256Bytes(Buffer.from(text, "utf8"));
}

export function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

export function writeJson(path, value) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify(value, null, 2));
}

export function writeReport(evidenceDir, value) {
  mkdirSync(evidenceDir, { recursive: true });
  const path = join(evidenceDir, "report.json");
  writeJson(path, value);
  return path;
}

/** A bounded wait. Never a sleep: the caller subscribes first, then awaits this. */
export function deadline(promise, label, ms) {
  const limit = typeof ms === "number" ? ms : 30000;
  let timer;
  return Promise.race([
    promise,
    new Promise((_resolve, reject) => {
      timer = setTimeout(() => reject(new Error(label + " timed out after " + limit + "ms")), limit);
    }),
  ]).finally(() => clearTimeout(timer));
}

export function countOccurrences(text, token) {
  if (!token) return 0;
  return text.split(token).length - 1;
}

/** A short, stable, non-secret label for evidence: never the raw credential. */
export function redactUrl(raw) {
  try {
    const url = new URL(raw);
    return url.origin + url.pathname;
  } catch {
    return "invalid-url";
  }
}

/* ==========================================================================
 * Provenance
 * ========================================================================== */

function git(root, args) {
  const result = spawnSync("git", args, { cwd: root, encoding: "utf8" });
  if (result.status !== 0) {
    return { ok: false, stdout: (result.stdout || "").trim(), stderr: (result.stderr || "").trim() };
  }
  return { ok: true, stdout: (result.stdout || "").trim(), stderr: "" };
}

/**
 * The candidate source provenance: the exact revision, the hash of every uncommitted
 * change (a dirty tree cannot be identified by HEAD alone), the tracked files the plan
 * names, and the tool versions the run used.
 */
export function captureSourceProvenance(root, sourcePaths, toolVersions) {
  const revision = git(root, ["rev-parse", "HEAD"]);
  const diff = spawnSync("git", ["diff", "HEAD", "--binary"], { cwd: root, encoding: "buffer" });
  const dirtyPatchSha256 = sha256Bytes(diff.stdout || Buffer.alloc(0));
  const status = git(root, ["status", "--porcelain=v1"]);
  const files = [];
  for (const relative of sourcePaths) {
    const absolute = resolve(root, relative);
    if (!existsSync(absolute)) {
      files.push({ path: relative, sha256: null, missing: true });
      continue;
    }
    files.push({ path: relative, sha256: sha256File(absolute) });
  }
  return {
    sourceRevision: revision.ok ? revision.stdout : null,
    revisionReadable: revision.ok,
    dirtyPatchSha256,
    dirty: (status.stdout || "").length > 0,
    dirtyStatusSha256: sha256Text(status.stdout || ""),
    sourceFiles: files,
    toolVersions: toolVersions || {},
  };
}

export function captureArtifact(path, label) {
  if (!existsSync(path)) return { path, label, sha256: null, missing: true };
  const stat = statSync(path);
  return {
    path,
    label,
    sha256: sha256File(path),
    sizeBytes: stat.size,
    mtimeMs: Math.round(stat.mtimeMs),
  };
}

/* ==========================================================================
 * Process ownership
 * ========================================================================== */

/**
 * The identity of a live process: its executable path and command line, read from the OS.
 * This is the identity check the teardown policy allows: a PID is killed only when its
 * live executable still matches the path recorded when it was spawned.
 */
export function probeProcessIdentity(pid) {
  if (process.platform === "win32") {
    const listing = spawnSync(
      "powershell",
      ["-NoProfile", "-Command", "(Get-Process -Id " + pid + " -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Path)"],
      { encoding: "utf8" },
    );
    if (listing.status !== 0) return { pid, alive: false, executable: null, commandLine: null };
    const executable = (listing.stdout || "").trim();
    if (!executable) return { pid, alive: false, executable: null, commandLine: null };
    return { pid, alive: true, executable, commandLine: executable };
  }
  const comm = spawnSync("ps", ["-p", String(pid), "-o", "comm="], { encoding: "utf8" });
  const args = spawnSync("ps", ["-p", String(pid), "-o", "args="], { encoding: "utf8" });
  const executable = (comm.stdout || "").trim();
  const commandLine = (args.stdout || "").trim();
  return { pid, alive: executable.length > 0, executable: executable || null, commandLine: commandLine || null };
}

/** The descendants of a PID, from the host process table. Used to find an owned PTY child. */
export function descendantPids(rootPid) {
  if (process.platform === "win32") {
    const listing = spawnSync(
      "powershell",
      ["-NoProfile", "-Command", "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,ExecutablePath | ConvertTo-Json -Compress"],
      { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 },
    );
    if (listing.status !== 0) return [];
    let rows;
    try {
      rows = JSON.parse(listing.stdout || "[]");
    } catch {
      return [];
    }
    if (!Array.isArray(rows)) rows = [rows];
    const byParent = new Map();
    for (const row of rows) {
      const parent = Number(row.ParentProcessId);
      if (!byParent.has(parent)) byParent.set(parent, []);
      byParent.get(parent).push({ pid: Number(row.ProcessId), executable: row.ExecutablePath || null });
    }
    return walkDescendants(byParent, rootPid);
  }
  const listing = spawnSync("ps", ["-Ao", "pid=,ppid=,comm="], { encoding: "utf8", maxBuffer: 32 * 1024 * 1024 });
  if (listing.status !== 0) return [];
  const byParent = new Map();
  for (const line of (listing.stdout || "").split("\n")) {
    const match = line.trim().match(/^(\d+)\s+(\d+)\s+(.*)$/);
    if (!match) continue;
    const parent = Number(match[2]);
    if (!byParent.has(parent)) byParent.set(parent, []);
    byParent.get(parent).push({ pid: Number(match[1]), executable: match[3] || null });
  }
  return walkDescendants(byParent, rootPid);
}

function walkDescendants(byParent, rootPid) {
  const found = [];
  const queue = [Number(rootPid)];
  const seen = new Set();
  while (queue.length > 0) {
    const parent = queue.shift();
    if (seen.has(parent)) continue;
    seen.add(parent);
    for (const child of byParent.get(parent) || []) {
      found.push(child);
      queue.push(child.pid);
    }
  }
  return found;
}

/**
 * The spawn ledger. Every runtime resource the harness starts is recorded here AT SPAWN
 * TIME with its exact PID and the executable path that was launched; teardown kills only
 * those exact PIDs after re-reading the live executable and confirming it still matches.
 * Anything else that looks related is reported, never killed.
 */
export class OwnedProcessLedger {
  constructor(role) {
    this.role = role;
    this.entries = [];
    this.receipts = [];
  }

  /** Record a spawned child process. */
  record(child, extra) {
    const entry = {
      pid: child.pid,
      executablePath: child.spawnfile || (extra && extra.executablePath) || null,
      argv: child.spawnargs || [],
      role: this.role,
      spawnedAt: new Date().toISOString(),
      extra: extra || {},
    };
    this.entries.push(entry);
    return entry;
  }

  /** Record a resource that is not a direct child but whose PID this harness created. */
  recordPid(pid, executablePath, extra) {
    const entry = {
      pid,
      executablePath,
      argv: [],
      role: this.role,
      spawnedAt: new Date().toISOString(),
      extra: extra || {},
    };
    this.entries.push(entry);
    return entry;
  }

  /**
   * Tear down every owned resource. A PID is killed only when it is still alive AND its
   * live executable matches the recorded one; a mismatch or an unreadable identity is
   * reported and left alone.
   */
  teardown(kill) {
    for (const entry of this.entries) {
      const live = probeProcessIdentity(entry.pid);
      if (!live.alive) {
        this.receipts.push({ pid: entry.pid, action: "already-exited", executablePath: entry.executablePath });
        continue;
      }
      const recorded = entry.executablePath;
      const matches = Boolean(recorded) && Boolean(live.executable) &&
        (live.executable === recorded ||
          live.executable.endsWith("/" + recorded.split("/").pop()) ||
          live.executable.endsWith("\\" + recorded.split("\\").pop()));
      if (!matches) {
        this.receipts.push({
          pid: entry.pid,
          action: "reported-not-killed",
          reason: "live executable does not match the spawn-recorded executable",
          recordedExecutablePath: recorded,
          liveExecutable: live.executable,
        });
        continue;
      }
      try {
        kill(entry.pid);
        this.receipts.push({ pid: entry.pid, action: "killed", executablePath: recorded });
      } catch (error) {
        this.receipts.push({ pid: entry.pid, action: "kill-failed", error: String(error) });
      }
    }
    return { schema: SPAWN_LEDGER_SCHEMA, role: this.role, entries: this.entries, receipts: this.receipts };
  }
}

/** Kill exactly one PID, after the caller has already confirmed its identity. */
export function killExactPid(pid) {
  if (process.platform === "win32") {
    const result = spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"], { encoding: "utf8" });
    if (result.status !== 0) throw new Error("taskkill failed for owned pid " + pid);
    return;
  }
  process.kill(pid, "SIGTERM");
}

/* ==========================================================================
 * Isolated profiles
 * ========================================================================== */

/**
 * A throwaway profile for one isolated gateway. Nothing here touches a production
 * profile: the data, runtime, session and HOME directories are all new and empty.
 */
export function setupIsolatedProfile(label) {
  const root = mkdtempSync(join(tmpdir(), "ferryx-herdr-ref-" + (label || "qa") + "-"));
  const paths = {};
  for (const name of ["data", "runtime", "session", "home", "evidence", "bin"]) {
    paths[name] = join(root, name);
    mkdirSync(paths[name]);
  }
  return {
    root,
    paths,
    env: {
      ...process.env,
      FERRYX_QA_ISOLATED: "1",
      FERRYX_DATA_DIR: paths.data,
      FERRYX_RUNTIME_DIR: paths.runtime,
      FERRYX_SESSION_DIR: paths.session,
      HOME: paths.home,
      USERPROFILE: paths.home,
    },
    cleanup() {
      rmSync(root, { recursive: true, force: true });
    },
  };
}

/** The daemon socket a profile owns, mirroring src-tauri/src/daemon/server.rs get_runtime_dir. */
export function daemonSocketPath(profile) {
  if (process.platform === "win32") {
    return join(profile.paths.runtime, "daemon.sock");
  }
  return join(profile.paths.runtime, "daemon.sock");
}

/* ==========================================================================
 * Isolated gateway launch (QA only)
 *
 * The product has exactly two launch modes - GUI (no arguments) and headless
 * (--daemon) - and NOTHING prints a JSON readiness line carrying a gateway URL and a
 * token. The launch contract implemented here is the product's own, each step read from
 * source:
 *
 *   1. --daemon is the only headless mode            cli.rs parse_launch_mode
 *      and it announces FERRYX_DAEMON_READY on stdout  cli.rs run_daemon_headless
 *   2. the daemon starts the gateway at boot from the
 *      persisted config <data>/remote/remote-config.json when mode != Off
 *                                                    daemon/server.rs boot restore
 *   3. the gateway ALWAYS binds a loopback listener; the
 *      non-loopback listener is gated behind
 *      FERRYX_ALLOW_INSECURE_DIRECT/_LAN, so leaving both
 *      unset keeps the isolated gateway on 127.0.0.1 only  remote/server.rs
 *   4. the loopback port is FIXED by the product: the
 *      daemon forces REMOTE_GATEWAY_PORT on config load
 *      and again on configure, so a persisted port is
 *      ignored and no per-instance port seam exists
 *                                      remote/state.rs, daemon/server.rs
 *   5. the bound address is read back from the daemon
 *      (RemoteGetStatus) and is the ONLY authority for
 *      the URL this harness dials                  daemon/protocol.rs
 *   6. the bearer token comes from the real pairing flow over the daemon's own control
 *      socket - RemoteCreatePairingCode (control) -> PIN, then
 *      POST /api/v1/pair/exchange -> device token. The auth store is this profile's;
 *      nothing is hand-written into it and no production credential is read.
 *
 * THE FIXED PORT IS A REQUIREMENT, NOT A CHOICE. Because the product pins it, this
 * gateway cannot run beside another listener on 127.0.0.1:REMOTE_GATEWAY_PORT, and the
 * harness must not manufacture that condition by stopping or reconfiguring whatever holds
 * the port. It is therefore pre-flighted BEFORE the spawn: an occupied port refuses the
 * launch as BLOCKED with the errno, and nothing is touched. A seeded port is never assumed
 * to take effect - it is written because the persisted shape carries one, while the URL
 * still comes from the daemon's own status. A dedicated free host is the supported way to
 * run this; the harness never widens the bind to find one.
 *
 * Readiness is event-driven and bounded: the daemon's own ready line, then one status
 * read. There is no fixed sleep and no retry poll anywhere on this path.
 * ========================================================================== */

/** The daemon's own headless readiness line (src-tauri/src/cli.rs). */
export const DAEMON_READY_LINE = "FERRYX_DAEMON_READY";

/** The daemon control protocol this harness speaks (src-tauri/src/daemon/protocol.rs). */
export const DAEMON_CONTROL_PROTOCOL_VERSION = 5;

/**
 * The gateway port the product FORCES (src-tauri/src/remote/state.rs REMOTE_GATEWAY_PORT,
 * applied when the persisted config is loaded and again by daemon/server.rs
 * handle_remote_configure). A persisted port is therefore ignored and there is no
 * port-isolation seam to configure around, which makes a free
 * 127.0.0.1:<this port> a REQUIREMENT of the isolated launch - pre-flighted below, and the
 * reason this harness runs on a dedicated host. It never builds the URL: that comes from
 * the bound address the daemon itself reports.
 */
export const REMOTE_GATEWAY_PORT = 43821;

/** The contract id every isolated-gateway handle reports. */
export const ISOLATED_GATEWAY_CONTRACT = "ferryx-herdr-reference.isolated-gateway/1";

/** The file inside the isolated profile that holds the device token this run obtained. */
export const ISOLATED_CREDENTIAL_FILENAME = "reference-chat-token";

const GATEWAY_PAIR_EXCHANGE_PATH = "/api/v1/pair/exchange";

/**
 * The variables that widen the gateway's bind beyond loopback. An isolated QA gateway
 * must never be reachable from a shared host's network, so the launcher refuses to start
 * when the environment it inherits would open it.
 */
const LAN_EXPOSURE_ENV = ["FERRYX_ALLOW_INSECURE_DIRECT", "FERRYX_ALLOW_INSECURE_LAN"];

/** A launch-contract failure. The caller reports it as BLOCKED, never as a pass. */
export class IsolatedGatewayError extends Error {
  constructor(reason, detail) {
    super(reason + (detail ? ": " + detail : ""));
    this.name = "IsolatedGatewayError";
    this.reason = reason;
    this.detail = detail || "";
  }
}

const gatewayBlocked = (reason, detail) => new IsolatedGatewayError(reason, detail);

/**
 * Seed the persisted remote gateway config the daemon restores at boot.
 *
 * Shape is PersistedRemoteGatewayConfig (remote/state.rs, camelCase). mode must be
 * non-Off or the daemon leaves the gateway off. `port` is written for shape completeness
 * and is NOT authoritative: the product overrides it with REMOTE_GATEWAY_PORT, so nothing
 * downstream may assume a seeded port took effect. localNetwork is the one non-Off mode that
 * can yield a loopback-only gateway: it resolves the host's LAN address and then refuses
 * to bind it because the insecure-direct gate is closed, while the always-on loopback
 * listener keeps serving. On a host with no non-loopback IPv4 the resolution itself
 * fails, the daemon logs a warning, and the status read below reports it.
 */
export function seedIsolatedRemoteConfig(profile) {
  const path = join(profile.paths.data, "remote", "remote-config.json");
  writeJson(path, {
    mode: "localNetwork",
    port: REMOTE_GATEWAY_PORT,
    allowControl: true,
    restartPolicy: "restoreListener",
    relayUrl: null,
  });
  return path;
}

/**
 * Pre-flight: is the port the product forces free on loopback?
 *
 * The probe only OBSERVES. A port held by anything else is a refusal, never something to
 * free up: stopping or reconfiguring an unrelated listener would be an unowned action on
 * a host this harness does not own, and an existing Ferryx service may be exactly what is
 * holding it.
 *
 * It is a guard, not a reservation: the socket is released before the daemon is spawned, so
 * a listener that appears in that window still wins the bind, and the launch then reports
 * gateway-not-running from the daemon's own status instead of silently sharing a port.
 */
function probeLoopbackPort(port) {
  return new Promise((resolveProbe) => {
    const server = createServer();
    server.once("error", (error) => {
      resolveProbe({ free: false, code: error && error.code ? error.code : null, reason: String(error) });
    });
    server.listen({ host: "127.0.0.1", port, exclusive: true }, () => {
      server.close(() => resolveProbe({ free: true, code: null, reason: null }));
    });
  });
}

/** The loopback URL the daemon reports it bound, or null for anything else. */
export function parseLoopbackBoundAddress(boundAddress) {
  if (typeof boundAddress !== "string") return null;
  const match = boundAddress.trim().match(/^(\[[0-9a-fA-F:]+\]|[0-9.]+):(\d{1,5})$/);
  if (!match) return null;
  const host = match[1].replace(/^\[/, "").replace(/\]$/, "");
  const port = Number.parseInt(match[2], 10);
  if (!Number.isInteger(port) || port <= 0 || port > 65535) return null;
  if (host !== "::1" && host !== "localhost" && !host.startsWith("127.")) return null;
  return { host: "127.0.0.1", port };
}

/**
 * One connection to a daemon control socket (newline-delimited JSON, protocol v5). On
 * unix the socket's ownership and mode are the authentication, so no token is sent.
 * A socket that closes with requests in flight resolves them as null, which every caller
 * already treats as a protocol failure.
 */
export function connectDaemonControl(socketPath, options) {
  const settings = options || {};
  const timeoutMs = typeof settings.timeoutMs === "number" ? settings.timeoutMs : 30000;
  return deadline(
    new Promise((resolveConnect, rejectConnect) => {
      const socket = createConnection({ path: socketPath });
      const lines = createInterface({ input: socket });
      const pending = [];
      lines.on("line", (line) => {
        let message;
        try {
          message = JSON.parse(line);
        } catch {
          return;
        }
        const next = pending.shift();
        if (next) next(message);
      });
      socket.once("error", (error) => {
        rejectConnect(gatewayBlocked("daemon-socket-error", socketPath + " " + String(error)));
      });
      socket.once("close", () => {
        while (pending.length > 0) pending.shift()(null);
      });
      const call = (payload) =>
        new Promise((resolveCall) => {
          pending.push(resolveCall);
          socket.write(JSON.stringify(payload) + "\n");
        });
      socket.once("connect", async () => {
        try {
          const handshake = await call({
            type: "handshake",
            version: DAEMON_CONTROL_PROTOCOL_VERSION,
            ...(settings.token ? { token: settings.token } : {}),
          });
          if (!handshake || handshake.type !== "handshakeOk") {
            socket.destroy();
            rejectConnect(gatewayBlocked("daemon-handshake-refused", socketPath + " " + JSON.stringify(handshake)));
            return;
          }
          resolveConnect({
            handshake,
            call,
            close() {
              lines.close();
              socket.destroy();
            },
          });
        } catch (error) {
          socket.destroy();
          rejectConnect(error);
        }
      });
    }),
    "daemon control handshake",
    timeoutMs,
  );
}

/** One bounded JSON request against the gateway, with the failure kept verbatim. */
async function gatewayJson(url, options, timeoutMs) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(url, { ...options, signal: controller.signal });
    const text = await response.text();
    let json = null;
    try {
      json = text.length > 0 ? JSON.parse(text) : null;
    } catch {
      json = null;
    }
    return { status: response.status, json, text };
  } catch (error) {
    throw gatewayBlocked("gateway-request-failed", url + " " + String(error));
  } finally {
    clearTimeout(timer);
  }
}

/**
 * Launch one isolated, authenticated gateway on a throwaway profile.
 *
 * options: { binary, label, uiDist, env, cwd, ledger, timeoutMs, extra }. The returned
 * handle's url and token come from the running daemon and its own auth store; stop()
 * shuts that daemon down in band and falls back to the exact PID this run spawned. Every
 * failure is an IsolatedGatewayError for the caller to report as BLOCKED.
 */
export async function launchIsolatedGateway(options) {
  const settings = options || {};
  const timeoutMs = typeof settings.timeoutMs === "number" ? settings.timeoutMs : 30000;
  const binary = settings.binary ? resolve(settings.binary) : null;
  if (!binary || !existsSync(binary)) {
    throw gatewayBlocked("gateway-binary-missing", String(settings.binary || ""));
  }

  const profile = setupIsolatedProfile(settings.label || "gateway");
  const configPath = seedIsolatedRemoteConfig(profile);
  const socketPath = daemonSocketPath(profile);
  const env = { ...profile.env };
  if (settings.uiDist) env.FERRYX_UI_DIST_DIR = resolve(settings.uiDist);
  for (const [key, value] of Object.entries(settings.env || {})) env[key] = String(value);
  for (const name of LAN_EXPOSURE_ENV) {
    const value = env[name];
    if (typeof value === "string" && value.trim().length > 0 && value.trim() !== "0") {
      throw gatewayBlocked(
        "isolated-gateway-lan-exposure",
        name + "=" + value.trim() + " would bind the isolated gateway beyond loopback",
      );
    }
  }

  // The product forces the port, so this launch REQUIRES 127.0.0.1:REMOTE_GATEWAY_PORT to be
  // free. It is checked here, before anything is spawned, and an occupied port refuses the
  // launch without touching whatever holds it.
  const portProbe = await probeLoopbackPort(REMOTE_GATEWAY_PORT);
  if (!portProbe.free) {
    throw gatewayBlocked(
      portProbe.code === "EADDRINUSE" ? "gateway-port-occupied" : "gateway-port-unusable",
      "127.0.0.1:" + REMOTE_GATEWAY_PORT + " is not free (" + (portProbe.code || portProbe.reason) +
        "); the product forces that port, so no isolated gateway can start here. Nothing was " +
        "stopped, reconfigured or reused; run this on a dedicated free host.",
    );
  }

  // The seeded port is neither assumed to take effect nor used to build the URL: the bound
  // address the daemon reports below is the only authority. A bind that still fails surfaces
  // as gateway-not-running, carrying that daemon's own status and stderr tail rather than a
  // guess.

  const child = spawn(binary, ["--daemon"], {
    cwd: settings.cwd || repoRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  const entry = settings.ledger
    ? settings.ledger.record(child, { ...(settings.extra || {}), launchArgs: ["--daemon"], remoteConfig: configPath })
    : { pid: child.pid, executablePath: child.spawnfile, argv: child.spawnargs };

  let stderr = "";
  child.stderr.on("data", (data) => {
    stderr = (stderr + data.toString()).slice(-16000);
  });
  const exited = new Promise((resolveExit) => {
    child.once("exit", (code, signal) => resolveExit({ code, signal }));
  });
  // The daemon's own readiness line, buffered rather than sampled: the line is checked on
  // arrival and once on attach, so a line that lands before the listener attaches is not
  // missed. No sleep and no retry.
  let stdout = "";
  const ready = new Promise((resolveReady) => {
    const inspect = () => {
      if (!stdout.includes(DAEMON_READY_LINE)) return;
      child.stdout.off("data", onData);
      resolveReady(true);
    };
    const onData = (data) => {
      stdout = (stdout + data.toString()).slice(-16000);
      inspect();
    };
    child.stdout.on("data", onData);
    inspect();
  });

  let control = null;
  const stopOwnedDaemon = async () => {
    if (child.exitCode !== null || child.signalCode !== null) {
      return { stopped: true, graceful: true, alreadyExited: true, pid: child.pid, executablePath: entry.executablePath };
    }
    if (control) {
      // In-band shutdown: the daemon persists remote sessions and exits 0
      // (daemon/server.rs Shutdown). Its reply is not awaited - that handler exits the
      // process - so the exit event is the receipt.
      control.call({ type: "shutdown" }).catch(() => null);
      try {
        const result = await deadline(exited, "isolated gateway shutdown", timeoutMs);
        return { stopped: true, graceful: true, pid: child.pid, executablePath: entry.executablePath, exit: result };
      } catch {
        /* The daemon did not stop in band; fall through to the exact-PID fallback. */
      }
    }
    // The exact PID this run spawned, re-identified before it is signalled: a PID whose
    // live executable no longer matches the spawn-recorded one is reported, never killed.
    const live = probeProcessIdentity(child.pid);
    const recorded = entry.executablePath;
    const recordedName = recorded ? basename(recorded) : null;
    const identityMatches = Boolean(
      live.alive && live.executable && recorded &&
        (live.executable === recorded ||
          live.executable.endsWith("/" + recordedName) ||
          live.executable.endsWith("\\" + recordedName)),
    );
    if (!identityMatches) {
      return {
        stopped: false,
        graceful: false,
        killed: false,
        killSkipped: "live executable does not match the spawn-recorded executable",
        pid: child.pid,
        executablePath: recorded,
        live,
      };
    }
    let killed = false;
    let killError = null;
    try {
      killExactPid(child.pid);
      killed = true;
    } catch (error) {
      killError = String(error);
    }
    let exit = null;
    try {
      exit = await deadline(exited, "isolated gateway termination", 10000);
    } catch {
      exit = null;
    }
    return {
      stopped: exit !== null,
      graceful: false,
      killed,
      killError,
      pid: child.pid,
      executablePath: entry.executablePath,
      exit,
    };
  };

  try {
    await deadline(
      new Promise((resolveReady, rejectReady) => {
        ready.then(() => resolveReady(true));
        exited.then((result) =>
          rejectReady(gatewayBlocked("gateway-exited-before-ready", JSON.stringify(result) + " " + stderr.slice(-2000))),
        );
      }),
      "isolated gateway readiness",
      timeoutMs,
    );

    // The accept loop starts only after the boot restore of the persisted remote config,
    // so a completed handshake already implies the gateway restore has run; the single
    // status read below is therefore authoritative and needs no polling.
    control = await connectDaemonControl(socketPath, { timeoutMs });
    const statusResponse = await control.call({ type: "remoteGetStatus" });
    if (!statusResponse || statusResponse.type !== "remoteStatusOk") {
      throw gatewayBlocked("gateway-status-unreadable", JSON.stringify(statusResponse));
    }
    const status = statusResponse.status || null;
    if (!status || status.isRunning !== true) {
      throw gatewayBlocked("gateway-not-running", JSON.stringify(status) + " " + stderr.slice(-2000));
    }
    const bound = parseLoopbackBoundAddress(status.boundAddress);
    if (!bound) {
      throw gatewayBlocked("gateway-bound-address-not-loopback", String(status.boundAddress));
    }
    const url = "http://" + bound.host + ":" + bound.port;

    // The real pairing flow, over the daemon's own control socket and the gateway's own
    // exchange route: nothing is hand-written into the auth store.
    const pinResponse = await control.call({ type: "remoteCreatePairingCode", permission: "control" });
    if (!pinResponse || pinResponse.type !== "remotePairingCodeOk" || !/^\d{6}$/.test(String(pinResponse.code || ""))) {
      throw gatewayBlocked("pairing-code-unavailable", JSON.stringify(pinResponse));
    }
    const exchanged = await gatewayJson(
      url + GATEWAY_PAIR_EXCHANGE_PATH,
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          code: String(pinResponse.code),
          deviceName: "herdr-reference-qa",
          installationId: "herdr-reference-" + basename(profile.root),
        }),
      },
      timeoutMs,
    );
    if (exchanged.status !== 200 || !exchanged.json || typeof exchanged.json.token !== "string" || exchanged.json.token.length === 0) {
      throw gatewayBlocked(
        "pairing-exchange-refused",
        "POST " + GATEWAY_PAIR_EXCHANGE_PATH + " -> " + exchanged.status + " " + String(exchanged.text).slice(0, 500),
      );
    }
    const token = exchanged.json.token;
    const credentialFile = join(profile.paths.data, ISOLATED_CREDENTIAL_FILENAME);
    writeFileSync(credentialFile, token + "\n", { mode: 0o600 });

    // Fail closed if the URL derived above does not belong to the daemon this run started:
    // a token from this profile's store must authenticate there, or the run must not
    // proceed with an endpoint someone else owns.
    const capabilities = await gatewayJson(
      url + REFERENCE_CAPABILITIES_PATH,
      { headers: { accept: "application/json", authorization: "Bearer " + token } },
      timeoutMs,
    );
    if (capabilities.status !== 200) {
      throw gatewayBlocked(
        "gateway-token-refused",
        "GET " + REFERENCE_CAPABILITIES_PATH + " -> " + capabilities.status + " " + String(capabilities.text).slice(0, 500),
      );
    }

    return {
      contract: ISOLATED_GATEWAY_CONTRACT,
      profile,
      entry,
      url,
      port: bound.port,
      boundAddress: status.boundAddress,
      pinnedPort: REMOTE_GATEWAY_PORT,
      portRequirement: {
        fixed: true,
        port: REMOTE_GATEWAY_PORT,
        authority: "remote/state.rs REMOTE_GATEWAY_PORT, forced on config load and on configure",
        urlAuthority: "the daemon's own RemoteGetStatus boundAddress",
      },
      token,
      credentialFile,
      daemonSocketPath: socketPath,
      daemonPid: typeof control.handshake.pid === "number" ? control.handshake.pid : null,
      daemonEpoch:
        control.handshake.epoch === undefined || control.handshake.epoch === null
          ? null
          : String(control.handshake.epoch),
      deviceId: exchanged.json.device && exchanged.json.device.id ? exchanged.json.device.id : null,
      devicePermission:
        exchanged.json.device && exchanged.json.device.permission ? exchanged.json.device.permission : null,
      machineId: exchanged.json.machineId || null,
      referenceHostId:
        capabilities.json && capabilities.json.referenceHostId ? capabilities.json.referenceHostId : null,
      remoteStatus: status,
      stderrTail: () => stderr,
      stop: stopOwnedDaemon,
    };
  } catch (error) {
    // A gateway this run started is never left running behind a failure, and its throwaway
    // profile is removed only once the process is gone.
    const outcome = await stopOwnedDaemon().catch(() => ({ stopped: false }));
    if (control) {
      try {
        control.close();
      } catch {
        /* The socket dies with the daemon. */
      }
    }
    if (outcome.stopped) {
      try {
        profile.cleanup();
      } catch {
        /* A leftover temp profile is reported by the ledger, not hidden. */
      }
    } else {
      error.detail = (error.detail ? error.detail + " " : "") + "profile-left-behind=" + profile.root + " pid=" + child.pid;
    }
    throw error;
  }
}

/* ==========================================================================
 * Manifest validation
 * ========================================================================== */

function isNonEmptyString(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function requireFields(errors, owner, value, fields) {
  for (const field of fields) {
    if (!isNonEmptyString(value[field])) errors.push(owner + " is missing " + field);
  }
}

/**
 * Validate the fixture manifest the provisioner wrote. The runner validates rather than
 * guessing defaults: a missing host, session, device or ledger row is an error the caller
 * reports, never a value the runner invents.
 */
export function validateFixtureManifest(value) {
  const errors = [];
  if (!value || typeof value !== "object") {
    return { ok: false, errors: ["fixture manifest is not an object"], hosts: [], sessions: [], devices: [] };
  }
  if (value.schema !== FIXTURE_SCHEMA) errors.push("fixture manifest schema is " + String(value.schema));

  const hosts = Array.isArray(value.hosts) ? value.hosts : [];
  if (hosts.length === 0) errors.push("fixture manifest names no host");
  for (const host of hosts) {
    requireFields(errors, "host", host, ["id", "transport", "url", "credentialFile"]);
    if (!REFERENCE_TRANSPORTS.includes(host.transport)) {
      errors.push("host " + host.id + " has an unknown transport: " + String(host.transport));
    }
    if (isNonEmptyString(host.credentialFile) && isAbsolute(host.credentialFile) && !existsSync(host.credentialFile)) {
      errors.push("host " + host.id + " credential file does not exist: " + host.credentialFile);
    }
  }

  const sessions = Array.isArray(value.sessions) ? value.sessions : [];
  if (sessions.length === 0) errors.push("fixture manifest names no session");
  const hostIds = new Set(hosts.map((host) => host.id));
  for (const session of sessions) {
    // The objective names this field `nativeSessionId`; the wire target calls the same
    // identity `providerSessionId`. Both spellings are accepted and normalized to both, so a
    // manifest written from the plan's prose and one written from the contract doc validate
    // identically. Neither spelling is ever defaulted: absent means *unknown*.
    if (isNonEmptyString(session.nativeSessionId) && !isNonEmptyString(session.providerSessionId)) {
      session.providerSessionId = session.nativeSessionId;
    }
    if (isNonEmptyString(session.providerSessionId) && !isNonEmptyString(session.nativeSessionId)) {
      session.nativeSessionId = session.providerSessionId;
    }
    requireFields(errors, "session", session, [
      "hostId",
      "backendSessionId",
      "provider",
      "pid",
      "executablePath",
    ]);
    if (!hostIds.has(session.hostId)) {
      errors.push("session " + session.backendSessionId + " names an unknown host " + session.hostId);
    }
    if (isNonEmptyString(session.transcriptPath) && !existsSync(session.transcriptPath)) {
      errors.push("session " + session.backendSessionId + " transcript path does not exist");
    }
    if (isNonEmptyString(session.transcriptPath) && !isNonEmptyString(session.transcriptSha256)) {
      errors.push("session " + session.backendSessionId + " has a transcript path with no raw hash");
    }
  }

  const devices = Array.isArray(value.devices) ? value.devices : [];
  for (const device of devices) {
    requireFields(errors, "device", device, ["platform", "serial", "driverEndpoint"]);
    if (!["android", "ios"].includes(device.platform)) {
      errors.push("device has an unknown platform: " + String(device.platform));
    }
  }

  const spawnLedger = value.spawnLedger;
  if (!spawnLedger || typeof spawnLedger !== "object") {
    errors.push("fixture manifest has no spawn ledger");
  } else if (spawnLedger.schema !== SPAWN_LEDGER_SCHEMA) {
    errors.push("spawn ledger schema is " + String(spawnLedger.schema));
  } else if (!Array.isArray(spawnLedger.entries)) {
    errors.push("spawn ledger has no entries array");
  }

  return { ok: errors.length === 0, errors, hosts, sessions, devices, spawnLedger: spawnLedger || null };
}

export function validateCandidateManifest(value) {
  const errors = [];
  if (!value || typeof value !== "object") {
    return { ok: false, errors: ["candidate manifest is not an object"] };
  }
  if (value.schema !== CANDIDATE_SCHEMA) errors.push("candidate manifest schema is " + String(value.schema));
  if (!isNonEmptyString(value.candidateId)) errors.push("candidate manifest has no candidateId");
  if (!isNonEmptyString(value.sourceRevision)) errors.push("candidate manifest has no sourceRevision");
  if (!isNonEmptyString(value.dirtyPatchSha256)) errors.push("candidate manifest has no dirtyPatchSha256");
  if (!Array.isArray(value.sourceFiles) || value.sourceFiles.length === 0) {
    errors.push("candidate manifest has no sourceFiles");
  } else {
    for (const file of value.sourceFiles) {
      if (!isNonEmptyString(file.path)) errors.push("candidate source file has no path");
      if (!isNonEmptyString(file.sha256)) errors.push("candidate source file has no hash: " + String(file.path));
    }
  }
  if (!value.binary || !isNonEmptyString(value.binary.path) || !isNonEmptyString(value.binary.sha256)) {
    errors.push("candidate manifest has no hashed binary");
  }
  if (!Array.isArray(value.uiFiles) || value.uiFiles.length === 0) {
    errors.push("candidate manifest has no uiFiles");
  } else {
    for (const file of value.uiFiles) {
      if (!isNonEmptyString(file.path)) errors.push("candidate UI file has no path");
      if (!isNonEmptyString(file.sha256)) errors.push("candidate UI file has no hash: " + String(file.path));
    }
  }
  if (value.buildExit !== 0) errors.push("candidate buildExit is " + String(value.buildExit));
  if (!isNonEmptyString(value.buildCommand)) errors.push("candidate manifest has no buildCommand");
  if (!value.toolVersions || typeof value.toolVersions !== "object") {
    errors.push("candidate manifest has no toolVersions");
  }
  return { ok: errors.length === 0, errors };
}

/**
 * Validate one native-device receipt (task 15 producer output; QA-08 consumes it here).
 *
 * The receipt binds the candidate, the served page, the authoritative target tuple and the
 * original PTY. A missing device or driver is a BLOCKED receipt, never a PASS.
 */
export function validateDeviceReceipt(value, options) {
  const platform = options && options.platform;
  const errors = [];
  if (!value || typeof value !== "object") {
    return { ok: false, errors: ["device receipt is not an object"], status: "BLOCKED" };
  }
  if (value.schema !== DEVICE_RECEIPT_SCHEMA) errors.push("device receipt schema is " + String(value.schema));
  if (!["android", "ios"].includes(value.platform)) errors.push("device receipt platform is " + String(value.platform));
  if (platform && value.platform !== platform) {
    errors.push("device receipt platform " + String(value.platform) + " is not the requested " + platform);
  }
  if (value.verdict === "blocked") {
    return { ok: true, status: "BLOCKED", errors, reason: value.blockedReason || "device receipt reports blocked" };
  }
  for (const [owner, fields] of [
    ["device", ["serial", "model", "osVersion", "driverEndpoint"]],
    ["candidate", ["candidateId", "sourceManifestSha256", "binarySha256"]],
    ["page", ["origin", "urlRedacted"]],
    ["target", ["hostId", "ownerId", "epoch", "backendSessionId", "registryId"]],
    ["pty", ["pid", "executablePath", "cols", "rows"]],
  ]) {
    if (!value[owner] || typeof value[owner] !== "object") {
      errors.push("device receipt has no " + owner + " block");
      continue;
    }
    requireFields(errors, "device receipt " + owner, value[owner], fields);
  }
  const scenarios = Array.isArray(value.scenarios) ? value.scenarios : [];
  if (scenarios.length === 0) errors.push("device receipt has no scenarios");
  for (const scenario of scenarios) {
    if (!isNonEmptyString(scenario.name)) errors.push("device receipt scenario has no name");
    if (!["pass", "fail", "blocked"].includes(scenario.status)) {
      errors.push("device receipt scenario " + String(scenario.name) + " has status " + String(scenario.status));
    }
    if (scenario.status === "pass" && (!scenario.captured || typeof scenario.captured !== "object")) {
      errors.push("device receipt scenario " + String(scenario.name) + " passes with no capture");
    }
  }
  const cleanup = value.cleanup;
  if (!cleanup || !Array.isArray(cleanup.killed)) {
    errors.push("device receipt has no spawn-recorded cleanup ledger");
  }
  if (value.verdict !== "pass") errors.push("device receipt verdict is " + String(value.verdict));
  return { ok: errors.length === 0, status: errors.length === 0 ? "PASS" : "FAIL", errors };
}

/**
 * The identity fields a session record is missing. The harness reports these instead of
 * substituting a convenient fixture: an absent providerSessionId means "unknown", never
 * permission to read another session's file.
 */
export function sessionMissingIdentity(session) {
  const missing = [];
  for (const field of ["hostId", "ownerId", "epoch", "backendSessionId", "registryId", "pid", "executablePath", "cols", "rows"]) {
    if (!isNonEmptyString(String(session[field] === undefined || session[field] === null ? "" : session[field]))) {
      missing.push(field);
    }
  }
  if (!isNonEmptyString(session.providerSessionId)) missing.push("providerSessionId");
  return missing;
}

export function hostById(manifest, hostId) {
  return (manifest.hosts || []).find((host) => host.id === hostId) || null;
}

/* ==========================================================================
 * Gateway capabilities (the authoritative identity source)
 * ==========================================================================
 *
 * The corrected contract makes three values authoritative HERE rather than in a fixture:
 *   daemonEpoch       the incarnation every read and mutation must name
 *   referenceHostId   the host id a MUTATION target must carry (NOT machineId)
 *   machineId         the machine identity; NOT a reference-chat host id
 *
 * A gateway that does not publish referenceHostId yet is reported as such. This module never
 * substitutes machineId for it.
 */

export const REFERENCE_CAPABILITIES_PATH = "/api/v1/capabilities";

/** Read the gateway's own capabilities. */
export async function readGatewayCapabilities(host) {
  const token = readCredentialToken(host);
  const base = String(host.url).replace(/\/+$/, "");
  const headers = { accept: "application/json" };
  if (token) headers.authorization = "Bearer " + token;
  const response = await fetch(base + REFERENCE_CAPABILITIES_PATH, { headers });
  const text = await response.text();
  let json = null;
  try {
    json = text.length > 0 ? JSON.parse(text) : null;
  } catch {
    json = null;
  }
  return { status: response.status, json, text };
}

/**
 * Validate the capabilities the reference-chat contract depends on.
 *
 * \`referenceHostId\` is an authoring prerequisite owned by the backend task; until it ships,
 * every mutation is BLOCKED here rather than being sent with a machineId that the gateway
 * would refuse.
 */
export function validateReferenceCapabilities(capabilities) {
  const errors = [];
  const missing = [];
  if (!capabilities || typeof capabilities !== "object") {
    return { ok: false, errors: ["capabilities are not an object"], missing: ["daemonEpoch", "referenceHostId"] };
  }
  if (!isNonEmptyString(capabilities.daemonEpoch)) missing.push("daemonEpoch");
  if (!isNonEmptyString(capabilities.referenceHostId)) missing.push("referenceHostId");
  if (missing.length > 0) {
    errors.push(
      "the gateway does not publish: " + missing.join(", ") +
        " (referenceHostId is the backend authoring prerequisite for mutations)",
    );
  }
  return { ok: errors.length === 0, errors, missing, machineId: capabilities.machineId || null };
}

/**
 * The ambiguity guard the contract requires: when identity is absent or ambiguous the reader
 * must NOT fall back to the newest file or to a same-cwd peer. This helper is what a branch
 * asserts against — it returns the refusal reason a conforming reader must produce.
 */
export function referenceIdentityRefusal(session) {
  const missing = sessionMissingIdentity(session);
  if (missing.length === 0) return null;
  return (
    "identity is incomplete for " + String(session.backendSessionId) + ": missing " +
    missing.join(", ") + "; a conforming reader refuses rather than reading the newest or a " +
    "same-cwd conversation"
  );
}


export function sessionById(manifest, sessionId) {
  return (manifest.sessions || []).find((session) => session.backendSessionId === sessionId) || null;
}

export function sessionsForHost(manifest, hostId) {
  return (manifest.sessions || []).filter((session) => session.hostId === hostId);
}

/**
 * The authoritative target tuple a MUTATION binds.
 *
 * The corrected identity contract: the host id a mutation carries is the gateway's OWN
 * `referenceHostId`, published in `/api/v1/capabilities` — NOT the `machineId`. The two are
 * different values and the gateway refuses a foreign one with 403, so a machineId here would
 * produce a permanent, confusing refusal rather than a pass.
 *
 * This fails closed: a session with no resolved referenceHostId cannot build a target at all,
 * which is the correct outcome when the gateway does not publish it yet.
 */
export function targetFor(session) {
  const hostId = session.referenceHostId;
  if (!isNonEmptyString(hostId)) {
    throw new Error(
      "a mutation target needs the gateway's own referenceHostId from /api/v1/capabilities; " +
        "the machineId is NOT a substitute and none may be invented",
    );
  }
  return {
    hostId,
    ownerId: session.ownerId,
    epoch: session.epoch,
    backendSessionId: session.backendSessionId,
  };
}

export function referenceTargetFor(session) {
  const target = targetFor(session);
  if (isNonEmptyString(session.providerSessionId)) target.providerSessionId = session.providerSessionId;
  return target;
}

/* ==========================================================================
 * Frozen HTTP request shapes
 * ========================================================================== */

export function referenceRouteUrl(host, sessionId, suffix) {
  const base = String(host.url).replace(/\/+$/, "");
  const trimmed = String(suffix || "").replace(/^\/+|\/+$/g, "");
  const path = trimmed.length === 0
    ? REFERENCE_CHAT_ROUTE_PREFIX + "/" + sessionId
    : REFERENCE_CHAT_ROUTE_PREFIX + "/" + sessionId + "/" + trimmed;
  return base + path;
}

/**
 * The read query.
 *
 * The corrected identity contract: a READ OMITS `hostId` entirely and the gateway resolves
 * its own reference-chat host id. `hostId` is therefore NOT part of the base tuple here; a
 * caller adds one only through `extra`, which is what the negative branch that forges a
 * foreign host does.
 *
 * `ownerId`, `epoch` and `backendSessionId` remain required, and `epoch` must equal the
 * gateway's own daemon epoch or the read is refused with TARGET_EXPIRED.
 */
export function referenceReadQuery(session, extra, route) {
  const allowed = REFERENCE_READ_QUERY_FIELDS[route || "history"];
  const params = new URLSearchParams();
  const source = {
    ownerId: session.ownerId,
    epoch: session.epoch,
    backendSessionId: session.backendSessionId,
    providerSessionId: session.providerSessionId,
    registryId: session.registryId,
    ...(extra || {}),
  };
  for (const [key, value] of Object.entries(source)) {
    if (value === undefined || value === null || value === "") continue;
    if (!allowed.includes(key)) {
      throw new Error("query field " + key + " is not admitted by the " + (route || "history") + " route");
    }
    params.set(key, String(value));
  }
  const query = params.toString();
  return query.length === 0 ? "" : "?" + query;
}

/** The frozen mutation envelope: { requestId, target, params } plus identified extras. */
export function mutationEnvelope(requestId, session, params, extra) {
  return {
    requestId,
    target: targetFor(session),
    ...(isNonEmptyString(session.providerSessionId) ? { providerSessionId: session.providerSessionId } : {}),
    ...(isNonEmptyString(session.registryId) ? { registryId: session.registryId } : {}),
    ...(extra || {}),
    params,
  };
}

/**
 * One reference-chat request. The bearer token is read from the host's credential file at
 * call time, never embedded in evidence.
 */
export async function referenceRequest(host, options) {
  const token = readCredentialToken(host);
  const url = referenceRouteUrl(host, options.sessionId, options.suffix) + (options.query || "");
  const headers = { accept: "application/json" };
  if (token) headers.authorization = "Bearer " + token;
  let body;
  if (options.body !== undefined) {
    headers["content-type"] = "application/json";
    body = JSON.stringify(options.body);
  }
  const response = await fetch(url, { method: options.method, headers, body, signal: options.signal });
  const text = await response.text();
  let parsed = null;
  try {
    parsed = text.length > 0 ? JSON.parse(text) : null;
  } catch {
    parsed = null;
  }
  return { status: response.status, headers: Object.fromEntries(response.headers.entries()), text, json: parsed, url };
}

/**
 * A credential file holds either a bare token or a JSON object with a token field. The
 * token is a reference kept outside evidence, exactly as the plan requires.
 */
export function readCredentialToken(host) {
  if (!isNonEmptyString(host.credentialFile) || !existsSync(host.credentialFile)) {
    throw new Error("host " + host.id + " has no readable credential file");
  }
  const raw = readFileSync(host.credentialFile, "utf8").trim();
  if (raw.startsWith("{")) {
    const parsed = JSON.parse(raw);
    const token = parsed.token || parsed.deviceToken || parsed.machineToken || parsed.bearer;
    if (!isNonEmptyString(token)) throw new Error("host " + host.id + " credential file has no token field");
    return token;
  }
  if (raw.length === 0) throw new Error("host " + host.id + " credential file is empty");
  return raw;
}

/** A read failure is the machine error envelope; a mutation failure is the ScopeResult. */
export function assertMachineError(body, expectedCode, label) {
  assert(body && typeof body === "object", label + ": body is not an object");
  assert(typeof body.code === "string", label + ": body has no error code");
  assert(body.code === expectedCode, label + ": expected " + expectedCode + ", got " + body.code);
  return body;
}

export function assertScopeResult(body, requestId, label) {
  assert(body && typeof body === "object", label + ": body is not an object");
  assert(typeof body.ok === "boolean", label + ": body has no ok flag");
  assert(body.requestId === requestId, label + ": requestId does not match");
  if (body.ok) {
    assert(body.data && typeof body.data === "object", label + ": success envelope has no data");
    return body.data;
  }
  assert(body.error && typeof body.error.code === "string", label + ": failure envelope has no error code");
  return body.error;
}

/** The staging receipt the file routes answer with (server.rs reference_chat_stage_file). */
export function assertFileReceipt(data, label) {
  assert(data.receipt && typeof data.receipt === "object", label + ": no receipt");
  for (const field of ["hostId", "attachmentId", "sha256", "sizeBytes", "mediaType"]) {
    assert(data.receipt[field] !== undefined && data.receipt[field] !== null, label + ": receipt has no " + field);
  }
  assert(isNonEmptyString(data.displayName), label + ": receipt has no displayName");
  assert(isNonEmptyString(data.mentionText), label + ": receipt has no mentionText");
  assert(data.mentionText.startsWith("@"), label + ": mention text is not an @path mention");
  return data.receipt;
}

/* ==========================================================================
 * TASK 15 HANDOFF — device-facing CLI and receipt schema (authored, not verified)
 * ==========================================================================
 *
 * Published by task 14 so task 15 can be dispatched on disjoint files. This block is a
 * CONTRACT, not evidence: nothing here has been executed.
 *
 * Files task 15 owns (task 14 does not create or edit them):
 *   scripts/qa/herdr-native-ime.mjs          Android producer (exists in W3; task 15 owns it)
 *   scripts/qa/herdr-reference-device.mjs    iOS producer (to be created by task 15)
 *
 * Fixed invocations (plan, "Fixture provisioning and device commands"):
 *   node scripts/qa/herdr-native-ime.mjs --serial <provisioned-serial> \
 *     --evidence-dir <dir> --page-url <isolated-gateway-url> \
 *     --scenarios direct-once,mode-switch,composition-enter,target-switch
 *   node scripts/qa/herdr-reference-device.mjs --platform ios \
 *     --fixture-manifest <fixtures.json> --scenario QA-08 --case all \
 *     --evidence-dir <dir>
 * The serial comes from the fixture manifest's devices[].serial — never the historically
 * seen R3CN8126R4Y, which may not exist on the verifying host.
 *
 * The receipt each producer writes is validated by validateDeviceReceipt() above. Its
 * binding is four-way, so a receipt can never be satisfied by a different candidate, a
 * different served page, a different pane or a different PTY:
 *
 *   candidate  candidateId + sourceManifestSha256 + binarySha256  -> the frozen build
 *   page       origin + urlRedacted                               -> the isolated gateway UI
 *   target     hostId + ownerId + epoch + backendSessionId + registryId -> the pane
 *   pty        pid + executablePath + cols + rows                 -> the original session
 *
 * plus device {serial, model, osVersion, driverEndpoint}, per-scenario captured payloads,
 * and a cleanup ledger of the exact PIDs the producer spawned.
 *
 * A missing device, a missing driver endpoint or an unacknowledged first-run consent is a
 * BLOCKED receipt (verdict "blocked", nonzero exit) — never a PASS.
 */

/** The receipt schema string every device producer must emit. */
export const REFERENCE_DEVICE_RECEIPT_SCHEMA = DEVICE_RECEIPT_SCHEMA;

/** The blocks a non-blocked receipt must carry, and the fields each block must bind. */
export const REFERENCE_DEVICE_RECEIPT_BINDING = {
  candidate: ["candidateId", "sourceManifestSha256", "binarySha256"],
  page: ["origin", "urlRedacted"],
  target: ["hostId", "ownerId", "epoch", "backendSessionId", "registryId"],
  pty: ["pid", "executablePath", "cols", "rows"],
  device: ["serial", "model", "osVersion", "driverEndpoint"],
};

/** The scenario names the Android producer selects (the plan's fixed list). */
export const REFERENCE_ANDROID_SCENARIOS = [
  "direct-once",
  "mode-switch",
  "composition-enter",
  "target-switch",
];

/** The producer files, for the gate's file-to-task map. */
export const REFERENCE_DEVICE_PRODUCER_FILES = [
  "scripts/qa/herdr-native-ime.mjs",
  "scripts/qa/herdr-reference-device.mjs",
];

/** Where a device receipt lives under an evidence directory. */
export function deviceReceiptPath(evidenceDir, platform) {
  return join(evidenceDir, platform, "device-receipt.json");
}

/**
 * Load and validate a device receipt for QA-08. A missing receipt is a reported
 * prerequisite, never a PASS.
 */
export function readDeviceReceipt(evidenceDir, platform) {
  const path = deviceReceiptPath(evidenceDir, platform);
  if (!existsSync(path)) {
    return { ok: false, status: "MISSING", errors: ["no device receipt at " + path], path };
  }
  let parsed;
  try {
    parsed = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    return { ok: false, status: "FAIL", errors: ["device receipt is not JSON: " + String(error)], path };
  }
  return { ...validateDeviceReceipt(parsed, { platform }), path };
}

/* ==========================================================================
 * TASK 15 INTEGRATION — how the candidate/page/target/PTY binding reaches the
 * device producers (authored contract, not evidence)
 * ==========================================================================
 *
 * The plan's fixed Android command has NO manifest argument:
 *
 *   node scripts/qa/herdr-native-ime.mjs --serial <serial> --evidence-dir <dir> \
 *     --page-url <url> --scenarios direct-once,mode-switch,composition-enter,target-switch
 *
 * That CLI is frozen, so the binding must reach the producer another way. It does NOT come
 * from a guess: the task 14 runner WRITES a binding file from the fixture and candidate
 * manifests plus the live gateway, and the producer READS that file from an explicit path.
 *
 * CHANNEL (agreed, exact)
 *   Producer side: environment variable FERRYX_HERDR_REFERENCE_BINDING holds the ABSOLUTE
 *   path of the binding JSON. There is NO fallback and NO default: an unset variable is a
 *   hard error, because a producer that guessed a path could bind the wrong candidate.
 *   Runner side: --device-binding-out <path> names where to write it (default
 *   <evidence-dir>/device-binding.json), and the resolved absolute path is echoed on stdout
 *   and recorded in report.json so the operator can export it verbatim.
 *
 * The producer copies candidate/page/target/pty from the binding into its receipt verbatim.
 * Every value in the binding is read from a manifest or from the live gateway; none is
 * defaulted.
 *
 * FINALIZED PTY IDENTITY FIELD NAMES (task15 writes these into the receipt unchanged):
 *
 *   binding.pty.pid            the PTY child pid
 *   binding.pty.executablePath the executable recorded at launch for that pid
 *   binding.pty.cols / .rows   the geometry the daemon reported
 *   binding.pty.identity       the full spawn-time record, or null when none exists:
 *     .schema                  "ferryx-herdr-reference.pty-identity/1"
 *     .platform                "unix" | "windows"
 *     .recordedAt              ISO-8601 UTC
 *     .cols / .rows            geometry at record time (may be null)
 *     .ptyChild.pid            the direct PTY child pid
 *     .ptyChild.executable     the wrapper executable (NEVER relabelled as the shell)
 *     .ptyChild.role           "pty-child-wrapper"
 *     .ptyChild.recordedBy     "wrapper-self" | "owner-host-spawn-receipt:<path>"
 *     .ptyChild.ownershipProof "owner-host-spawn-receipt" when the host, not this run, spawned it
 *     .ptyChild.note           present only on a host-supplied record
 *     .sessionShell.pid        the shell pid (equal to ptyChild.pid only where exec preserved it)
 *     .sessionShell.pidKnown   false on windows, where the child pid is NOT guessed
 *     .sessionShell.executable the real shell executable
 *     .sessionShell.role       "session-shell"
 *     .sessionShell.recordedBy same as ptyChild.recordedBy
 *     .sessionShell.execPreservesPid  true on unix, where exec kept the pid
 *     .provider                { executable, role: "provider-process", pid, pidKnown } or null
 *
 * A receipt that carries an identity must not relabel ptyChild.executable as the shell, and
 * must not report a sessionShell pid where pidKnown is false.
 */

/** The binding schema string. */
export const DEVICE_BINDING_SCHEMA = "ferryx-herdr-reference.device-binding/1";

/** The environment variable naming the binding file. No fallback path exists. */
export const REFERENCE_DEVICE_BINDING_ENV = "FERRYX_HERDR_REFERENCE_BINDING";

/**
 * Build the binding the device producers consume. Every field comes from a manifest or the
 * live gateway; a field the run does not have is written as null rather than invented.
 */
export function buildDeviceBinding(options) {
  const candidate = options.candidate || {};
  const host = options.host || {};
  const session = options.session || {};
  const gateway = options.gateway || {};
  const receiptPath = options.receiptPath || null;
  return {
    schema: DEVICE_BINDING_SCHEMA,
    producedAt: new Date().toISOString(),
    candidate: {
      candidateId: candidate.candidateId || null,
      sourceManifestSha256: candidate.sourceManifestSha256 || null,
      binaryPath: candidate.binaryPath || null,
      binarySha256: candidate.binarySha256 || null,
    },
    page: {
      origin: gateway.origin || null,
      url: gateway.url || null,
      urlRedacted: gateway.urlRedacted || null,
    },
    target: {
      // The gateway's OWN reference-chat host id, exactly as a mutation target carries it.
      // The manifest's local label is a fallback for a read-only binding, never a substitute
      // for the authoritative value when the gateway published one.
      hostId: session.referenceHostId || session.hostId || host.id || null,
      ownerId: session.ownerId || null,
      epoch: session.epoch || null,
      backendSessionId: session.backendSessionId || null,
      registryId: session.registryId || null,
      providerSessionId: session.providerSessionId || null,
    },
    pty: {
      pid: session.pid || null,
      executablePath: session.executablePath || null,
      cols: session.cols || null,
      rows: session.rows || null,
      // The full spawn-time identity record, when the provisioner produced one. It states the
      // PTY child (the wrapper) and the session shell as SEPARATE roles with the exact
      // executable recorded at launch, so a reader can never mistake one for the other.
      identity: session.ptyIdentity || null,
    },
    receiptSchema: DEVICE_RECEIPT_SCHEMA,
    receiptPath,
  };
}

/** Write the binding to an absolute path and return that path. */
export function writeDeviceBinding(path, binding) {
  writeJson(path, binding);
  return path;
}

/**
 * Read the binding the task 14 runner wrote. The path comes ONLY from the environment; a
 * missing variable or a missing file is an error, never a default.
 */
export function readDeviceBinding(envValue) {
  const path = envValue === undefined ? process.env[REFERENCE_DEVICE_BINDING_ENV] : envValue;
  if (!isNonEmptyString(path)) {
    throw new Error(
      REFERENCE_DEVICE_BINDING_ENV + " is not set; the device producer must be given the exact " +
        "binding file the task 14 runner wrote (no default path is assumed)",
    );
  }
  if (!isAbsolute(path)) {
    throw new Error(REFERENCE_DEVICE_BINDING_ENV + " must be an absolute path, got: " + path);
  }
  if (!existsSync(path)) {
    throw new Error(REFERENCE_DEVICE_BINDING_ENV + " points at a file that does not exist: " + path);
  }
  let parsed;
  try {
    parsed = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    throw new Error("the device binding is not readable JSON: " + String(error));
  }
  const validated = validateDeviceBinding(parsed);
  if (!validated.ok) {
    throw new Error("the device binding is incomplete: " + validated.errors.join("; "));
  }
  return { path, binding: parsed };
}

/**
 * Validate a binding. A missing candidate, page, target or PTY block is an error: the
 * producer must not invent one to get past this.
 */
export function validateDeviceBinding(value) {
  const errors = [];
  if (!value || typeof value !== "object") {
    return { ok: false, errors: ["device binding is not an object"] };
  }
  if (value.schema !== DEVICE_BINDING_SCHEMA) errors.push("device binding schema is " + String(value.schema));
  for (const [block, fields] of [
    ["candidate", ["candidateId", "sourceManifestSha256", "binarySha256"]],
    ["page", ["origin", "urlRedacted"]],
    ["target", ["hostId", "ownerId", "epoch", "backendSessionId", "registryId"]],
    ["pty", ["pid", "executablePath", "cols", "rows"]],
  ]) {
    if (!value[block] || typeof value[block] !== "object") {
      errors.push("device binding has no " + block + " block");
      continue;
    }
    for (const field of fields) {
      const field_value = value[block][field];
      if (field_value === undefined || field_value === null || field_value === "") {
        errors.push("device binding " + block + " is missing " + field);
      }
    }
  }
  if (!isNonEmptyString(value.receiptPath)) {
    errors.push("device binding names no receiptPath for the producer to write");
  }
  return { ok: errors.length === 0, errors };
}



