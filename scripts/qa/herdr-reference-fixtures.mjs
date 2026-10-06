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
  readlinkSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, isAbsolute, join, resolve, sep } from "node:path";
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

/**
 * The transports whose original session is reached through the owning host's OWN gateway
 * over the frozen HTTP routes - never through a direct daemon connection from this machine.
 *
 * This is the repository's existing mechanism, not a new one: a non-local host is a
 * provisioned machine running its own gateway (directly for ssh/paired, through the relay for
 * account-relay), and every reference-chat route is served by THAT gateway with THAT host's
 * credential. The runner already drives exactly that (`referenceRequest` -> `host.url` +
 * `host.credentialFile`), so the harness needs no second transport and must not invent one.
 *
 * What it does need is the endpoint and the credential. Those cannot be derived from a
 * runtime directory on this machine, which is why they are REQUIRED fixture fields and why an
 * absent one is reported as that exact missing field rather than as a blanket refusal.
 */
export const REFERENCE_NONLOCAL_TRANSPORTS = ["ssh", "paired", "account-relay"];

/** How a host's original session is reached, and therefore what the fixture must declare. */
export const REFERENCE_HOST_ACCESS_KINDS = { localDaemon: "local-daemon", httpGateway: "http-gateway" };

/** The credential tiers a host credential file can carry. */
export const REFERENCE_CREDENTIAL_TIERS = { machine: "machine", device: "device", unattributed: "unattributed" };

/**
 * The credential tier a host's credential file carries, and whether it is readable at all.
 *
 * The relay admits the machine tier (a machine token authenticates the host's own tunnel); a
 * gateway route admits a paired device token. Both are read here rather than assumed, and a
 * file whose tier cannot be shown is reported as `unattributed` instead of being guessed at.
 */
export function readHostCredential(host) {
  if (!isNonEmptyString(host.credentialFile)) {
    return { ok: false, reason: "host-credential-missing", tier: null, token: null };
  }
  if (!existsSync(host.credentialFile)) {
    return { ok: false, reason: "host-credential-unreadable", tier: null, token: null, path: host.credentialFile };
  }
  const raw = readFileSync(host.credentialFile, "utf8").trim();
  if (raw.length === 0) {
    return { ok: false, reason: "host-credential-empty", tier: null, token: null, path: host.credentialFile };
  }
  if (raw.startsWith("{")) {
    let parsed = null;
    try {
      parsed = JSON.parse(raw);
    } catch {
      return { ok: false, reason: "host-credential-unparseable", tier: null, token: null, path: host.credentialFile };
    }
    const machine = isNonEmptyString(parsed.machineToken) ? parsed.machineToken : null;
    const device = isNonEmptyString(parsed.token)
      ? parsed.token
      : isNonEmptyString(parsed.deviceToken)
        ? parsed.deviceToken
        : isNonEmptyString(parsed.bearer)
          ? parsed.bearer
          : null;
    if (machine === null && device === null) {
      return { ok: false, reason: "host-credential-has-no-token", tier: null, token: null, path: host.credentialFile };
    }
    return {
      ok: true,
      tier: machine !== null ? REFERENCE_CREDENTIAL_TIERS.machine : REFERENCE_CREDENTIAL_TIERS.device,
      token: machine !== null ? machine : device,
      path: host.credentialFile,
    };
  }
  return {
    ok: true,
    tier: REFERENCE_CREDENTIAL_TIERS.unattributed,
    token: raw,
    path: host.credentialFile,
  };
}

/**
 * The access contract of one fixture host: how its original session is reached, and what the
 * fixture must have declared for that to be possible.
 *
 * A LOCAL host is reached through a daemon this machine owns (launched or adopted), so its url
 * and credential are produced by that launch and are not required up front.
 *
 * A NON-LOCAL host is reached over the frozen HTTP routes at its own gateway URL with its own
 * credential, so both are REQUIRED. The relay transport additionally requires the machine
 * tier, because a relay tunnel is authenticated by a machine token and a file that cannot be
 * shown to carry one cannot be shown to reach the relay at all.
 *
 * Every failure is an `IsolatedGatewayError` naming the exact missing piece, so a BLOCKED run
 * says which endpoint or credential to provision instead of refusing the transport wholesale.
 *
 * `options.credential` injects an already-resolved credential read, the same way
 * `daemonControlTransport` takes an injected platform: it lets the contract be exercised
 * without a host file on disk.
 */
export function hostAccessContract(host, options) {
  const settings = options || {};
  if (!host || typeof host !== "object" || !isNonEmptyString(host.id)) {
    throw gatewayBlocked("host-unknown", JSON.stringify(host));
  }
  if (!REFERENCE_TRANSPORTS.includes(host.transport)) {
    throw gatewayBlocked("host-transport-unknown", host.id + " -> " + String(host.transport));
  }
  if (host.transport === "local") {
    return {
      hostId: host.id,
      transport: host.transport,
      kind: REFERENCE_HOST_ACCESS_KINDS.localDaemon,
      url: isNonEmptyString(host.url) ? host.url : null,
      credentialFile: isNonEmptyString(host.credentialFile) ? host.credentialFile : null,
      credentialTier: null,
      // A local host's url/credential come from the daemon this run launches or adopts.
      providedBy: "isolated-launch",
    };
  }
  if (!isNonEmptyString(host.url)) {
    throw gatewayBlocked("host-url-missing", host.id + " (" + host.transport + ")");
  }
  if (!/^https?:\/\//.test(host.url.trim())) {
    throw gatewayBlocked("host-url-not-http", host.id + " -> " + host.url);
  }
  const credential = settings.credential || readHostCredential(host);
  if (!credential.ok) {
    throw gatewayBlocked(credential.reason, host.id + " -> " + String(credential.path || host.credentialFile || ""));
  }
  if (host.transport === "account-relay" && credential.tier === REFERENCE_CREDENTIAL_TIERS.unattributed) {
    throw gatewayBlocked(
      "host-machine-credential-required",
      host.id + " reaches the relay through a machine-authenticated tunnel; its credential file " +
        "carries an unattributed bare token, so it cannot be shown to be one. Write " +
        '{"machineToken":"..."} or use a paired gateway URL instead.',
    );
  }
  return {
    hostId: host.id,
    transport: host.transport,
    kind: REFERENCE_HOST_ACCESS_KINDS.httpGateway,
    url: host.url.trim().replace(/\/+$/, ""),
    credentialFile: credential.path,
    credentialTier: credential.tier,
    // The owning host spawned the original PTY, so its identity arrives as that host's own
    // spawn receipt rather than from anything this machine can observe.
    providedBy: "provisioned-host",
  };
}

/**
 * What a non-local session still needs before its original-PTY identity can be asserted: the
 * OWNING host's own spawn receipt. Returns null when the session already carries one.
 *
 * This is the identity half of the non-local contract, and it is deliberately separate from
 * the endpoint half: an endpoint proves the session is reachable, never that this run created
 * the process. A pid typed into a config is not ownership proof.
 */
export function ownerHostReceiptRequirement(host, session) {
  if (!host || host.transport === "local") return null;
  const receiptPath = (session && session.spawnReceiptPath) ||
    (host && host.spawnReceiptPath) || null;
  if (isNonEmptyString(receiptPath)) return null;
  return {
    kind: "owner-host-spawn-receipt",
    hostId: host.id,
    transport: host.transport,
    backendSessionId: (session && session.backendSessionId) || null,
    requiredFields: ["sourceKind", "hostId", "transport", "backendSessionId", "epoch", "pid", "executable", "spawnedAt", "candidate"],
    reason: "no owner-host spawn receipt is configured for this externally spawned session; " +
      "an opaque pid from the config is not ownership proof",
  };
}

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
 * The identity of a live process: its executable PATH and command line, read from the OS.
 *
 * The OS is asked for the path, never for a name. A name is not identity: two different
 * executables can share a basename, so a name match would let an unrelated process - a
 * recycled PID, or the same build in another directory - be treated as one this harness
 * started. This is the only identity the teardown policy accepts.
 *
 * Per platform: Windows reads `Get-Process ... -ExpandProperty Path`; Linux reads
 * `/proc/<pid>/exe`, which is the path and not the 15-character `comm` name; Darwin reads
 * `ps -o comm=`, which reports the executable path there. Where an OS yields only a name,
 * the probe says so by returning it as-is, and every comparison then fails closed.
 */
export function probeProcessIdentity(pid) {
  if (process.platform === "win32") {
    // Existence and path are asked for SEPARATELY: a process that exists but whose path this
    // account may not read is ALIVE with an unreadable identity, which must report as such
    // rather than be flattened into "already exited" - the two have different remedies.
    const listing = spawnSync(
      "powershell",
      [
        "-NoProfile",
        "-Command",
        "$p = Get-Process -Id " + pid + " -ErrorAction SilentlyContinue | Select-Object -First 1; " +
          "if ($null -eq $p) { '' } else { @{ id = $p.Id; path = $p.Path } | ConvertTo-Json -Compress }",
      ],
      { encoding: "utf8" },
    );
    const raw = (listing.stdout || "").trim();
    if (raw.length === 0) return { pid, alive: false, executable: null, commandLine: null };
    let parsed = null;
    try {
      parsed = JSON.parse(raw);
    } catch {
      parsed = null;
    }
    const executable =
      parsed && typeof parsed.path === "string" && parsed.path.trim().length > 0 ? parsed.path.trim() : null;
    return { pid, alive: true, executable, commandLine: executable };
  }
  const args = spawnSync("ps", ["-p", String(pid), "-o", "args="], { encoding: "utf8" });
  const commandLine = (args.stdout || "").trim();
  const fromProc = linuxExecutablePath(pid);
  if (fromProc !== null) return { pid, alive: true, executable: fromProc, commandLine: commandLine || null };
  const comm = spawnSync("ps", ["-p", String(pid), "-o", "comm="], { encoding: "utf8" });
  const executable = (comm.stdout || "").trim();
  return { pid, alive: executable.length > 0, executable: executable || null, commandLine: commandLine || null };
}

/** `/proc/<pid>/exe` on Linux: the executable PATH, or null where it is unavailable. */
function linuxExecutablePath(pid) {
  if (process.platform !== "linux") return null;
  try {
    return readlinkSync("/proc/" + pid + "/exe").replace(/ \(deleted\)$/, "");
  } catch {
    return null;
  }
}

/**
 * A path as identity: absolute, symlink-resolved where the file is reachable, separators
 * unified, and case-folded ONLY on a case-insensitive filesystem (Windows).
 *
 * This is deliberately a PATH comparison and not a name comparison. Two executables in
 * different directories never compare equal here, however they are spelled.
 */
export function normalizeExecutablePath(value, platform) {
  if (typeof value !== "string") return null;
  const trimmed = value.trim().replace(/ \(deleted\)$/, "");
  if (trimmed.length === 0) return null;
  let resolved = trimmed;
  try {
    if (existsSync(trimmed)) resolved = realpathSync.native ? realpathSync.native(trimmed) : realpathSync(trimmed);
  } catch {
    /* Unreachable now (a process that has exited): the lexical path is still comparable. */
  }
  const isWindows = (platform || process.platform) === "win32";
  let path = resolved.replace(/\\/g, "/").replace(/\/{2,}/g, "/");
  if (path.length > 1) path = path.replace(/\/+$/, "");
  if (isWindows) {
    path = path.replace(/^([a-z]):/, (_match, drive) => drive.toUpperCase() + ":");
    path = path.toLowerCase();
  }
  return path.length === 0 ? null : path;
}

/**
 * Whether a live process's executable IS the executable recorded at spawn time: full
 * normalized path equality, both sides known. A null on either side is NOT a match - an
 * identity that cannot be read is reported, never assumed.
 */
export function executablePathMatches(liveExecutable, recordedExecutable, platform) {
  const live = normalizeExecutablePath(liveExecutable, platform);
  const recorded = normalizeExecutablePath(recordedExecutable, platform);
  return live !== null && recorded !== null && live === recorded;
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
 * those exact PIDs after re-reading the live executable and confirming it is the SAME FILE,
 * by full normalized path equality. A basename is never identity proof: another directory's
 * executable can share the name. Anything else - a mismatch, or an identity that cannot be
 * read at all - is reported, never killed.
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
      const matches = executablePathMatches(live.executable, recorded, process.platform);
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

/* ==========================================================================
 * Daemon control transport (QA only)
 *
 * The product publishes a DIFFERENT control endpoint per platform, and this harness must
 * speak the one the profile it started actually publishes:
 *
 *   unix   <runtime>/daemon.sock - a unix socket, and nothing else. The socket's ownership
 *          and mode ARE the authentication: `get_transport_token_path` does not exist on
 *          unix at all, and `handle_client` says "Unix inherits the socket's ownership
 *          boundary and never needs the check".
 *   win32  <runtime>/daemon.port (the loopback TCP port, as text) plus
 *          <runtime>/daemon.token (the per-boot bearer). `get_socket_path` returns
 *          `daemon.port` on non-unix, and the token path states why: "A loopback TCP port
 *          has no filesystem ownership check, so the published port alone must not be
 *          enough to drive the daemon." The FIRST frame of every connection must carry
 *          that token, or the daemon answers TRANSPORT_UNAUTHORIZED and closes.
 *
 * Dialling `<runtime>/daemon.sock` on Windows is the defect this section replaces: the
 * product never creates that file there, so the connect failed with a bare ENOENT that
 * looked like a daemon which had not started.
 *
 * NOTE: `gatewayBlocked` is declared further down this module. Only the CALL is here, and
 * that binding is initialised long before any caller can reach this section.
 * ========================================================================== */

/** The transport files a daemon publishes in its runtime directory (daemon/server.rs). */
export const DAEMON_TRANSPORT_FILES = {
  unixSocket: "daemon.sock",
  port: "daemon.port",
  token: "daemon.token",
};

/** The address the non-unix listener binds, and the only one this harness will dial. */
export const DAEMON_LOOPBACK_HOST = "127.0.0.1";

/** The transport kinds this harness speaks. */
export const DAEMON_TRANSPORT_KINDS = { unixSocket: "unix-socket", loopbackTcp: "loopback-tcp" };

/**
 * The unix control socket of a profile's daemon, mirroring `get_socket_path` on unix.
 *
 * This is the unix transport ONLY. Windows publishes no such file - `get_socket_path`
 * returns `daemon.port` there - so a Windows caller must go through
 * [`daemonControlTransport`] instead, which is what the launcher in this module does.
 */
export function daemonSocketPath(profile) {
  return join(profile.paths.runtime, DAEMON_TRANSPORT_FILES.unixSocket);
}

/**
 * The control transport a profile's daemon publishes, derived from `profile.paths.runtime`
 * alone. Nothing here falls back to a production endpoint: a runtime directory outside the
 * profile root is refused outright, so the harness can only ever drive the daemon it started
 * itself.
 *
 * `options.platform` defaults to `process.platform` and exists so the Windows branch is
 * exercisable - and regression-covered - from any host.
 */
export function daemonControlTransport(profile, options) {
  const settings = options || {};
  const platform = settings.platform || process.platform;
  const runtimeDir = resolve(profile.paths.runtime);
  const root = profile.root ? resolve(profile.root) : null;
  if (root && runtimeDir !== root && !runtimeDir.startsWith(root + sep)) {
    throw gatewayBlocked(
      "daemon-runtime-not-profile-owned",
      runtimeDir + " is outside the isolated profile " + root,
    );
  }
  if (platform === "win32") {
    return {
      kind: DAEMON_TRANSPORT_KINDS.loopbackTcp,
      platform,
      runtimeDir,
      host: DAEMON_LOOPBACK_HOST,
      portFile: join(runtimeDir, DAEMON_TRANSPORT_FILES.port),
      tokenFile: join(runtimeDir, DAEMON_TRANSPORT_FILES.token),
      requiresToken: true,
    };
  }
  return {
    kind: DAEMON_TRANSPORT_KINDS.unixSocket,
    platform,
    runtimeDir,
    socketPath: join(runtimeDir, DAEMON_TRANSPORT_FILES.unixSocket),
    tokenFile: null,
    requiresToken: false,
  };
}

/**
 * A published transport token, or null.
 *
 * An absent file, or one holding only whitespace, is NO credential: the product reports it
 * as absent (`read_transport_token_at`) so a reader treats the pair it is reading as
 * incomplete rather than presenting an empty token the daemon would reject.
 */
export function parseDaemonTransportToken(text) {
  if (typeof text !== "string") return null;
  const token = text.trim();
  return token.length === 0 ? null : token;
}

/**
 * The loopback port a `daemon.port` file publishes, or null when the file is not a usable
 * port. The product writes the port as decimal text and reads it back with `trim().parse()`,
 * so anything that is not exactly a 1..65535 decimal is refused rather than guessed at.
 */
export function parseDaemonPortFile(text) {
  if (typeof text !== "string") return null;
  const trimmed = text.trim();
  if (!/^\d{1,5}$/.test(trimmed)) return null;
  const port = Number.parseInt(trimmed, 10);
  return port >= 1 && port <= 65535 ? port : null;
}

/**
 * The first frame of a control connection: one newline-terminated JSON line, carrying
 * `token` only for a transport that requires it.
 *
 * `transport_token_from_line` reads the bearer from exactly that field of the first line, so
 * a transport that must present one and does not is refused (`TRANSPORT_UNAUTHORIZED`) before
 * any request is dispatched. A unix socket presents none and is not checked.
 */
export function daemonHandshakeFrame(transport, token, version) {
  const payload = {
    type: "handshake",
    version: typeof version === "number" ? version : DAEMON_CONTROL_PROTOCOL_VERSION,
  };
  if (transport && transport.requiresToken) payload.token = token;
  return JSON.stringify(payload) + "\n";
}

/* ==========================================================================
 * Workspace registration (the owning daemon's own contract)
 *
 * A daemon does not accept a spawn for a workspace it has never heard of: `Spawn` carries a
 * non-optional `workspaceId` (daemon/protocol.rs) and the daemon answers `Workspace '<id>' is
 * not registered` for an id nothing created. The headless daemon registers ONE startup
 * workspace from its process working directory (`ipc/project::initial_project`), and that id is
 * derived - from the cwd, its git top level and the launch route - so a config cannot rely on it
 * to produce the id the config itself names. The bridge is the daemon's own explicit request:
 *
 *   RegisterWorkspace { workspaceId, repoRoot }  ->  RegisterWorkspaceOk
 *
 * The daemon validates it (daemon/workspace_service.rs `register`): the id must pass
 * `WorkspaceRegistry::validate_workspace_id`, and `repoRoot` must be an absolute path that
 * canonicalizes to a directory which IS the canonical repository root. This harness therefore
 * mirrors the id rules and checks the root exists BEFORE sending, so a config error is reported
 * as a config error instead of as an opaque daemon refusal.
 * ========================================================================== */

/**
 * The workspace id rules the daemon enforces (`WorktreeRegistry::validate_workspace_id`).
 * Returns null when the id is acceptable, or the reason it is not.
 */
export function workspaceIdRefusal(workspaceId) {
  if (typeof workspaceId !== "string" || workspaceId.trim().length === 0) {
    return "the session names no workspaceId; the daemon refuses a spawn for an unnamed workspace";
  }
  const id = workspaceId.trim();
  if (id.startsWith("daemon:") || id.includes(":")) {
    return 'workspaceId "' + id + '" is a daemon/remote namespace, which the local registry refuses';
  }
  if (id.startsWith("-") || id.includes("/") || id.includes("\\")) {
    return 'workspaceId "' + id + '" contains a character the registry refuses';
  }
  if ([...id].some((ch) => ch.trim().length === 0 || /[\u0000-\u001f\u007f]/.test(ch))) {
    return 'workspaceId "' + id + '" contains whitespace or a control character';
  }
  return null;
}

/**
 * The repository root a session's workspace must be registered against, or a typed refusal.
 *
 * The root is DECLARED, never inferred. The daemon's startup registration derives an id from its
 * working directory, which is exactly the incidental behaviour this harness must not depend on,
 * and the daemon requires the canonical repository root - not a subdirectory, not a symlink
 * spelling. `session.repoRoot` (or the host's, for a host that declares one) is that value; a
 * missing or unusable one is reported as the exact missing field.
 */
export function workspaceRootFor(host, session) {
  const declared = (session && session.repoRoot) || (host && host.repoRoot) || null;
  if (!isNonEmptyString(declared)) {
    return { ok: false, reason: "workspace-repo-root-missing", detail: "declare session.repoRoot (or host.repoRoot) for " + ((session && session.workspaceId) || "the session") };
  }
  if (!isAbsolute(declared)) {
    return { ok: false, reason: "workspace-repo-root-not-absolute", detail: declared };
  }
  if (!existsSync(declared)) {
    return { ok: false, reason: "workspace-repo-root-absent", detail: declared };
  }
  let canonical = declared;
  try {
    canonical = realpathSync.native ? realpathSync.native(declared) : realpathSync(declared);
  } catch {
    /* Unreachable now: the declared spelling is still the best available root. */
  }
  return { ok: true, repoRoot: canonical, declared };
}

/**
 * The `RegisterWorkspace` frame for one session's workspace, or a typed refusal describing what
 * the config is missing. Nothing here is guessed: the id rules and the root requirements are the
 * daemon's own.
 */
export function workspaceRegistrationFor(host, session) {
  const idRefusal = workspaceIdRefusal(session && session.workspaceId);
  if (idRefusal) return { ok: false, reason: "workspace-id-invalid", detail: idRefusal };
  const root = workspaceRootFor(host, session);
  if (!root.ok) return root;
  return {
    ok: true,
    workspaceId: String(session.workspaceId).trim(),
    repoRoot: root.repoRoot,
    declaredRoot: root.declared,
    request: { type: "registerWorkspace", workspaceId: String(session.workspaceId).trim(), repoRoot: root.repoRoot },
  };
}

/**
 * Register one workspace on the daemon this run owns, over the same control connection that will
 * spawn in it. The request is sent BEFORE the spawn, so the workspace exists by the time the spawn
 * names it.
 *
 * Returns `{ ok: true }` on `registerWorkspaceOk`, and otherwise a structured refusal: a typed
 * transport failure, or the daemon's own rejection with its message. A workspace already
 * registered against the same root is accepted by the daemon, so re-running is safe.
 */
export async function registerWorkspaceOnDaemon(client, registration, meta) {
  // This call knows exactly what it asked for, so it hands that identity to the failure reader:
  // a daemon refusal names its own message, and the request kind comes from here rather than
  // being read out of a reply that does not carry one.
  const requested = {
    requestKind: "registerWorkspace",
    requestId: (meta && meta.requestId) || registration.workspaceId,
  };
  const response = await client.call(registration.request, requested);
  const failure = daemonRequestFailure(response, requested);
  if (failure) {
    return { ok: false, reason: failure.code, detail: describeDaemonRequestFailure(failure) };
  }
  if (!response || response.type !== "registerWorkspaceOk") {
    // An unexpected reply is still a refusal of THIS request: the same description keeps the
    // request identity and carries the reply verbatim instead of dropping either.
    return {
      ok: false,
      reason: "workspace-registration-refused",
      detail: describeDaemonRequestFailure({
        code: "workspace-registration-refused",
        requestKind: requested.requestKind,
        requestId: requested.requestId,
        message: JSON.stringify(response),
      }),
    };
  }
  return { ok: true, workspaceId: registration.workspaceId, repoRoot: registration.repoRoot };
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
 *   5. the control endpoint is the profile's OWN
 *      transport, never a production one: a unix
 *      socket where the product publishes one, and on
 *      Windows the loopback TCP pair
 *      <runtime>/daemon.port + daemon.token, whose
 *      token the FIRST frame must carry because a
 *      port has no ownership check   daemon/server.rs, daemon/client.rs
 *   6. the bound address is read back from the daemon
 *      (RemoteGetStatus) and is the ONLY authority for
 *      the URL this harness dials                  daemon/protocol.rs
 *   7. the bearer token comes from the real pairing flow over the daemon's own control
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

/**
 * The contract id of the ownership record a launcher hands to the next process in the same
 * run chain, so that process can ADOPT the exact daemon instead of launching a second one.
 */
export const ISOLATED_GATEWAY_OWNERSHIP_CONTRACT = "ferryx-herdr-reference.isolated-gateway-ownership/1";

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
 * One connection to a daemon's control endpoint (newline-delimited JSON, protocol v5), over
 * whichever transport the profile publishes.
 *
 *   - `unix-socket`: connect the socket and present no token - the ownership boundary
 *     authenticates, and the product never checks a token there.
 *   - `loopback-tcp`: read the CREDENTIAL BEFORE THE PORT, exactly as `DaemonClient` does
 *     ("the credential is read before the port is, so the token presented can never be newer
 *     than the port it is presented to"), connect `127.0.0.1:<port>`, and put that token on
 *     the first frame. Without it the daemon answers TRANSPORT_UNAUTHORIZED and closes,
 *     because a loopback port has no filesystem ownership check.
 *
 * A missing or unusable transport file is reported as its own reason, naming the exact path,
 * rather than surfacing a bare ENOENT from `connect`: on Windows that ENOENT was read as a
 * daemon that had not started, when the harness was in fact dialling a file the product never
 * creates.
 *
 * Deliberate divergence from the product: `DaemonClient` re-reads a straddled port/token pair
 * once, because a machine's runtime directory outlives a reboot. This profile is created by
 * this run, so it cannot hold a predecessor's pair and a rejection here is real - it is
 * reported, not retried.
 *
 * A descriptor from [`daemonControlTransport`] is expected; a bare string is accepted as a
 * unix socket path so an existing caller keeps working. A socket that closes with requests in
 * flight resolves them as null, which every caller already treats as a protocol failure.
 */
export function connectDaemonControl(transport, options) {
  const settings = options || {};
  const timeoutMs = typeof settings.timeoutMs === "number" ? settings.timeoutMs : 30000;
  const resolved =
    typeof transport === "string"
      ? { kind: DAEMON_TRANSPORT_KINDS.unixSocket, socketPath: transport, tokenFile: null, requiresToken: false }
      : transport;
  if (!resolved || typeof resolved !== "object") {
    throw gatewayBlocked("daemon-transport-missing", String(transport));
  }

  const explicitToken =
    typeof settings.token === "string" && settings.token.trim().length > 0 ? settings.token.trim() : null;
  let token = explicitToken;
  if (resolved.requiresToken && token === null) {
    if (!resolved.tokenFile || !existsSync(resolved.tokenFile)) {
      throw gatewayBlocked("daemon-transport-token-missing", String(resolved.tokenFile || ""));
    }
    token = parseDaemonTransportToken(readFileSync(resolved.tokenFile, "utf8"));
    if (token === null) {
      throw gatewayBlocked("daemon-transport-token-empty", resolved.tokenFile);
    }
  }

  let connection = { path: resolved.socketPath };
  let endpoint = String(resolved.socketPath);
  if (resolved.kind === DAEMON_TRANSPORT_KINDS.loopbackTcp) {
    if (!resolved.portFile || !existsSync(resolved.portFile)) {
      throw gatewayBlocked("daemon-transport-port-missing", String(resolved.portFile || ""));
    }
    const portText = readFileSync(resolved.portFile, "utf8");
    const port = parseDaemonPortFile(portText);
    if (port === null) {
      throw gatewayBlocked(
        "daemon-transport-port-invalid",
        resolved.portFile + " -> " + JSON.stringify(portText.slice(0, 32)),
      );
    }
    const host = resolved.host || DAEMON_LOOPBACK_HOST;
    connection = { host, port };
    endpoint = host + ":" + port;
  }

  return deadline(
    new Promise((resolveConnect, rejectConnect) => {
      const socket = createConnection(connection);
      // `crlfDelay` only affects line splitting; the important part is that the Interface's
      // own `error` event is subscribed BELOW, before anything can emit it. readline re-emits
      // a socket error on the Interface, and an Interface with no error listener makes Node
      // throw the raw stream error ("Unhandled 'error' event ... on Interface instance"),
      // which kills the process with a trace instead of a typed reason. That is the crash this
      // client used to have.
      const lines = createInterface({ input: socket, crlfDelay: Infinity });
      const pending = [];
      // Every event this connection observed, in order. Diagnostics for evidence only - it
      // never carries the transport token or any other secret.
      const events = [];
      const note = (event, detail) => {
        events.push({ at: new Date().toISOString(), event, detail: detail === undefined ? null : String(detail) });
        if (events.length > 64) events.shift();
      };
      // Settled exactly once. The first cause wins; later events are recorded as diagnostics
      // and cannot re-settle anything, so a reset followed by a close (or the reverse) is one
      // outcome, not two.
      let settled = false;
      // Whether this connection's resources are released. A socket that died on its own is
      // already released; this exists so a caller (and a test's cleanup hook) can tell a live
      // handle from a dead one without probing the process table.
      let closed = false;
      const settleAll = (code, message) => {
        if (settled) {
          note("settle-ignored", code);
          return false;
        }
        settled = true;
        note("settled", code + " inflight=" + pending.length);
        const failure = (entry) =>
          entry.resolve({
            type: "error",
            code,
            message,
            requestKind: entry.requestKind || "unknown",
            requestId: entry.requestId || null,
          });
        while (pending.length > 0) failure(pending.shift());
        return true;
      };
      // Registered before any I/O is attempted: both the socket and the readline Interface
      // have an error listener from the start, so no stream error can ever be unhandled.
      //
      // The Interface is not a detail. readline re-emits the socket's error on the Interface,
      // and an Interface with no `error` listener makes Node throw the raw stream error
      // ("Unhandled 'error' event ... Emitted 'error' event on Interface instance"), which
      // killed this provisioner with a stream trace instead of a typed reason. That is the
      // crash this file used to have, and the Interface listener is what closes it.
      lines.on("error", (error) => {
        const code = transportErrorCode(error);
        note("interface-error", code);
        // Socket lifetime: an Interface error means the stream underneath is unusable, so the
        // socket is torn down here rather than left half-open with nothing reading it.
        settleAll(code, endpoint + " " + String(error && error.message ? error.message : error));
        rejectConnect(gatewayBlocked(code, endpoint + " " + String(error)));
        socket.destroy();
      });
      socket.on("error", (error) => {
        const code = transportErrorCode(error);
        note("socket-error", code);
        settleAll(code, endpoint + " " + String(error && error.message ? error.message : error));
        rejectConnect(gatewayBlocked(code, endpoint + " " + String(error)));
      });
      socket.on("close", (hadError) => {
        note("socket-close", hadError ? "had-error" : "clean");
        closed = true;
        // A close with requests in flight settles them structurally rather than with `null`:
        // a caller that receives `null` cannot tell a reset from a malformed reply.
        settleAll("daemon-transport-closed", endpoint + " closed with requests in flight");
        // Socket lifetime again: the Interface holds the socket's readers, so it is closed
        // with it. A dangling Interface could still emit after the connection is gone.
        try {
          lines.close();
        } catch {
          /* Already closed. */
        }
      });
      lines.on("line", (line) => {
        let message;
        try {
          message = JSON.parse(line);
        } catch {
          return;
        }
        const next = pending.shift();
        if (next) next.resolve(message);
      });
      const send = (line, meta) =>
        new Promise((resolveCall) => {
          if (settled) {
            resolveCall({
              type: "error",
              code: "daemon-transport-closed",
              message: endpoint + " is no longer connected",
              requestKind: (meta && meta.requestKind) || "unknown",
              requestId: (meta && meta.requestId) || null,
            });
            return;
          }
          pending.push({
            resolve: resolveCall,
            requestKind: (meta && meta.requestKind) || null,
            requestId: (meta && meta.requestId) || null,
          });
          socket.write(line);
        });
      const call = (payload, meta) =>
        send(JSON.stringify(payload) + "\n", meta || { requestKind: payload && payload.type ? payload.type : null });
      socket.once("connect", async () => {
        try {
          note("connected", resolved.kind);
          const handshake = await send(daemonHandshakeFrame(resolved, token), {
            requestKind: "handshake",
            requestId: "handshake",
          });
          if (!handshake || handshake.type !== "handshakeOk") {
            socket.destroy();
            const unauthorized = Boolean(handshake) && handshake.code === "TRANSPORT_UNAUTHORIZED";
            rejectConnect(
              gatewayBlocked(
                unauthorized ? "daemon-transport-unauthorized" : "daemon-handshake-refused",
                endpoint + " " + JSON.stringify(handshake),
              ),
            );
            return;
          }
          note("handshake-ok", "epoch=" + String(handshake.epoch));
          resolveConnect({
            transportKind: resolved.kind,
            endpoint,
            handshake,
            call,
            // The daemon's own exit closes this socket. A caller that asks the daemon to
            // shut down awaits that close instead of polling for the process to die.
            onClose(listener) {
              socket.once("close", listener);
            },
            // What this connection saw and how it ended, for evidence. Never a secret.
            diagnostics() {
              return { endpoint, transportKind: resolved.kind, settled, closed, events: events.slice() };
            },
            // True once the socket is gone, whether this handle closed it or the peer did.
            get closed() {
              return closed;
            },
            close() {
              if (closed) return;
              closed = true;
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

/**
 * The structured failure of a settled daemon request, or null for a real reply.
 *
 * A transport that ends mid-request settles every in-flight request with this shape, so a
 * caller reads a code and the request it belonged to instead of an opaque null. The message
 * carries no secret: the transport token never travels in a response or a diagnostic.
 *
 * A reply the DAEMON itself sent carries only what the daemon chose to send - typically a code
 * and a message, and no request identity. The caller, however, always knows which request it
 * issued, so `fallback` supplies that identity and is used ONLY where the reply is silent. Both
 * facts therefore survive: the daemon's own message is never replaced by the caller's, and the
 * request that provoked it is never lost.
 */
export function daemonRequestFailure(response, fallback) {
  if (!response || typeof response !== "object" || response.type !== "error") return null;
  const known = fallback || {};
  return {
    code: typeof response.code === "string" && response.code.length > 0 ? response.code : "daemon-transport-error",
    message: typeof response.message === "string" ? response.message : "",
    requestKind:
      typeof response.requestKind === "string" && response.requestKind.length > 0
        ? response.requestKind
        : isNonEmptyString(known.requestKind) ? known.requestKind : "unknown",
    requestId:
      response.requestId !== undefined && response.requestId !== null
        ? response.requestId
        : known.requestId === undefined ? null : known.requestId,
  };
}

/**
 * A one-line, secret-free description of a settled daemon request failure, for a typed
 * blocker detail: the code, the request kind, the request id and the endpoint message.
 */
export function describeDaemonRequestFailure(failure) {
  if (!failure) return "";
  return failure.code + " " + failure.requestKind +
    (failure.requestId ? "#" + failure.requestId : "") + " " + failure.message;
}

/**
 * The typed code for a stream error, derived from the errno the OS reported. The message is
 * never used as the discriminator: a caller parses the code, and the raw error text travels
 * only as a detail.
 */
export function transportErrorCode(error) {
  const errno = error && typeof error.code === "string" ? error.code : null;
  if (errno === "ECONNRESET") return "daemon-transport-reset";
  if (errno === "ECONNREFUSED") return "daemon-transport-refused";
  if (errno === "EPIPE") return "daemon-transport-pipe-closed";
  if (errno === "ENOENT") return "daemon-transport-missing";
  if (errno === "ETIMEDOUT") return "daemon-transport-timed-out";
  return "daemon-transport-error";
}

/**
 * One bounded JSON request against the gateway, with the failure kept verbatim. */
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
 * options: { binary, label, uiDist, env, cwd, ledger, timeoutMs, extra, lease }. `lease`
 * (`{ ownerPid, createdAt, deadlineAt }`) is copied into the handle's ownership record: it is
 * what a caller sets when it intends to HAND THE DAEMON OVER to another process instead of
 * stopping it, and the adopter refuses an expired one. The returned
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
  const transport = daemonControlTransport(profile);
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

  // The daemon's OWN log file, inside this run's isolated profile (daemon/logging.rs opens
  // <FERRYX_DATA_DIR>/logs/daemon.log, and setupIsolatedProfile sets FERRYX_DATA_DIR to
  // <profile>/data). It is destroyed with the profile, so it must be captured before cleanup.
  const daemonLogPath = join(profile.paths.data, "logs", "daemon.log");

  const child = spawn(binary, ["--daemon"], {
    cwd: settings.cwd || repoRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  const entry = settings.ledger
    ? settings.ledger.record(child, { ...(settings.extra || {}), launchArgs: ["--daemon"], remoteConfig: configPath })
    : { pid: child.pid, executablePath: child.spawnfile, argv: child.spawnargs };

  // The daemon's own output, bounded and kept for evidence. A reset is not diagnosable
  // without it: the last thing the daemon wrote before the connection dropped is what
  // attributes the failure, and this harness does not get to assert a cause it did not
  // observe. 64 KiB per stream is enough for a boot trace and bounded on purpose.
  const OUTPUT_LIMIT = 64 * 1024;
  let stderr = "";
  child.stderr.on("data", (data) => {
    stderr = (stderr + data.toString()).slice(-OUTPUT_LIMIT);
  });
  const exited = new Promise((resolveExit) => {
    child.once("exit", (code, signal) => {
      noteDaemonExit({ code, signal });
      resolveExit({ code, signal });
    });
  });
  let exitInfo = null;
  const noteDaemonExit = (info) => {
    exitInfo = { at: new Date().toISOString(), code: info.code, signal: info.signal };
  };
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
      stdout = (stdout + data.toString()).slice(-OUTPUT_LIMIT);
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
    // The exact PID this run spawned, re-identified before it is signalled: only the SAME
    // executable file (full normalized path) may be signalled, so a recycled PID or a
    // same-named binary in another directory is reported instead.
    const live = probeProcessIdentity(child.pid);
    const recorded = entry.executablePath;
    const identityMatches = live.alive && executablePathMatches(live.executable, recorded, process.platform);
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
    control = await connectDaemonControl(transport, { timeoutMs });
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

    // The real pairing flow, over the daemon's own control endpoint and the gateway's own
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
      // The connectable descriptor for the daemon this run started, so a caller that must
      // speak to the SAME daemon (the provisioner's PTY spawn) does not have to re-derive
      // one from a config that may not name this throwaway profile at all. The JSON-safe
      // summary is `daemonTransport`; this raw form holds local file paths and is never
      // serialized into a receipt.
      controlTransport: transport,
      // Everything the next process in this run chain needs to adopt THIS daemon rather
      // than launch another. A fresh launch would have a different epoch and none of the
      // sessions provisioned here, so a recorded originalPTY identity would end up asserted
      // against a daemon that never spawned it.
      ownership: ownershipRecord(profile, transport, child, entry, url, credentialFile, control, settings.lease),
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
      // The control endpoint this run actually drove, and the transport it used. The socket
      // field stays a truthful string-or-null: on Windows the product publishes no socket,
      // only the loopback port/token pair.
      daemonTransport: {
        kind: transport.kind,
        platform: transport.platform,
        runtimeDir: transport.runtimeDir,
        endpoint: control.endpoint,
        requiresToken: transport.requiresToken,
      },
      daemonSocketPath: transport.kind === DAEMON_TRANSPORT_KINDS.unixSocket ? transport.socketPath : null,
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
      // Everything needed to attribute a later transport failure: what the daemon wrote, how
      // it exited, and what the control connection observed. No secret is included - the
      // transport token never reaches these buffers.
      daemonOutput() {
        return {
          pid: child.pid,
          executablePath: entry.executablePath,
          exit: exitInfo,
          stdout: stdout.slice(-OUTPUT_LIMIT),
          stderr: stderr.slice(-OUTPUT_LIMIT),
          diagnostics: control ? control.diagnostics() : null,
        };
      },
      // Persisted QA evidence: written when this handle is stopped and on any failure, so the
      // next run can read why the daemon connection ended instead of guessing.
      evidenceName: settings.evidenceName || null,
      persistDaemonOutput(dir, name) {
        return persistDaemonOutput(
          dir,
          name || settings.evidenceName || "daemon-output.log",
          child,
          entry,
          exitInfo,
          stdout,
          stderr,
          control,
          daemonLogPath,
        );
      },
      stop: stopOwnedDaemon,
    };
  } catch (error) {
    // The daemon's own output is captured BEFORE it is stopped: after the shutdown its final
    // lines are gone, and a reset cannot be attributed without them. This is evidence, not a
    // claim about the cause.
    try {
      persistDaemonOutput(
        settings.evidenceDir,
        settings.evidenceName || "daemon-output.log",
        child,
        entry,
        exitInfo,
        stdout,
        stderr,
        control,
        daemonLogPath,
      );
    } catch {
      /* Evidence that cannot be written must not replace the failure it explains. */
    }
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
    // The failure detail carries what the connection observed, so a BLOCKED line names the
    // request that was in flight when the transport ended.
    if (control && typeof control.diagnostics === "function") {
      try {
        const trail = control.diagnostics();
        error.detail = (error.detail ? error.detail + " " : "") + "diagnostics=" + JSON.stringify(trail.events.slice(-4));
      } catch {
        /* Diagnostics are best-effort. */
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

/**
 * The last `limit` bytes of a file, or a short explanation of why it is not there.
 *
 * Bounded on purpose: a daemon log can grow without limit, and evidence that cannot be read
 * in one pass is not evidence. A missing file is reported as missing rather than as empty,
 * because "the daemon wrote nothing" and "the daemon log was never there" are different
 * findings.
 */
export function readBoundedTail(path, limit) {
  if (!isNonEmptyString(path)) return "<no path>";
  if (!existsSync(path)) return "<absent: " + path + ">";
  try {
    const raw = readFileSync(path, "utf8");
    const bound = typeof limit === "number" && limit > 0 ? limit : 64 * 1024;
    return raw.length > bound ? "<truncated to last " + bound + " bytes>\n" + raw.slice(-bound) : raw;
  } catch (error) {
    return "<unreadable: " + String(error) + ">";
  }
}

/**
 * Persist a launched or adopted daemon's own output as bounded QA evidence.
 *
 * The point is attribution: when a control connection resets, the daemon's last lines and the
 * connection's own event trail are what say whether it exited, refused, or is still running.
 * This function asserts nothing about the cause - it records what was observed, and it redacts
 * anything token-shaped so a log can be kept beside other evidence safely.
 *
 * Returns the path written, or null when no evidence directory was supplied.
 */
export function persistDaemonOutput(dir, name, child, entry, exitInfo, stdout, stderr, control, daemonLogPath) {
  if (!isNonEmptyString(dir)) return null;
  const redact = (value) =>
    String(value === undefined || value === null ? "" : value)
      .replace(/"(?:token|deviceToken|machineToken|pairingToken|authorization)"\s*:\s*"[^"]*"/gi, '"redacted":"<REDACTED>"')
      .replace(/Bearer\s+[A-Za-z0-9._-]{12,}/g, "Bearer <REDACTED>");
  const lines = [
    "# launched daemon output (bounded QA evidence)",
    "pid=" + String(child && child.pid !== undefined ? child.pid : "unknown"),
    "executablePath=" + String((entry && entry.executablePath) || "unknown"),
    "exit=" + (exitInfo ? JSON.stringify(exitInfo) : "still-running-or-not-observed"),
    "diagnostics=" + (control ? JSON.stringify(control.diagnostics()) : "none"),
    "",
    "## stdout",
    redact(stdout),
    "",
    "## stderr",
    redact(stderr),
    "",
    "## daemon log (" + String(daemonLogPath || "not-configured") + ")",
    redact(readBoundedTail(daemonLogPath, 128 * 1024)),
    "",
  ];
  const target = join(dir, name);
  try {
    mkdirSync(dir, { recursive: true });
    writeFileSync(target, lines.join("\n"));
  } catch (error) {
    // Evidence that cannot be written must not replace the failure it was meant to explain.
    return "unwritable:" + String(error);
  }
  return target;
}

/**
 * The exact ownership of the daemon a launcher started: what the next process in this run
 * chain needs in order to ADOPT that daemon instead of launching another one.
 *
 * `daemonPid`/`daemonExecutablePath` are the spawn-recorded pair (the identity proof),
 * `daemonReportedPid` is the daemon's own handshake report for corroboration, and `lease` is
 * the ADOPTION deadline - how long the handoff stays adoptable. It is not a lifetime and not
 * a cleanup guarantee, so `reapRequirement` names what the verifier must do instead.
 */
function ownershipRecord(profile, transport, child, entry, url, credentialFile, control, lease) {
  return {
    contract: ISOLATED_GATEWAY_OWNERSHIP_CONTRACT,
    profileRoot: profile.root,
    runtimeDir: profile.paths.runtime,
    platform: transport.platform,
    transportKind: transport.kind,
    daemonPid: child.pid,
    daemonExecutablePath: entry.executablePath,
    daemonReportedPid: typeof control.handshake.pid === "number" ? control.handshake.pid : null,
    daemonEpoch:
      control.handshake.epoch === undefined || control.handshake.epoch === null
        ? null
        : String(control.handshake.epoch),
    url,
    credentialFile,
    lease: lease || null,
    // There is no reaper service anywhere in this harness, deliberately: a reaper is a
    // second long-lived process, and an unattended one is worse than the leftover it chases.
    // So the duty is written down instead - the verifier's own finally reaps this exact PID,
    // identity-checked against this executable path, and removes the profile root, INCLUDING
    // the case where provisioning succeeded and the runner then failed to start or adopt.
    reapRequirement: {
      by: "exact-pid",
      pid: child.pid,
      executablePath: entry.executablePath,
      profileRoot: profile.root,
      note: "no reaper service exists: if no runner adopts this daemon, the verifier must stop exactly this PID (identity-checked against this executable path) and remove the profile root",
    },
  };
}

/**
 * Adopt a daemon that another process in this run chain started, from its ownership record.
 *
 * Why adoption rather than a fresh launch: the provisioner spawns the original PTYs in the
 * daemon it started, so a runner that launched its own daemon would be asserting those
 * recorded PID/executable identities against a daemon that never spawned them - a different
 * incarnation with none of the sessions. Adoption keeps ONE daemon and ONE epoch across
 * provisioning and the scenario run.
 *
 * CLEANUP IS NOT BOUNDED BY THIS FILE. The lease is an ADOPTION deadline - how long a handoff
 * stays adoptable - not a lifetime, and nothing here starts a reaper: a reaper would be a
 * second service, and an unattended one is worse than the leftover it chases. When a runner
 * adopts, it reaps in its own cleanup. When nothing adopts, the retained daemon and its
 * profile stay until the verifier reaps them by the exact PID in the ownership record's
 * `reapRequirement` (also carried in the provisioning spawn-ledger receipt).
 *
 * Every check fails closed: a caller that did not pass `allowHost: true`, a record that is
 * incomplete, an expired lease, a PID that is no longer that executable, a daemon that is not
 * running, an epoch or address that drifted from the record, or a credential the adopted
 * daemon refuses. Ownership is PROVEN before anything is signalled; a failure before that
 * proof leaves the process alone and names the exact PID, executable and lease deadline in the
 * error detail so the verifier can reap it.
 */
export async function adoptOwnedGateway(ownership, options) {
  const settings = options || {};
  const timeoutMs = typeof settings.timeoutMs === "number" ? settings.timeoutMs : 30000;
  // AUTHORIZATION FIRST, and it is the caller's, not the file's. The ownership record comes
  // from a fixture manifest, which is DATA: a manifest that names a daemon and a lease is not
  // permission to drive or kill that daemon. Only an explicit --allow-host true does that,
  // and it is required here for the same reason it is required to launch one.
  if (settings.allowHost !== true) {
    throw gatewayBlocked(
      "owned-gateway-authorization-required",
      "adopting a retained gateway requires --allow-host true; a fixture manifest is data, not authorization",
    );
  }
  if (!ownership || typeof ownership !== "object" || ownership.contract !== ISOLATED_GATEWAY_OWNERSHIP_CONTRACT) {
    throw gatewayBlocked("owned-gateway-record-invalid", JSON.stringify(ownership));
  }
  for (const field of ["profileRoot", "runtimeDir", "platform", "daemonPid", "daemonExecutablePath", "url", "credentialFile"]) {
    if (ownership[field] === undefined || ownership[field] === null || ownership[field] === "") {
      throw gatewayBlocked("owned-gateway-record-incomplete", field);
    }
  }
  const lease = ownership.lease;
  if (!lease || typeof lease.deadlineAt !== "number") {
    throw gatewayBlocked("owned-gateway-lease-missing", "a retained daemon must carry a bounded lease");
  }
  if (Date.now() > lease.deadlineAt) {
    throw gatewayBlocked("owned-gateway-lease-expired", "deadlineAt=" + lease.deadlineAt + " now=" + Date.now());
  }
  // Identity first: the recorded PID must still BE the same executable FILE, by full
  // normalized path. A basename match would accept a different file of the same name.
  const live = probeProcessIdentity(ownership.daemonPid);
  if (!live.alive) {
    throw gatewayBlocked("owned-gateway-not-running", ownership.daemonPid + " " + ownership.daemonExecutablePath);
  }
  const identityProven = executablePathMatches(live.executable, ownership.daemonExecutablePath, ownership.platform);
  if (!identityProven) {
    throw gatewayBlocked(
      "owned-gateway-identity-mismatch",
      "pid " + ownership.daemonPid + " live=" + live.executable + " recorded=" + ownership.daemonExecutablePath,
    );
  }
  if (!existsSync(ownership.credentialFile)) {
    throw gatewayBlocked("owned-gateway-credential-missing", ownership.credentialFile);
  }

  const profile = {
    root: ownership.profileRoot,
    paths: { runtime: ownership.runtimeDir },
    cleanup() {
      rmSync(ownership.profileRoot, { recursive: true, force: true });
    },
  };
  const transport = daemonControlTransport(profile, { platform: ownership.platform });
  // The adopted daemon's own log file, inside the profile this record names. It is destroyed
  // with the profile, so it is captured before any cleanup.
  const daemonLogPath = join(ownership.profileRoot, "data", "logs", "daemon.log");
  let control = null;

  const stopAdopted = async () => {
    // In-band first: the daemon persists its remote sessions and exits 0, which closes the
    // control socket. That close is the receipt - never a poll on the process table.
    if (control) {
      control.call({ type: "shutdown" }).catch(() => null);
      try {
        await deadline(new Promise((resolve) => control.onClose(resolve)), "adopted gateway shutdown", timeoutMs);
      } catch {
        /* The daemon did not stop in band; fall through to the identity-checked kill. */
      }
    }
    const still = probeProcessIdentity(ownership.daemonPid);
    if (!still.alive) {
      return { stopped: true, graceful: true, pid: ownership.daemonPid, executablePath: ownership.daemonExecutablePath };
    }
    const matches = executablePathMatches(still.executable, ownership.daemonExecutablePath, ownership.platform);
    if (!matches) {
      return {
        stopped: false,
        graceful: false,
        killed: false,
        killSkipped: "live executable does not match the spawn-recorded executable",
        pid: ownership.daemonPid,
        executablePath: ownership.daemonExecutablePath,
        live: still,
      };
    }
    let killed = false;
    let killError = null;
    try {
      killExactPid(ownership.daemonPid);
      killed = true;
    } catch (error) {
      killError = String(error);
    }
    return { stopped: killed, graceful: false, killed, killError, pid: ownership.daemonPid, executablePath: ownership.daemonExecutablePath };
  };

  try {
    control = await connectDaemonControl(transport, { timeoutMs });
    const statusResponse = await control.call({ type: "remoteGetStatus" });
    if (!statusResponse || statusResponse.type !== "remoteStatusOk") {
      throw gatewayBlocked("owned-gateway-status-unreadable", JSON.stringify(statusResponse));
    }
    const status = statusResponse.status || null;
    if (!status || status.isRunning !== true) {
      throw gatewayBlocked("owned-gateway-not-running", JSON.stringify(status));
    }
    const bound = parseLoopbackBoundAddress(status.boundAddress);
    if (!bound) {
      throw gatewayBlocked("owned-gateway-bound-address-not-loopback", String(status.boundAddress));
    }
    const url = "http://" + bound.host + ":" + bound.port;
    const epoch =
      control.handshake.epoch === undefined || control.handshake.epoch === null
        ? null
        : String(control.handshake.epoch);
    if (ownership.daemonEpoch !== null && ownership.daemonEpoch !== epoch) {
      throw gatewayBlocked("owned-gateway-epoch-drift", "recorded=" + ownership.daemonEpoch + " live=" + epoch);
    }
    if (ownership.url !== url) {
      throw gatewayBlocked("owned-gateway-address-drift", "recorded=" + ownership.url + " live=" + url);
    }
    const token = parseDaemonTransportToken(readFileSync(ownership.credentialFile, "utf8"));
    if (token === null) {
      throw gatewayBlocked("owned-gateway-credential-empty", ownership.credentialFile);
    }
    // The adopted daemon's own auth store must accept the retained credential, exactly as the
    // launcher required of the daemon it started.
    const capabilities = await gatewayJson(
      url + REFERENCE_CAPABILITIES_PATH,
      { headers: { accept: "application/json", authorization: "Bearer " + token } },
      timeoutMs,
    );
    if (capabilities.status !== 200) {
      throw gatewayBlocked(
        "owned-gateway-token-refused",
        "GET " + REFERENCE_CAPABILITIES_PATH + " -> " + capabilities.status + " " + String(capabilities.text).slice(0, 500),
      );
    }

    return {
      contract: ISOLATED_GATEWAY_CONTRACT,
      adopted: true,
      ownership,
      profile,
      entry: {
        pid: ownership.daemonPid,
        executablePath: ownership.daemonExecutablePath,
        argv: [],
        role: "adopted-isolated-gateway",
      },
      controlTransport: transport,
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
      credentialFile: ownership.credentialFile,
      daemonTransport: {
        kind: transport.kind,
        platform: transport.platform,
        runtimeDir: transport.runtimeDir,
        endpoint: control.endpoint,
        requiresToken: transport.requiresToken,
      },
      daemonSocketPath: transport.kind === DAEMON_TRANSPORT_KINDS.unixSocket ? transport.socketPath : null,
      daemonPid: ownership.daemonPid,
      daemonEpoch: epoch,
      deviceId: null,
      devicePermission: null,
      machineId: capabilities.json && capabilities.json.machineId ? capabilities.json.machineId : null,
      referenceHostId:
        capabilities.json && capabilities.json.referenceHostId ? capabilities.json.referenceHostId : null,
      remoteStatus: status,
      stderrTail: () => "",
      // The adopted daemon's own output is not this process's pipe, but its log file is
      // reachable and is the run's record; expose it the same way a launched handle does, so
      // the caller's cleanup captures it before the profile is removed.
      evidenceName: settings.evidenceName || null,
      daemonOutput() {
        return {
          pid: ownership.daemonPid,
          executablePath: ownership.daemonExecutablePath,
          exit: null,
          stdout: "",
          stderr: "",
          diagnostics: control ? control.diagnostics() : null,
        };
      },
      persistDaemonOutput(dir, name) {
        return persistDaemonOutput(
          dir,
          name || settings.evidenceName || "daemon-output-adopted.log",
          { pid: ownership.daemonPid },
          { executablePath: ownership.daemonExecutablePath },
          null,
          "",
          "",
          control,
          daemonLogPath,
        );
      },
      stop: stopAdopted,
    };
  } catch (error) {
    // The adopted daemon's own log is captured BEFORE anything is stopped or removed: it is
    // the only record of why the adoption failed, and it lives inside the profile.
    try {
      persistDaemonOutput(
        settings.evidenceDir,
        settings.evidenceName || "daemon-output-adopted.log",
        { pid: ownership.daemonPid },
        { executablePath: ownership.daemonExecutablePath },
        null,
        "",
        "",
        control,
        daemonLogPath,
      );
    } catch {
      /* Evidence that cannot be written must not replace the failure it explains. */
    }
    // Ownership was proven before any signal: reap only then, and otherwise leave the
    // process alone while naming exactly what must be reaped and by when.
    if (identityProven) {
      const outcome = await stopAdopted().catch(() => ({ stopped: false }));
      error.detail =
        (error.detail ? error.detail + " " : "") +
        (outcome.stopped
          ? "the adopted daemon was stopped and its profile removed"
          : "profile-left-behind=" + ownership.profileRoot + " pid=" + ownership.daemonPid);
    } else {
      error.detail =
        (error.detail ? error.detail + " " : "") +
        "not-signalled pid=" + ownership.daemonPid + " executable=" + ownership.daemonExecutablePath +
        " leaseDeadlineAt=" + lease.deadlineAt;
    }
    if (control) {
      try {
        control.close();
      } catch {
        /* The socket dies with the daemon. */
      }
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



