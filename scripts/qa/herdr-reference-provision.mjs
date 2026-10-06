#!/usr/bin/env node
/**
 * Herdr reference-chat fixture and candidate producer (plan task 14).
 *
 * AUTHORED, NOT EXECUTED. The complete-code merge barrier owns every run of this file.
 *
 * WHAT IT DOES
 *   Reads an authorized provisioning config, spawns and RECORDS the original PTYs it
 *   creates, resolves each session's native transcript path from the provider/session
 *   identity the daemon actually reported, hashes the raw transcript BEFORE sanitizing it,
 *   freezes the candidate source/binary/UI provenance, and writes:
 *     <out>/fixtures.json      hosts, sessions, devices, spawn ledger, blockers
 *     <out>/candidate.json     source revision, dirty patch hash, source/binary/UI hashes
 *     <out>/spawn-ledger.json  exact PID + executable of every resource this run created
 *     <out>/blockers.json      explicit BLOCKED list (missing executable/device/driver)
 *
 * WHAT IT NEVER DOES
 *   - It never invents transcript provenance. A transcript it cannot resolve from a real
 *     provider/session identity is reported missing, and the session is marked unavailable.
 *   - It never turns a missing executable, device or driver endpoint into a PASS: those
 *     become blockers and the process exits nonzero.
 *   - It never touches a production profile. Every host it starts runs on a throwaway
 *     isolated profile.
 *   - It never kills a process it did not spawn. Teardown re-reads each recorded PID's live
 *     executable and kills only an exact match; anything else is reported.
 *
 * EXIT CODES
 *   0  manifests written, no blocker
 *   2  BLOCKED: an actual dependency is missing (executable, device, driver, transcript)
 *   3  interaction failure (a host could not be driven)
 *   4  assertion failure (a manifest invariant did not hold)
 *   5  usage error
 *   130 interrupted (teardown still runs)
 *
 * USAGE
 *   node scripts/qa/herdr-reference-provision.mjs \
 *     --config C:\ferryx-qa\herdr\hosts.json \
 *     --out C:\ferryx-qa\herdr \
 *     [--capture-native] [--allow-host true] [--timeout-ms 30000]
 */


import { createConnection } from "node:net";
import { createInterface } from "node:readline";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  CANDIDATE_SCHEMA,
  FIXTURE_SCHEMA,
  IsolatedGatewayError,
  OwnedProcessLedger,
  REFERENCE_REGISTRY_ROWS,
  REFERENCE_TRANSPORTS,
  SPAWN_LEDGER_SCHEMA,
  captureArtifact,
  captureSourceProvenance,
  daemonSocketPath,
  killExactPid,
  launchIsolatedGateway,
  probeProcessIdentity,
  redactUrl,
  repoRoot,
  sha256Bytes,
  sha256File,
  validateRegistryRows,
  writeJson,
} from "./herdr-reference-fixtures.mjs";
import {
  PTY_IDENTITY_SCHEMA,
  bindHostSuppliedIdentity,
  clearPtyIdentity,
  readPtyIdentity,
  validateOwnerHostSpawnReceipt,
  validatePtyIdentity,
  writePtyIdentity,
  writePtyIdentityWrapper,
} from "./herdr-reference-pty-identity.mjs";

const SCRIPT_ID = "herdr-reference-provision.mjs/1.0.0";

/** The daemon protocol version this producer speaks. Mirrors daemon/protocol.rs. */
const DAEMON_PROTOCOL_VERSION = 5;

/** The sanitizer version recorded beside every sanitized capture. */
const SANITIZER_VERSION = "herdr-reference-sanitize/1";

const EXIT = { OK: 0, BLOCKED: 2, INTERACTION: 3, ASSERTION: 4, USAGE: 5, INTERRUPT: 130 };

class ProvisionError extends Error {
  constructor(code, reason, detail) {
    super(reason + (detail ? ": " + detail : ""));
    this.code = code;
    this.reason = reason;
    this.detail = detail || "";
  }
}

const blocked = (reason, detail) => new ProvisionError(EXIT.BLOCKED, reason, detail);
const usage = (message) => {
  process.stderr.write(
    "USAGE ERROR: " + message + "\n\n" +
      "  node scripts/qa/herdr-reference-provision.mjs --config <hosts.json> --out <dir>\n" +
      "      [--capture-native] [--allow-host true] [--timeout-ms <ms>]\n",
  );
  process.exit(EXIT.USAGE);
};

/* ==========================================================================
 * CLI
 * ========================================================================== */

function parseArgs(argv) {
  const args = { captureNative: false, allowHost: false, timeoutMs: 30000 };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    const next = () => {
      i += 1;
      if (i >= argv.length) usage(flag + " requires a value");
      return argv[i];
    };
    switch (flag) {
      case "--config": args.config = next(); break;
      case "--out": args.out = next(); break;
      case "--capture-native": args.captureNative = true; break;
      case "--allow-host": args.allowHost = next() === "true"; break;
      case "--timeout-ms": args.timeoutMs = Number.parseInt(next(), 10); break;
      case "-h":
      case "--help":
        process.stdout.write(readFileSync(new URL(import.meta.url), "utf8").split("*/")[0] + "*/\n");
        process.exit(EXIT.OK);
        break;
      default:
        usage("unknown flag " + flag);
    }
  }
  if (!args.config) usage("--config is mandatory (authorized isolated host endpoints)");
  if (!args.out) usage("--out is mandatory (where fixtures.json and candidate.json are written)");
  if (!Number.isFinite(args.timeoutMs) || args.timeoutMs <= 0) usage("--timeout-ms must be a positive integer");
  return args;
}

/* ==========================================================================
 * Daemon UDS client (newline-delimited JSON, protocol v5)
 * ========================================================================== */

/**
 * One connection to a daemon's control socket. Requests are newline-delimited JSON; the
 * response for a request is the next JSON line that is not a stream frame.
 */
function connectDaemon(socketPath, token) {
  return new Promise((resolveConnect, rejectConnect) => {
    const socket = createConnection({ path: socketPath });
    const lines = createInterface({ input: socket });
    const pending = [];
    const streams = [];
    lines.on("line", (line) => {
      let message;
      try {
        message = JSON.parse(line);
      } catch {
        return;
      }
      if (message.type === "stream" || message.type === "output" || message.type === "terminalOutput") {
        for (const listener of streams) listener(message);
        return;
      }
      const next = pending.shift();
      if (next) next(message);
    });
    socket.on("error", rejectConnect);
    const call = (payload) =>
      new Promise((resolveCall) => {
        pending.push(resolveCall);
        socket.write(JSON.stringify(payload) + "\n");
      });
    socket.on("connect", async () => {
      const handshake = await call({ type: "handshake", version: DAEMON_PROTOCOL_VERSION, ...(token ? { token } : {}) });
      if (!handshake || handshake.type !== "handshakeOk") {
        socket.destroy();
        rejectConnect(new Error("daemon handshake refused: " + JSON.stringify(handshake)));
        return;
      }
      resolveConnect({
        handshake,
        call,
        onStream(listener) { streams.push(listener); },
        close() {
          lines.close();
          socket.destroy();
        },
      });
    });
  });
}

/* ==========================================================================
 * Config
 * ========================================================================== */

function loadConfig(path) {
  if (!existsSync(path)) throw blocked("config-missing", path);
  let parsed;
  try {
    parsed = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    throw new ProvisionError(EXIT.USAGE, "config-unreadable", String(error));
  }
  return parsed;
}

/**
 * Collect every dependency this run actually needs, and report the ones that are absent.
 * A missing executable, device or driver is a BLOCKED entry, never a skipped scenario.
 */
function preflight(config) {
  const blockers = [];
  const checks = [];

  const sourceRoot = (config.source && config.source.root) || repoRoot;
  if (!existsSync(sourceRoot)) {
    blockers.push({ kind: "source-root", detail: sourceRoot, reason: "candidate source root does not exist" });
  }
  checks.push({ kind: "source-root", path: sourceRoot, present: existsSync(sourceRoot) });

  const candidate = config.candidate || {};
  for (const [kind, path] of [["gateway-binary", candidate.binary], ["ui-dist", candidate.uiDist]]) {
    const present = typeof path === "string" && path.length > 0 && existsSync(path);
    checks.push({ kind, path: path || null, present });
    if (!present) {
      blockers.push({ kind, detail: path || null, reason: "required composed artifact is absent" });
    }
  }

  const providers = Array.isArray(config.providers) ? config.providers : [];
  const providerIds = new Set(providers.map((provider) => provider.registryId));
  const requiredRows = REFERENCE_REGISTRY_ROWS.map((row) => row.id);
  for (const row of requiredRows) {
    const provider = providers.find((entry) => entry.registryId === row);
    if (!provider) {
      // A registry row with no configured executable cannot be certified as a live provider.
      blockers.push({
        kind: "provider-executable",
        registryId: row,
        reason: "no executable path and version configured for this registry id",
      });
      continue;
    }
    const present = typeof provider.executablePath === "string" && existsSync(provider.executablePath);
    checks.push({ kind: "provider-executable", registryId: row, path: provider.executablePath || null, present });
    if (!present) {
      blockers.push({
        kind: "provider-executable",
        registryId: row,
        detail: provider.executablePath || null,
        reason: "configured provider executable does not exist",
      });
    }
  }
  void providerIds;

  const devices = Array.isArray(config.devices) ? config.devices : [];
  for (const device of devices) {
    const present = typeof device.driverEndpoint === "string" && device.driverEndpoint.trim().length > 0;
    checks.push({ kind: "device-driver", platform: device.platform, serial: device.serial, present });
    if (!present) {
      blockers.push({
        kind: "device-driver",
        platform: device.platform,
        serial: device.serial || null,
        reason: "no driver endpoint configured for this device",
      });
    }
  }
  if (!devices.some((device) => device.platform === "android")) {
    blockers.push({ kind: "device", platform: "android", reason: "no Android device is provisioned" });
  }
  if (!devices.some((device) => device.platform === "ios")) {
    blockers.push({ kind: "device", platform: "ios", reason: "no iOS device is provisioned" });
  }

  const hosts = Array.isArray(config.hosts) ? config.hosts : [];
  if (hosts.length === 0) {
    blockers.push({ kind: "host", reason: "no isolated host is configured" });
  }
  const transports = new Set(hosts.map((host) => host.transport));
  for (const transport of REFERENCE_TRANSPORTS) {
    if (!transports.has(transport)) {
      blockers.push({ kind: "host-transport", transport, reason: "no isolated host is configured for this transport" });
    }
  }

  return { blockers, checks };
}

/* ==========================================================================
 * Native capture
 * ========================================================================== */

/**
 * The transcript store roots, per provider, from the config. The producer resolves a
 * session's transcript ONLY inside its own provider's store; it never falls back to
 * "the newest file in the directory", which is how one pane's conversation gets served
 * for another's.
 */
function transcriptStoreFor(config, registryId) {
  const stores = (config.nativeStores || []).find((store) => store.registryId === registryId);
  return stores ? stores.root : null;
}

/**
 * Resolve the exact native transcript for one session.
 *
 * Identity first: the daemon-reported providerSessionId names the record. A path the config
 * already pinned is accepted only when it sits inside that provider's own store root.
 */
function resolveNativeTranscript(config, session) {
  const store = transcriptStoreFor(config, session.registryId);
  if (typeof session.transcriptPath === "string" && session.transcriptPath.length > 0) {
    if (!existsSync(session.transcriptPath)) {
      return { resolved: false, reason: "configured transcript path does not exist", path: session.transcriptPath };
    }
    if (store && !resolve(session.transcriptPath).startsWith(resolve(store))) {
      return {
        resolved: false,
        reason: "configured transcript path is outside its provider's own store",
        path: session.transcriptPath,
        store,
      };
    }
    return { resolved: true, path: session.transcriptPath, source: "config" };
  }
  if (!store || !existsSync(store)) {
    return { resolved: false, reason: "no native store root is configured for this registry id", store: store || null };
  }
  if (typeof session.providerSessionId !== "string" || session.providerSessionId.length === 0) {
    return {
      resolved: false,
      reason: "the session has no providerSessionId, so no exact transcript can be identified",
      store,
    };
  }
  const candidates = [
    join(store, session.providerSessionId + ".jsonl"),
    join(store, session.providerSessionId),
  ];
  for (const candidate of candidates) {
    if (existsSync(candidate)) return { resolved: true, path: candidate, source: "provider-session-id" };
  }
  return {
    resolved: false,
    reason: "no transcript for that provider session id exists in its own store",
    store,
    providerSessionId: session.providerSessionId,
  };
}

/**
 * Sanitize a raw transcript for evidence. The RAW hash is recorded before this runs; the
 * sanitized copy records what was removed so a reader can tell sanitized bytes from source
 * bytes.
 */
function sanitizeTranscript(raw) {
  const text = raw.toString("utf8");
  const home = process.env.HOME || process.env.USERPROFILE || "";
  let sanitized = text;
  const applied = [];
  if (home.length > 2) {
    const before = sanitized;
    sanitized = sanitized.split(home).join("<HOME>");
    if (sanitized !== before) applied.push("home-path");
  }
  const tokenPatterns = [
    [/"(?:token|deviceToken|machineToken|pairingToken|apiKey|authorization)"\s*:\s*"[^"]*"/gi, "\"redacted\":\"<REDACTED>\""],
    [/Bearer\s+[A-Za-z0-9._-]{12,}/g, "Bearer <REDACTED>"],
  ];
  for (const [pattern, replacement] of tokenPatterns) {
    const before = sanitized;
    sanitized = sanitized.replace(pattern, replacement);
    if (sanitized !== before) applied.push("secret");
  }
  return { sanitized, applied };
}

/**
 * Capture one session's native evidence: raw hash first, then the sanitized copy. The raw
 * bytes are never written to the evidence directory — only their hash and length are.
 */
function captureNativeSession(config, session, outDir) {
  const resolution = resolveNativeTranscript(config, session);
  if (!resolution.resolved) {
    return {
      resolved: false,
      reason: resolution.reason,
      transcriptPath: null,
      transcriptSha256: null,
      store: resolution.store || null,
    };
  }
  const raw = readFileSync(resolution.path);
  const rawSha256 = sha256Bytes(raw);
  const rawBytes = raw.length;
  const version = readTranscriptVersion(config, session);
  const directory = join(outDir, "native", session.hostId, session.backendSessionId);
  mkdirSync(directory, { recursive: true });
  const { sanitized, applied } = sanitizeTranscript(raw);
  const sanitizedPath = join(directory, "sanitized.jsonl");
  writeFileSync(sanitizedPath, sanitized);
  const record = {
    resolved: true,
    source: resolution.source,
    transcriptPath: resolution.path,
    transcriptSha256: rawSha256,
    transcriptBytes: rawBytes,
    sanitizedPath,
    sanitizedSha256: sha256Bytes(Buffer.from(sanitized, "utf8")),
    sanitizerVersion: SANITIZER_VERSION,
    sanitizerApplied: applied,
    transcriptVersion: version,
    capturedAt: new Date().toISOString(),
  };
  writeJson(join(directory, "capture.json"), record);
  return record;
}

/** The provider's own version string, read from the configured provider entry. */
function readTranscriptVersion(config, session) {
  const provider = (config.providers || []).find((entry) => entry.registryId === session.registryId);
  if (provider && typeof provider.version === "string") return provider.version;
  return null;
}

/* ==========================================================================
 * Host provisioning
 * ========================================================================== */

/**
 * Start one isolated gateway for a host through the product's own headless launch
 * contract (--daemon + a persisted isolated remote config, the bound address read back
 * from that daemon, and a token from its own auth store). Nothing here reuses a
 * production data directory, and nothing waits for a JSON ready line the binary never
 * writes.
 */
async function startIsolatedGateway(config, host, ledger, args) {
  try {
    return await launchIsolatedGateway({
      binary: config.candidate && config.candidate.binary,
      label: host.id,
      uiDist: config.candidate && config.candidate.uiDist,
      env: host.env || {},
      cwd: config.source && config.source.root ? config.source.root : repoRoot,
      ledger,
      timeoutMs: args.timeoutMs,
      extra: { hostId: host.id, transport: host.transport, launchedBy: "herdr-reference-provision.mjs" },
    });
  } catch (error) {
    if (error instanceof IsolatedGatewayError) throw blocked(error.reason, error.detail);
    throw error;
  }
}

/**
 * Spawn one original PTY through the host's own daemon and record the exact PID and
 * executable. The session record the daemon returns is the authority for the geometry the
 * pane actually has; a mismatch with the requested geometry is reported, not smoothed over.
 *
 * The PTY child is a generated recording wrapper, launched through the daemon's supported
 * \`shell\` preference, so the identity recorded is the one this run itself created. There is
 * deliberately NO process-table or descendant scan anywhere on this path: a scan is not
 * ownership proof and cannot distinguish a recycled PID from the original session.
 */
async function spawnOriginalPty(host, session, ledger, args) {
  const socketPath = host.daemon && host.daemon.socketPath
    ? host.daemon.socketPath
    : (host.daemon && host.daemon.runtimeDir
      ? daemonSocketPath({ paths: { runtime: host.daemon.runtimeDir } })
      : null);
  if (!socketPath) throw blocked("daemon-socket-unknown", host.id);
  if (!existsSync(socketPath)) {
    throw blocked("daemon-socket-missing", host.id + " -> " + socketPath);
  }
  const realShell = session.realShell || host.realShell || null;
  if (!realShell) {
    throw blocked(
      "real-shell-unknown",
      "no real shell is configured for " + host.id + "; the wrapper must exec a known shell",
    );
  }
  // The wrapper is generated per session under this run's own output directory, and any
  // record from a previous run is removed first so a stale identity can never be read as
  // this run's.
  //
  // NOTE ON A KNOWN HAZARD: a client-side cached default_shell can fill a spawn IPC and
  // override a pane's requested shell. This producer talks to the daemon UDS directly and
  // sets \`shell\` on the request itself, with no WebView in the path, so no cached default can
  // intervene here. A host that drives the UI instead must verify the shell it got back.
  const wrapperDir = join(resolve(args.out), "pty-identity", String(session.backendSessionId || session.clientRequestId || "session"));
  const identityPath = join(wrapperDir, "identity.json");
  clearPtyIdentity(identityPath);
  const wrapper = writePtyIdentityWrapper({
    platform: host.platform || (process.platform === "win32" ? "windows" : "unix"),
    dir: wrapperDir,
    outputPath: identityPath,
    realShell,
    loginArgs: session.loginArgs,
    providerCommand: session.providerCommand || null,
  });
  const token = host.daemon && host.daemon.token ? host.daemon.token : undefined;
  const client = await connectDaemon(socketPath, token);
  try {
    const requestId = session.clientRequestId || ("herdr-ref-" + session.backendSessionId);
    const response = await client.call({
      type: "spawn",
      clientRequestId: requestId,
      workspaceId: session.workspaceId,
      worktree: session.worktree || null,
      cwd: session.cwd || null,
      cols: session.cols,
      rows: session.rows,
      // The wrapper IS the shell the daemon launches; resolve_shell_command_pure uses a
      // non-empty preference as the program verbatim, so the wrapper becomes the direct PTY
      // child and records its own identity before becoming the real shell.
      shell: wrapper.wrapperPath,
      startup: session.startup || null,
    });
    if (!response || response.type !== "spawnOk") {
      throw new ProvisionError(EXIT.INTERACTION, "spawn-refused", JSON.stringify(response));
    }
    const details = response.session || {};
    if (details.cols !== session.cols || details.rows !== session.rows) {
      throw new ProvisionError(
        EXIT.ASSERTION,
        "spawn-geometry-mismatch",
        "requested " + session.cols + "x" + session.rows + ", daemon reports " + details.cols + "x" + details.rows,
      );
    }
    const daemonPid = client.handshake.pid;
    const record = await readPtyIdentity(identityPath, args.timeoutMs);
    const validated = validatePtyIdentity(record, { shell: realShell }, probeProcessIdentity);
    if (!validated.ok) {
      throw new ProvisionError(EXIT.ASSERTION, "pty-identity-invalid", validated.errors.join("; "));
    }
    // The daemon PID is recorded for context only. It is never used as the shell PID.
    ledger.recordPid(record.ptyChild.pid, record.ptyChild.executable, {
      hostId: host.id,
      backendSessionId: response.sessionId,
      role: "pty-child-wrapper",
    });
    return {
      backendSessionId: response.sessionId,
      epoch: String(response.epoch),
      daemonPid,
      pid: record.ptyChild.pid,
      executablePath: record.ptyChild.executable,
      ptyIdentity: record,
      ptyIdentityPath: identityPath,
      cols: details.cols,
      rows: details.rows,
      running: details.running,
    };
  } finally {
    client.close();
  }
}

/* ==========================================================================
 * Candidate provenance
 * ========================================================================== */

function buildCandidateManifest(config, outDir) {
  const candidate = config.candidate || {};
  const sourceRoot = (config.source && config.source.root) || repoRoot;
  const sourcePaths = (config.source && config.source.paths) || [];
  const provenance = captureSourceProvenance(sourceRoot, sourcePaths, candidate.toolVersions || {});
  const binary = candidate.binary ? captureArtifact(candidate.binary, "gateway-binary") : { path: null, sha256: null, missing: true };
  const uiDist = candidate.uiDist || null;
  const uiFiles = [];
  if (uiDist && existsSync(uiDist)) {
    for (const relative of listUiFiles(uiDist)) {
      const absolute = join(uiDist, relative);
      uiFiles.push({ path: relative, sha256: sha256File(absolute), sizeBytes: statSync(absolute).size });
    }
  }
  const manifest = {
    schema: CANDIDATE_SCHEMA,
    scriptId: SCRIPT_ID,
    candidateId: sha256Bytes(Buffer.from(String(provenance.sourceRevision) + "|" + provenance.dirtyPatchSha256 + "|" + String(binary.sha256), "utf8")),
    sourceRevision: provenance.sourceRevision,
    revisionReadable: provenance.revisionReadable,
    dirty: provenance.dirty,
    dirtyPatchSha256: provenance.dirtyPatchSha256,
    dirtyStatusSha256: provenance.dirtyStatusSha256,
    sourceFiles: provenance.sourceFiles,
    binary: { path: binary.path, sha256: binary.sha256, sizeBytes: binary.sizeBytes || null },
    uiDist,
    uiFiles,
    buildCommand: candidate.buildCommand || null,
    buildExit: candidate.buildExit === undefined ? null : candidate.buildExit,
    toolVersions: candidate.toolVersions || {},
    capturedAt: new Date().toISOString(),
  };
  writeJson(join(outDir, "candidate.json"), manifest);
  return manifest;
}

function listUiFiles(root) {
  const found = [];
  const queue = [""];
  while (queue.length > 0) {
    const relative = queue.shift();
    const absolute = relative.length === 0 ? root : join(root, relative);
    let entries;
    try {
      entries = readdirSync(absolute, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      const child = relative.length === 0 ? entry.name : relative + "/" + entry.name;
      if (entry.isDirectory()) queue.push(child);
      else if (entry.isFile()) found.push(child);
    }
  }
  return found.sort();
}

/* ==========================================================================
 * Main
 * ========================================================================== */

async function main() {
  const args = parseArgs(process.argv.slice(2));
  mkdirSync(args.out, { recursive: true });

  const registryCheck = validateRegistryRows();
  if (!registryCheck.ok) {
    throw new ProvisionError(EXIT.ASSERTION, "registry-matrix-invalid", registryCheck.errors.join("; "));
  }

  const config = loadConfig(args.config);
  const { blockers, checks } = preflight(config);

  // The candidate provenance is frozen even when the run is blocked: a blocked run still
  // records exactly which source and binary it would have certified.
  const candidateManifest = buildCandidateManifest(config, args.out);
  // The hash of the candidate manifest as written, so an owner-host spawn receipt can be bound
  // to the exact candidate this provisioning run froze rather than to a lookalike.
  const candidateManifestPath = join(args.out, "candidate.json");
  const candidateManifestSha256 = existsSync(candidateManifestPath) ? sha256File(candidateManifestPath) : null;

  const ledger = new OwnedProcessLedger("herdr-reference-provision");
  const hosts = [];
  const sessionRecords = [];
  const captureRecords = [];
  const startedGateways = [];
  let interrupted = false;

  const onSignal = () => {
    interrupted = true;
  };
  process.on("SIGINT", onSignal);
  process.on("SIGTERM", onSignal);

  try {
    const configuredHosts = Array.isArray(config.hosts) ? config.hosts : [];
    for (const host of configuredHosts) {
      const record = {
        id: host.id,
        transport: host.transport,
        url: host.gatewayUrl || null,
        urlRedacted: host.gatewayUrl ? redactUrl(host.gatewayUrl) : null,
        credentialFile: host.credentialFile || null,
        daemonEpoch: null,
        daemonPid: null,
        started: false,
      };
      // Only a local host is started here. ssh/paired/account-relay hosts are already
      // running elsewhere and are referenced by URL + credential; starting a second copy
      // would be a different machine's gateway, not the one the evidence must bind.
      if (host.transport === "local" && !args.allowHost) {
        blockers.push({
          kind: "host-start",
          hostId: host.id,
          reason: "starting a local gateway requires explicit --allow-host true authorization",
        });
      }
      if (host.transport === "local" && args.allowHost && config.candidate && config.candidate.binary) {
        // A launch that cannot honour the contract is a blocker with the launcher's own
        // reason, never a silently unstarted host.
        try {
          const gateway = await startIsolatedGateway(config, host, ledger, args);
          startedGateways.push(gateway);
          record.started = true;
          // Recorded as EVIDENCE of this launch, not as the fixture's host url: this
          // gateway lives only for the provisioning run and is stopped when the run
          // ends, so a fixture pointing at it would hand the runner a dead URL. The
          // runner launches its own (host.startLocal) or is given an external host.
          record.gateway = {
            contract: gateway.contract,
            urlRedacted: redactUrl(gateway.url),
            boundAddress: gateway.boundAddress,
            pinnedPort: gateway.pinnedPort,
            portRequirement: gateway.portRequirement,
            pid: gateway.entry.pid,
            executablePath: gateway.entry.executablePath,
            daemonSocketPath: gateway.daemonSocketPath,
            daemonPid: gateway.daemonPid,
            daemonEpoch: gateway.daemonEpoch,
            devicePermission: gateway.devicePermission,
            referenceHostId: gateway.referenceHostId,
          };
        } catch (error) {
          if (!(error instanceof ProvisionError)) throw error;
          blockers.push({
            kind: "gateway-launch",
            hostId: host.id,
            transport: host.transport,
            reason: error.reason,
            detail: error.detail,
          });
        }
      }
      hosts.push(record);
    }

    // The daemon epoch and identity of every host are read from that host's own daemon.
    for (const host of configuredHosts) {
      const record = hosts.find((entry) => entry.id === host.id);
      const socketPath = host.daemon && host.daemon.socketPath ? host.daemon.socketPath : null;
      if (!socketPath || !existsSync(socketPath)) continue;
      const client = await connectDaemon(socketPath, host.daemon.token);
      try {
        record.daemonEpoch = String(client.handshake.epoch);
        record.daemonPid = client.handshake.pid;
      } finally {
        client.close();
      }
    }

    // Sessions: the authoritative target tuple is assembled from the host plus the daemon's
    // own report. A field the harness cannot obtain is recorded as missing.
    for (const host of configuredHosts) {
      const hostRecord = hosts.find((entry) => entry.id === host.id);
      for (const session of host.sessions || []) {
        const row = {
          hostId: host.id,
          ownerId: session.ownerId || null,
          epoch: hostRecord.daemonEpoch || session.epoch || null,
          backendSessionId: session.backendSessionId || null,
          provider: session.provider || null,
          registryId: session.registryId || session.provider || null,
          // The objective names this identity `nativeSessionId`; the wire target calls it
          // `providerSessionId`. Both are written from the same daemon-reported value, and
          // neither is defaulted: absent means *unknown*, never "use another session".
          providerSessionId: session.providerSessionId || session.nativeSessionId || null,
          nativeSessionId: session.nativeSessionId || session.providerSessionId || null,
          pid: session.pid || null,
          executablePath: session.executablePath || null,
          cols: session.cols || null,
          rows: session.rows || null,
          transcriptPath: null,
          transcriptSha256: null,
          transport: host.transport,
          missingIdentity: [],
        };
        for (const field of ["hostId", "ownerId", "epoch", "backendSessionId", "registryId", "pid", "executablePath", "cols", "rows"]) {
          if (row[field] === null || row[field] === "") row.missingIdentity.push(field);
        }
        if (row.providerSessionId === null) row.missingIdentity.push("providerSessionId");

        // A host that asks for spawning has its original PTY created HERE, through its own
        // daemon, so the PID and executable in the manifest are the ones this run created
        // and can prove ownership of. A host that already runs its own session supplies the
        // PID instead, and this producer never invents one.
        if (host.spawnSessions === true && row.pid === null) {
          const spawned = await spawnOriginalPty(host, {
            ...session,
            cols: session.cols,
            rows: session.rows,
          }, ledger, args);
          row.backendSessionId = spawned.backendSessionId;
          row.epoch = spawned.epoch;
          row.pid = spawned.pid;
          row.executablePath = spawned.executablePath;
          row.cols = spawned.cols;
          row.rows = spawned.rows;
          row.daemonPid = spawned.daemonPid;
          row.spawnedByThisRun = true;
          row.ptyIdentity = spawned.ptyIdentity;
          row.ptyIdentityPath = spawned.ptyIdentityPath;
          row.missingIdentity = row.missingIdentity.filter((field) =>
            !["backendSessionId", "epoch", "pid", "executablePath", "cols", "rows"].includes(field));
        }

        // A session this run did not spawn (ssh, paired, account-relay) must be bound to the
        // OWNING HOST's own spawn receipt. A pid that merely appears in the config is not
        // ownership proof: it could be stale, belong to another session, or have been typed by
        // hand. So the receipt is mandatory, it must agree with this run on host, transport,
        // session, incarnation and candidate provenance, and an absent or mismatched receipt is
        // a BLOCKER rather than a stamped identity.
        if (!row.ptyIdentity) {
          const receiptPath = session.spawnReceiptPath || host.spawnReceiptPath || null;
          if (!receiptPath) {
            blockers.push({
              kind: "owner-host-spawn-receipt",
              hostId: host.id,
              transport: host.transport,
              backendSessionId: row.backendSessionId,
              reason:
                "no owner-host spawn receipt is configured for this externally spawned session; " +
                "an opaque pid from the config is not ownership proof",
            });
          } else if (!existsSync(receiptPath)) {
            blockers.push({
              kind: "owner-host-spawn-receipt",
              hostId: host.id,
              transport: host.transport,
              backendSessionId: row.backendSessionId,
              detail: receiptPath,
              reason: "the configured owner-host spawn receipt does not exist",
            });
          } else {
            const receipt = JSON.parse(readFileSync(receiptPath, "utf8"));
            const receiptCheck = validateOwnerHostSpawnReceipt(receipt, {
              hostId: host.id,
              transport: host.transport,
              backendSessionId: row.backendSessionId,
              epoch: row.epoch,
              candidateId: candidateManifest.candidateId,
              sourceManifestSha256: candidateManifestSha256,
            });
            if (!receiptCheck.ok) {
              blockers.push({
                kind: "owner-host-spawn-receipt",
                hostId: host.id,
                transport: host.transport,
                backendSessionId: row.backendSessionId,
                detail: receiptPath,
                reason: "owner-host spawn receipt rejected: " + receiptCheck.errors.join("; "),
              });
            } else {
              row.ptyIdentity = bindHostSuppliedIdentity({
                receipt,
                receiptPath,
                hostId: host.id,
                transport: host.transport,
                backendSessionId: row.backendSessionId,
                epoch: row.epoch,
                candidateId: candidateManifest.candidateId,
                platform: host.platform,
                providerCommand: session.providerCommand || null,
                cols: row.cols,
                rows: row.rows,
              });
              row.ptyIdentitySource = "owner-host-spawn-receipt";
              row.spawnReceiptPath = receiptPath;
            }
          }
        }

        if (args.captureNative) {
          const capture = captureNativeSession(config, row, args.out);
          captureRecords.push({ hostId: host.id, backendSessionId: row.backendSessionId, ...capture });
          if (capture.resolved) {
            row.transcriptPath = capture.transcriptPath;
            row.transcriptSha256 = capture.transcriptSha256;
          } else {
            row.transcriptUnavailableReason = capture.reason;
          }
        }
        sessionRecords.push(row);
      }
    }

    // If a host has no daemon socket, its sessions cannot be spawned or identified; that is
    // an explicit blocker rather than an empty session list.
    if (sessionRecords.length === 0) {
      blockers.push({ kind: "session", reason: "no original session could be identified on any configured host" });
    }

    const fixtureManifest = {
      schema: FIXTURE_SCHEMA,
      scriptId: SCRIPT_ID,
      producedAt: new Date().toISOString(),
      captureNative: args.captureNative,
      hosts,
      sessions: sessionRecords,
      devices: (config.devices || []).map((device) => ({
        platform: device.platform,
        serial: device.serial || null,
        driverEndpoint: device.driverEndpoint || null,
        model: device.model || null,
        osVersion: device.osVersion || null,
      })),
      nativeCaptures: captureRecords,
      preflightChecks: checks,
      blockers,
      spawnLedger: {
        schema: SPAWN_LEDGER_SCHEMA,
        role: ledger.role,
        entries: ledger.entries,
        receipts: [],
      },
    };
    writeJson(join(args.out, "fixtures.json"), fixtureManifest);
    writeJson(join(args.out, "blockers.json"), { schema: FIXTURE_SCHEMA, blockers });
    void candidateManifest;

    if (interrupted) throw new ProvisionError(EXIT.INTERRUPT, "interrupted");
    if (blockers.length > 0) {
      process.stderr.write(
        "BLOCKED: " + blockers.length + " missing dependency(ies)\n" +
          blockers.map((entry) => "  - " + entry.kind + ": " + entry.reason + (entry.detail ? " (" + entry.detail + ")" : "")).join("\n") + "\n",
      );
      return EXIT.BLOCKED;
    }
    process.stdout.write(
      "PROVISIONED " + sessionRecords.length + " session(s) across " + hosts.length + " host(s); " +
        "wrote fixtures.json, candidate.json, spawn-ledger.json\n",
    );
    return EXIT.OK;
  } finally {
    // Teardown is paired and recorded: every gateway this run started is stopped, and the
    // ledger kills only PIDs whose live executable still matches the spawn-recorded one.
    for (const gateway of startedGateways) {
      try {
        await gateway.stop();
      } catch {
        /* The ledger below is the authority for the receipt. */
      }
      try {
        gateway.profile.cleanup();
      } catch {
        /* A leftover temp profile is reported by the receipt's entry list, not hidden. */
      }
    }
    const receipt = ledger.teardown(killExactPid);
    writeJson(join(args.out, "spawn-ledger.json"), receipt);
    for (const entry of receipt.receipts) {
      if (entry.action === "reported-not-killed") {
        process.stderr.write(
          "REPORTED (not killed): pid " + entry.pid + " live=" + entry.liveExecutable +
            " recorded=" + entry.recordedExecutablePath + "\n",
        );
      }
    }
  }
}

main()
  .then((code) => process.exit(code))
  .catch((error) => {
    const code = error instanceof ProvisionError ? error.code : EXIT.INTERACTION;
    process.stderr.write((error.reason || "provision-failed") + ": " + (error.detail || error.message) + "\n");
    process.exit(code);
  });

