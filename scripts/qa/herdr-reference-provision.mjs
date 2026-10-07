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
 *     isolated profile, and a configured host's daemon is reached only through the
 *     runtimeDir that host declares (see the configured-host contract below). A declared
 *     production runtime directory is refused without an explicit opt-in.
 *   - It never assumes a control transport. A host's daemon is dialled over the transport
 *     the product publishes on that platform - a unix socket, or on Windows the loopback
 *     daemon.port/daemon.token pair whose token the first frame must carry.
 *   - It never manufactures evidence. When a spawned session's identity does not appear, the
 *     run records only what the daemon already holds (its session details and the attach
 *     replay) with read-only requests, bounded and redacted. It does not write to the PTY,
 *     answer a terminal query, extend a deadline, or turn the failure into a success.
 *   - It never lets a diagnostic outlive its purpose. The capture runs under one total bound and
 *     is cancelled by closing the owned connection when that bound expires, so a daemon that
 *     accepts a request without answering cannot turn a bounded identity failure into a hang.
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


import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { spawn } from "node:child_process";
import { basename, dirname, join, resolve } from "node:path";
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
  DIAGNOSTIC_CAPTURE_TIMEOUT_MS,
  captureSessionDiagnostics,
  connectDaemonControl,
  daemonControlTransport,
  daemonRequestFailure,
  describeDaemonRequestFailure,
  hostAccessContract,
  killExactPid,
  launchIsolatedGateway,
  ownerHostReceiptRequirement,
  registerWorkspaceOnDaemon,
  workspaceRegistrationFor,
  probeProcessIdentity,
  redactUrl,
  repoRoot,
  sha256Bytes,
  sha256File,
  summarizeSessionDiagnostics,
  validateRegistryRows,
  writeJson,
} from "./herdr-reference-fixtures.mjs";
import {
  PTY_IDENTITY_SCHEMA,
  HOST_SUPPLIED_SOURCE_KIND,
  bindHostSuppliedIdentity,
  clearPtyIdentity,
  readPtyIdentity,
  validateOwnerHostSpawnReceipt,
  validatePtyIdentity,
  writePtyIdentity,
  writePtyIdentityWrapper,
} from "./herdr-reference-pty-identity.mjs";

const SCRIPT_ID = "herdr-reference-provision.mjs/1.0.0";

/** The sanitizer version recorded beside every sanitized capture. */
const SANITIZER_VERSION = "herdr-reference-sanitize/1";

const EXIT = { OK: 0, BLOCKED: 2, INTERACTION: 3, ASSERTION: 4, USAGE: 5, INTERRUPT: 130 };

export class ProvisionError extends Error {
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
      "      [--capture-native] [--allow-host true] [--timeout-ms <ms>]\n" +
      "      [--retain-gateway true --retain-deadline-ms <ms>]\n",
  );
  process.exit(EXIT.USAGE);
};

/* ==========================================================================
 * CLI
 * ========================================================================== */

function parseArgs(argv) {
  const args = {
    captureNative: false,
    allowHost: false,
    timeoutMs: 30000,
    retainGateway: false,
    retainDeadlineMs: null,
  };
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
      case "--retain-gateway": args.retainGateway = next() === "true"; break;
      case "--retain-deadline-ms": args.retainDeadlineMs = Number.parseInt(next(), 10); break;
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
  // Retention is a HANDOFF, never an unbounded leak: a daemon kept for the scenario runner
  // must carry a deadline, and the runner refuses an expired lease.
  if (args.retainGateway && (!Number.isFinite(args.retainDeadlineMs) || args.retainDeadlineMs <= 0)) {
    usage("--retain-gateway requires --retain-deadline-ms (a retained daemon must be bounded)");
  }
  return args;
}

/* ==========================================================================
 * Daemon UDS client (newline-delimited JSON, protocol v5)
 * ========================================================================== */

/* ==========================================================================
 * Configured-host daemon transport
 *
 * A host declares the daemon it owns; the transport it publishes is the product's own
 * per-platform pair (see the transport section of herdr-reference-fixtures.mjs): a unix
 * socket, or on Windows the loopback `daemon.port` plus the per-boot `daemon.token` the
 * first frame must carry. This producer therefore speaks BOTH through the one shared
 * client, `connectDaemonControl`, instead of dialling a socket path itself - dialling
 * `daemon.sock` on Windows is exactly the defect that made every Windows provisioning run
 * die with a bare ENOENT on a file the product never creates.
 *
 * CONFIGURED-HOST CONTRACT (no inferred fallback):
 *   host.daemon.runtimeDir   REQUIRED. The daemon runtime directory this run owns, on the
 *                            host the config names. There is no default and no discovery:
 *                            a host that declares no runtimeDir has no reachable daemon.
 *   host.daemon.platform     Optional "win32" | "linux" | "darwin". Names the platform
 *                            the DAEMON runs on, which decides the transport. Omitted, it
 *                            is taken from host.platform ("windows"/"unix") when that
 *                            declares one, and otherwise from this process's platform.
 *   host.daemon.socketPath   Optional unix socket override, honoured ONLY when it resolves
 *                            inside runtimeDir. It can never point the harness at a daemon
 *                            outside the profile the config declares.
 *   host.daemon.token        Optional explicit token. Omitted, it is read from the
 *                            runtimeDir the config declares, exactly as the product does.
 *
 * The platform's own production runtime directory is refused OUTRIGHT - there is no opt-in
 * field, because driving a production daemon is not authorized and no config may authorize
 * it. Only a host that declares its own isolated runtimeDir is reachable.
 *
 * This resolver is for the LOCAL transport only. A non-local host (ssh, paired, account-relay)
 * is reached over the frozen HTTP routes at its own gateway with its own credential - the
 * repository's existing mechanism, validated by `hostAccessContract` and driven by
 * `referenceRequest` - so it returns no daemon transport here. A non-local host that declares a
 * daemon runtimeDir is refused (`daemon-transport-local-only`), because a remote host's runtime
 * directory is not reachable from this process; that is a config error, not a missing
 * transport.
 * ========================================================================== */

/** The runtime directory the product uses by default on this platform, without env overrides. */
function productionRuntimeDir(platform) {
  if (platform === "win32") {
    const base = process.env.LOCALAPPDATA || process.env.TEMP || "C:\\ProgramData";
    return join(base, "Ferryx", "runtime");
  }
  const uid = typeof process.getuid === "function" ? process.getuid() : null;
  return uid === null ? null : "/tmp/rorca-" + uid;
}

/**
 * The control transport a configured host declares, or null when it declares no daemon.
 *
 * Throws an IsolatedGatewayError for a declared daemon this producer refuses to drive (a
 * missing runtimeDir, a socket override outside it, or the platform's production runtime
 * directory without an explicit opt-in).
 */
function daemonTransportForHost(host) {
  const daemon = host.daemon || null;
  if (!daemon || (!daemon.runtimeDir && !daemon.socketPath)) return null;
  // A non-local host is reached over the frozen HTTP routes at its OWN gateway (see
  // hostAccessContract in herdr-reference-fixtures.mjs), never through this machine's
  // loopback - so it has no daemon transport HERE, and that is not a refusal of the
  // transport. Declaring a daemon for one IS a config error worth naming, because a remote
  // host's own runtime directory is not reachable from this process.
  if (host.transport && host.transport !== "local") {
    if (daemon) {
      throw blocked(
        "daemon-transport-local-only",
        host.id + " is a " + host.transport + " host and declares a daemon runtimeDir/socketPath; " +
          "a remote daemon is not reachable from this machine. Reach this host through its " +
          "gateway url + credentialFile instead (hostAccessContract).",
      );
    }
    return null;
  }
  if (!daemon.runtimeDir) {
    throw blocked(
      "daemon-runtime-undeclared",
      host.id + " declares a daemon socketPath but no runtimeDir; this producer never infers one",
    );
  }
  const platform =
    daemon.platform ||
    (host.platform === "windows" ? "win32" : host.platform === "unix" ? "linux" : process.platform);
  const runtimeDir = resolve(daemon.runtimeDir);
  // No opt-in: driving a production daemon is not authorized, so the production runtime
  // directory is refused whenever the config names it, and no config field can lift that.
  const production = productionRuntimeDir(platform);
  if (production && resolve(production) === runtimeDir) {
    throw blocked(
      "daemon-runtime-is-production",
      host.id + " points at the platform production runtime directory " + runtimeDir +
        "; driving a production daemon is not authorized and no config field can enable it",
    );
  }
  // The shared descriptor derives every path from the runtimeDir, and refuses a runtimeDir
  // outside the profile root it is given - so the declared directory IS the ownership
  // boundary here.
  const transport = daemonControlTransport(
    { root: runtimeDir, paths: { runtime: runtimeDir } },
    { platform },
  );
  if (daemon.socketPath) {
    if (transport.kind !== "unix-socket") {
      throw blocked(
        "daemon-socket-override-not-unix",
        host.id + " declares a unix socketPath for a " + transport.kind + " transport",
      );
    }
    const declared = resolve(daemon.socketPath);
    if (declared !== resolve(transport.socketPath)) {
      throw blocked(
        "daemon-socket-outside-runtime",
        host.id + " socketPath " + declared + " is not the socket of " + runtimeDir,
      );
    }
  }
  return daemon.token ? { ...transport, explicitToken: String(daemon.token) } : transport;
}

/** The JSON-safe identity of a transport, for the receipts. */
function describeDaemonTransport(transport, endpoint) {
  if (!transport) return null;
  return {
    kind: transport.kind,
    platform: transport.platform,
    runtimeDir: transport.runtimeDir,
    endpoint: endpoint || null,
    requiresToken: transport.requiresToken,
    tokenSource: transport.explicitToken ? "config" : transport.requiresToken ? "runtime-file" : "socket-ownership",
  };
}

/** One control connection over a configured host's declared transport. */
function connectHostDaemon(transport, timeoutMs) {
  return connectDaemonControl(transport, {
    timeoutMs,
    ...(transport.explicitToken ? { token: transport.explicitToken } : {}),
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
async function startIsolatedGateway(config, host, ledger, args, lease) {
  try {
    return await launchIsolatedGateway({
      binary: config.candidate && config.candidate.binary,
      label: host.id,
      uiDist: config.candidate && config.candidate.uiDist,
      env: host.env || {},
      cwd: config.source && config.source.root ? config.source.root : repoRoot,
      ledger,
      timeoutMs: args.timeoutMs,
      lease,
      // The launched daemon's own output is QA evidence: a transport failure during the run
      // is only attributable if its last lines survive the run. Bounded, redacted, written
      // under --out so it travels with the other receipts.
      evidenceDir: args.out,
      evidenceName: "daemon-output-" + String(host.id).replace(/[^A-Za-z0-9._-]/g, "_") + ".log",
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
async function spawnOriginalPty(host, session, ledger, args, transport) {
  if (!transport) throw blocked("daemon-runtime-undeclared", host.id);
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
  // override a pane's requested shell. This producer talks to the daemon's own control
  // endpoint directly and sets \`shell\` on the request itself, with no WebView in the path,
  // so no cached default can intervene here. A host that drives the UI instead must verify
  // the shell it got back.
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
  // The shared client speaks whichever transport this host declares and reports its own
  // named reason when the endpoint is absent - no bare ENOENT, and no socket path this
  // producer guessed. The spawn request and the `spawnOk` response shape are unchanged.
  const client = await connectHostDaemon(transport, args.timeoutMs);
  try {
    // The workspace this session names must EXIST on the daemon before the spawn that names it:
    // the daemon answers `Workspace '<id>' is not registered` otherwise, and its startup
    // registration derives an id from the launch cwd, which is not this config's id. So the
    // registration is explicit, over this same connection, and it happens FIRST.
    const registration = workspaceRegistrationFor(host, session);
    if (!registration.ok) {
      throw blocked(registration.reason, registration.detail);
    }
    const registered = await registerWorkspaceOnDaemon(client, registration, {
      requestId: session.workspaceId,
    });
    if (!registered.ok) {
      throw blocked(registered.reason, registered.detail);
    }
    const requestId = session.clientRequestId || ("herdr-ref-" + session.backendSessionId);
    const response = await client.call({
      type: "spawn",
      clientRequestId: requestId,
      // The id that was just REGISTERED, not the raw config value: the registrar trims it, so
      // naming the untrimmed string here would ask the daemon for a workspace this run never
      // created. Registering and spawning must name the same id.
      workspaceId: registered.workspaceId,
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
      // A transport that ended mid-request settles this call with a structured failure: the
      // code names what happened (reset, closed, refused), and the request kind/id name which
      // request it was. That is a BLOCKED dependency, not an interaction bug in this script.
      const failure = daemonRequestFailure(response);
      if (failure) {
        throw blocked(failure.code, describeDaemonRequestFailure(failure));
      }
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
    // The identity file is this run's proof that the wrapper EXECUTED. A created process is not a
    // process that ran: `spawnOk` says the daemon started a child, and says nothing about whether
    // that child is alive, idle, or already dead. So when the identity never appears, the failure
    // is captured together with what the daemon ALREADY knows - its own session details and the
    // attach replay - and nothing else changes.
    let record;
    try {
      record = await readPtyIdentity(identityPath, args.timeoutMs);
    } catch (identityError) {
      // Read-only: no Write, no Resize, no answer to any query, and no waiting. The capture cannot
      // alter the run's timing and cannot convert this failure into a success - the same error is
      // rethrown with the same code, carrying evidence it did not have before.
      let report = null;
      try {
        report = await captureSessionDiagnostics(client, response.sessionId, {
          timeoutMs: DIAGNOSTIC_CAPTURE_TIMEOUT_MS,
        });
      } catch (captureError) {
        report = {
          sessionId: response.sessionId,
          captureFailure: String(captureError && captureError.message ? captureError.message : captureError),
        };
      }
      if (report && report.diagnosticUnavailable) {
        // The bound expired. Cancel the reads that are still pending by closing the connection
        // this run owns: closing settles them, so nothing is left in flight, and close() is
        // idempotent so the teardown below is unaffected. The capture is then reported as
        // UNAVAILABLE - never as an empty session, which would read as a silent PTY.
        try {
          client.close();
        } catch {
          /* The socket is already gone. */
        }
      }
      let diagnosticsPath = null;
      try {
        diagnosticsPath = join(resolve(args.out), "pty-identity-diagnostics", String(requestId).replace(/[^A-Za-z0-9._-]/g, "_") + ".json");
        writeJson(diagnosticsPath, {
          ...report,
          spawn: {
            sessionId: response.sessionId,
            epoch: String(response.epoch),
            daemonPid,
            requestedCols: session.cols,
            requestedRows: session.rows,
            reportedCols: details.cols,
            reportedRows: details.rows,
            reportedRunning: details.running,
          },
        });
      } catch {
        // A diagnostic that cannot be written must not replace the failure it explains.
        diagnosticsPath = null;
      }
      identityError.detail =
        identityError.message +
        " | sessionDiagnostics: " + summarizeSessionDiagnostics(report) +
        (diagnosticsPath ? " | diagnosticsFile=" + diagnosticsPath : " | diagnosticsFile=unwritable");
      throw identityError;
    }
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
      daemonTransport: describeDaemonTransport(transport, client.endpoint),
      // Recorded so the receipt proves the workspace existed before the spawn that named it,
      // and against which root.
      workspaceRegistration: {
        workspaceId: registration.workspaceId,
        repoRoot: registration.repoRoot,
        declaredRoot: registration.declaredRoot,
        request: "registerWorkspace",
      },
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
/* ==========================================================================
 * SSH owner-host receipt (QA only)
 * ==========================================================================
 *
 * A remote ssh session this run did not create is bound to the OWNING daemon's own report of it.
 * The daemon is the one party holding BOTH identities - its own session id and the helper's
 * target - because it created that helper session, so its reply IS the correlation, rather than a
 * cwd comparison dressed up as one.
 *
 * That reply already exists: `DaemonRequest::RemoteSessionDetails { sessionId }` answers with
 * `RemoteSessionDetails`, whose `descriptor` carries `backendSessionId`, the helper `target` and
 * the `clientRequestId` the helper was spawned with, plus the connected `pid`. This producer adds
 * no product surface and no new transport: it reads that reply through the same control client it
 * already uses for every other daemon question.
 *
 * This run CANNOT create a remote session - the local registry refuses an `ssh:` workspace id
 * (see workspaceIdRefusal) - so the create side is DECLARED by the config and the daemon's reply
 * is what verifies it. A declared value the daemon does not confirm is a typed refusal.
 */

/** The only platform this producer can read a process identity from: the probe reads /proc. */
export const SSH_RECEIPT_PROBE_PLATFORM = "linux";

/*
 * The ssh inventory path is a FILE, not a directory.
 *
 * `daemon_ssh_store_path()` returns `FERRYX_DATA_DIR/ssh_hosts.json` - a file - and with
 * FERRYX_DATA_DIR set that early return makes the dev variant unreachable. The daemon compares the
 * value it is handed for EXACT equality with its own:
 *
 *     if host_store_path != &self.ssh_store_path { "SSH inventory path is not daemon-configured" }
 *
 * and it derives the project store as that FILE's SIBLING (`projects::store_path` =
 * `host_store.with_file_name("remote_projects.json")`). So the declared value is the file
 * `<FERRYX_DATA_DIR>/ssh_hosts.json`, and the projects file sits beside it - never inside it.
 */
export const SSH_HOST_STORE_FILENAME = "ssh_hosts.json";
export const SSH_PROJECTS_STORE_FILENAME = "remote_projects.json";

/**
 * The project store the product derives from the inventory FILE, by the same rule it uses:
 * `host_store.with_file_name("remote_projects.json")` is the file's SIBLING, not a child of it.
 */
export function sshProjectsStorePath(hostStorePath) {
  return join(dirname(hostStorePath), SSH_PROJECTS_STORE_FILENAME);
}

function isNonEmptyText(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function quotePosix(value) {
  return "'" + String(value).replace(/'/g, "'\\''") + "'";
}

/**
 * The correlation, or a typed refusal naming exactly which agreement failed.
 *
 * Every check refuses rather than defaulting: an absent reply, a session id or request id the
 * daemon does not confirm, a missing helper target, and an absent pid each stop the receipt. The
 * daemon's session id and the helper's are returned SEPARATELY and never relabelled as one another.
 */
export function remoteReceiptCorrelation(details, create) {
  if (!details || typeof details !== "object") {
    return {
      ok: false,
      reason: "ssh-remote-session-unknown",
      detail: "the daemon reports no remote session details for this session id",
    };
  }
  const descriptor = details.descriptor;
  if (!descriptor || typeof descriptor !== "object") {
    return { ok: false, reason: "ssh-remote-descriptor-absent", detail: "the reply carries no descriptor" };
  }
  if (!isNonEmptyText(descriptor.backendSessionId)) {
    return { ok: false, reason: "ssh-daemon-session-id-absent", detail: "the descriptor names no backendSessionId" };
  }
  if (descriptor.backendSessionId !== create.sessionId) {
    return {
      ok: false,
      reason: "ssh-correlation-session-mismatch",
      detail: "the daemon reports session " + descriptor.backendSessionId +
        ", the create record names " + create.sessionId,
    };
  }
  if (descriptor.clientRequestId !== create.clientRequestId) {
    return {
      ok: false,
      reason: "ssh-correlation-request-mismatch",
      detail: "the daemon reports request " + String(descriptor.clientRequestId) +
        ", the create record names " + create.clientRequestId,
    };
  }
  const target = descriptor.target;
  if (!target || !isNonEmptyText(target.backendSessionId)) {
    return { ok: false, reason: "ssh-helper-target-absent", detail: "the descriptor carries no helper target" };
  }
  if (!Number.isInteger(details.pid) || details.pid <= 0) {
    return {
      ok: false,
      reason: "ssh-connected-pid-absent",
      detail: "the daemon reports pid " + String(details.pid) +
        "; a session that has not connected has no helper pid to bind",
    };
  }
  return {
    ok: true,
    daemonSessionId: descriptor.backendSessionId,
    helperTarget: target,
    helperSessionId: target.backendSessionId,
    clientRequestId: descriptor.clientRequestId,
    pid: details.pid,
  };
}

/**
 * The owner-host receipt, assembled from a correlation the daemon confirmed and host observations.
 *
 * `backendSessionId` is the DAEMON's session and `helperSessionId` is the helper's: two different
 * values, both kept. `executable` is the only value not from the daemon's reply, because the reply
 * does not carry one - it is read from the host, which is why the host must be Linux.
 *
 * Nothing here is a spawn-time record. `spawnedAt` is the moment this run observed the session and
 * says so, and the receipt states that it authorizes no cleanup: a discovered pid is evidence, not
 * a process this run owns.
 */
export function remoteOwnerHostReceipt(input) {
  const options = input || {};
  const correlation = options.correlation || {};
  if (!correlation.ok || !isNonEmptyText(correlation.daemonSessionId)) {
    throw new ProvisionError(
      EXIT.ASSERTION,
      "ssh-receipt-without-correlation",
      "a receipt is only built from a correlation the daemon confirmed",
    );
  }
  if (options.remotePlatform !== SSH_RECEIPT_PROBE_PLATFORM) {
    throw new ProvisionError(
      EXIT.ASSERTION,
      "ssh-probe-platform-unsupported",
      "the host platform is " + String(options.remotePlatform) +
        "; the process identity probe reads /proc and is Linux-only, so no receipt is produced",
    );
  }
  if (!isNonEmptyText(options.executable)) {
    throw new ProvisionError(
      EXIT.ASSERTION,
      "ssh-executable-unknown",
      "the host reported no executable for pid " + String(correlation.pid),
    );
  }
  const observedAt = isNonEmptyText(options.observedAt) ? options.observedAt : new Date().toISOString();
  const candidate = options.candidate || {};
  return {
    schema: "ferryx-herdr-reference.ssh-owner-host-receipt/1",
    // The fields the shared validator requires, each from an observed source.
    sourceKind: HOST_SUPPLIED_SOURCE_KIND,
    hostId: options.hostId,
    transport: "ssh",
    backendSessionId: correlation.daemonSessionId,
    epoch: String(options.epoch),
    pid: correlation.pid,
    executable: options.executable,
    spawnedAt: observedAt,
    candidate: {
      candidateId: candidate.candidateId || null,
      sourceManifestSha256: candidate.sourceManifestSha256 || null,
      binarySha256: candidate.binarySha256 || null,
    },
    // Provenance. Additive: the shared validator reads the block above and ignores what it does
    // not know, so nothing here can weaken the contract it enforces.
    correlation: {
      source: "daemon-remote-session-details",
      daemonSessionId: correlation.daemonSessionId,
      helperSessionId: correlation.helperSessionId,
      helperTarget: correlation.helperTarget,
      clientRequestId: correlation.clientRequestId,
    },
    acquisition: {
      method: "daemon-remote-session-details",
      observedAt,
      // Stated, because the two are easy to conflate: this is when the session was OBSERVED, not
      // when it was spawned, and no spawn-time pid record exists anywhere for this session.
      spawnedAtProvenance: "acquisition-observed",
      executableProvenance: "host-observed-proc-exe",
      platform: options.remotePlatform,
      cleanupAuthority: "none: a discovered pid is evidence, never an owned process",
    },
  };
}

/** The ssh argument vector for one host, from the config's own declared connection facts. */
export function sshConnectionArgs(host) {
  const args = [
    "-T",
    "-o", "BatchMode=yes",
    "-o", "StrictHostKeyChecking=yes",
    "-o", "UpdateHostKeys=no",
    "-o", "ConnectTimeout=" + Math.max(1, Math.ceil((host.connectTimeoutMs || 5000) / 1000)),
  ];
  if (isNonEmptyText(host.identityFile)) args.push("-i", host.identityFile);
  if (Number.isInteger(host.port) && host.port > 0) args.push("-p", String(host.port));
  args.push(isNonEmptyText(host.username) ? host.username + "@" + host.hostname : String(host.hostname));
  return args;
}

/**
 * The command that reads a remote process's executable from the host itself.
 *
 * Only the executable: `/proc/<pid>/exe` is what the daemon's reply does not carry. No start time
 * is read, because there is nothing here to compare one against - a start time observed now is not
 * a spawn record, and a value with no meaning is worse than an absent one.
 */
export function remoteExecutableProbeCommand(pid) {
  return "readlink /proc/" + String(pid) + "/exe 2>/dev/null || true";
}

/** Run one command on the host over ssh, bounded. The child is recorded by exact pid. */
function sshExec(host, remoteCommand, timeoutMs, ledger) {
  return new Promise((resolveExec, rejectExec) => {
    const child = spawn("ssh", [...sshConnectionArgs(host), remoteCommand], { stdio: ["ignore", "pipe", "pipe"] });
    if (ledger) ledger.record(child, { role: "ssh-exec", command: remoteCommand });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk.toString(); });
    child.stderr.on("data", (chunk) => { stderr += chunk.toString(); });
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      rejectExec(blocked("ssh-exec-timeout", remoteCommand + ": no exit within " + timeoutMs + "ms"));
    }, timeoutMs);
    child.once("error", (error) => { clearTimeout(timer); rejectExec(error); });
    child.once("exit", (code) => { clearTimeout(timer); resolveExec({ code, stdout, stderr }); });
  });
}

/**
 * The product's own remote spawn request for one ssh session.
 *
 * This is the request the product sends, not an approximation of it: the workspace id is the
 * DERIVED `ssh:` id (`ssh::identity(host_id, repo_root)`), the startup is `remoteSsh` with the
 * inventory FILE the daemon is configured with, and `shell` is null because a local shell override is
 * refused for an ssh session and the recording wrapper is a LOCAL artifact that does not exist on
 * the remote host. Ownership for a remote session therefore comes from the daemon's own report of
 * it, never from a wrapper this run installs.
 */
export function remoteSpawnRequest(session, host, requestId, storePath) {
  const workspaceId = isNonEmptyText(session.workspaceId) ? session.workspaceId.trim() : null;
  if (!workspaceId || !workspaceId.startsWith("ssh:")) {
    throw new ProvisionError(
      EXIT.ASSERTION,
      "ssh-workspace-id-required",
      "a remote spawn names the derived \"ssh:\" workspace id; the session declares " +
        String(session.workspaceId),
    );
  }
  if (!isNonEmptyText(storePath)) {
    throw new ProvisionError(
      EXIT.ASSERTION,
      "ssh-host-store-undeclared",
      host.id + ": declare sshHostStorePath as the FILE <FERRYX_DATA_DIR>/" + SSH_HOST_STORE_FILENAME,
    );
  }
  // The daemon's own path always ends in this filename, so a directory (or any other name) is not
  // the value it compares against. Refused here, before the spawn, rather than left for the daemon
  // to refuse as an inventory mismatch.
  if (basename(storePath) !== SSH_HOST_STORE_FILENAME) {
    throw new ProvisionError(
      EXIT.ASSERTION,
      "ssh-host-store-not-inventory-file",
      String(storePath) + ": the value is the FILE ending in " + SSH_HOST_STORE_FILENAME +
        "; a directory is not this value",
    );
  }
  return {
    type: "spawn",
    clientRequestId: requestId,
    workspaceId,
    worktree: session.worktree || null,
    cwd: session.remoteCwd || null,
    cols: session.cols,
    rows: session.rows,
    shell: null,
    startup: { remoteSsh: { hostStorePath: storePath } },
  };
}

/**
 * Spawn the original PTY for a REMOTE ssh session through the product's own remote startup, and
 * return the daemon's REAL response as the create record the correlation is then verified against.
 *
 * Two differences from the local path, both because the session is remote:
 *
 *   - NO local workspace registration. The id is an `ssh:` remote namespace that the local
 *     registry refuses by rule (`workspaceIdRefusal`), and the product resolves it from the remote
 *     project store instead. Registering would be a refusal, not a preparation.
 *   - NO recording wrapper. The wrapper is a local file this run would install on the daemon's own
 *     host; the remote host never sees it, and the daemon's `remoteSessionDetails` reply is what
 *     binds the session to the helper's PTY instead.
 */
async function spawnRemotePty(host, session, args, transport) {
  if (!transport) throw blocked("daemon-runtime-undeclared", host.id);
  const storePath = host.sshHostStorePath || null;
  if (!isNonEmptyText(storePath)) {
    throw blocked(
      "ssh-host-store-undeclared",
      host.id + ": declare sshHostStorePath as the FILE <FERRYX_DATA_DIR>/" + SSH_HOST_STORE_FILENAME,
    );
  }
  // The inventory FILE and the project store beside it are both TASK-OWNED and must already exist:
  // this producer resolves nothing on the remote host and fabricates no project. A store the
  // fixture did not place is reported, not created.
  if (!existsSync(storePath)) throw blocked("ssh-host-store-missing", storePath);
  if (!statSync(storePath).isFile()) throw blocked("ssh-host-store-not-a-file", storePath);
  const projectsPath = sshProjectsStorePath(storePath);
  if (!existsSync(projectsPath)) throw blocked("ssh-projects-store-missing", projectsPath);
  const requestId = session.clientRequestId || ("herdr-ref-" + String(session.workspaceId));
  const request = remoteSpawnRequest(session, host, requestId, storePath);
  const client = await connectHostDaemon(transport, args.timeoutMs);
  try {
    const response = await client.call(request, { requestKind: "spawn", requestId });
    if (!response || response.type !== "spawnOk") {
      const failure = daemonRequestFailure(response, { requestKind: "spawn", requestId });
      if (failure) throw blocked(failure.code, describeDaemonRequestFailure(failure));
      throw new ProvisionError(EXIT.INTERACTION, "remote-spawn-refused", JSON.stringify(response));
    }
    const details = response.session || {};
    if (details.cols !== session.cols || details.rows !== session.rows) {
      throw new ProvisionError(
        EXIT.ASSERTION,
        "spawn-geometry-mismatch",
        "requested " + session.cols + "x" + session.rows + ", daemon reports " + details.cols + "x" + details.rows,
      );
    }
    return {
      backendSessionId: response.sessionId,
      epoch: String(response.epoch),
      clientRequestId: requestId,
      daemonPid: client.handshake.pid,
      cols: details.cols,
      rows: details.rows,
      running: details.running,
      daemonTransport: describeDaemonTransport(transport, client.endpoint),
      // The daemon's OWN response, recorded as the create record. The correlation is verified
      // against this, never against a value the config asserted.
      createResponse: {
        sessionId: response.sessionId,
        clientRequestId: requestId,
        spawnedAt: new Date().toISOString(),
      },
    };
  } finally {
    client.close();
  }
}

/**
 * Acquire the owner-host receipt for one ssh session from the daemon that owns it.
 *
 * The daemon's reply is read through the SAME control client this producer already uses, and the
 * create record is verified against it. That record is the daemon's own spawn response when this
 * run created the session, and the config's declaration only when the session already existed.
 * Nothing acquired here enters the owned-process ledger: the ledger kills only pids this run
 * recorded at spawn time.
 */
async function acquireSshOwnerHostReceipt({ host, create, args, ledger, candidate, transport }) {
  if (!create || !isNonEmptyText(create.sessionId) || !isNonEmptyText(create.clientRequestId)) {
    throw blocked(
      "ssh-create-record-missing",
      host.id + ": no create record for this session - spawn it here, or declare createResponse",
    );
  }
  if (host.platform !== SSH_RECEIPT_PROBE_PLATFORM) {
    throw blocked(
      "ssh-probe-platform-unsupported",
      host.id + ": platform " + String(host.platform) + "; the process identity probe is Linux-only",
    );
  }
  if (!transport) throw blocked("daemon-runtime-undeclared", host.id);
  const client = await connectHostDaemon(transport, args.timeoutMs);
  let details = null;
  try {
    const reply = await client.call(
      { type: "remoteSessionDetails", sessionId: create.sessionId },
      { requestKind: "remoteSessionDetails", requestId: create.sessionId },
    );
    const failure = daemonRequestFailure(reply, { requestKind: "remoteSessionDetails", requestId: create.sessionId });
    if (failure) throw blocked(failure.code, describeDaemonRequestFailure(failure));
    if (!reply || reply.type !== "remoteSessionDetailsOk") {
      throw blocked("ssh-remote-session-details-refused", JSON.stringify(reply));
    }
    details = reply.details || null;
  } finally {
    client.close();
  }
  const correlation = remoteReceiptCorrelation(details, create);
  if (!correlation.ok) throw blocked(correlation.reason, correlation.detail);
  // The one value the daemon's reply does not carry, read from the host it names.
  const probe = await sshExec(host, remoteExecutableProbeCommand(correlation.pid), args.timeoutMs, ledger);
  if (probe.code !== 0) {
    throw blocked("ssh-executable-probe-failed", "pid " + correlation.pid + ": exit " + probe.code + ": " + probe.stderr.slice(-500));
  }
  const receipt = remoteOwnerHostReceipt({
    correlation,
    hostId: host.id,
    epoch: client.handshake.epoch,
    remotePlatform: host.platform,
    executable: probe.stdout.trim(),
    observedAt: new Date().toISOString(),
    candidate,
  });
  const dir = join(resolve(args.out), "ssh-owner-host-receipts");
  mkdirSync(dir, { recursive: true });
  const receiptPath = join(dir, create.sessionId + ".json");
  writeJson(receiptPath, receipt);
  return { receipt, receiptPath };
}


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
  // Gateways handed to the scenario runner instead of being stopped here. They are NOT
  // owned by this process any more, so they are excluded from the ledger teardown and named
  // in its receipts by exact PID and lease.
  const retainedGateways = new Set();
  let interrupted = false;
  // Whether this run reached its success path, so the retained-daemon announcement is made
  // ONCE: on stdout as part of the receipt, or on stderr when the run failed after retention.
  let completedSuccessfully = false;

  const onSignal = () => {
    interrupted = true;
  };
  process.on("SIGINT", onSignal);
  process.on("SIGTERM", onSignal);

  try {
    const configuredHosts = Array.isArray(config.hosts) ? config.hosts : [];
    // One transport resolution per host, reused by every later step: resolving twice would
    // also report the same refusal twice.
    const hostTransports = new Map();
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
        // How this host's original session is reached: a daemon this machine owns (local), or
        // the host's own gateway over the frozen HTTP routes (ssh/paired/account-relay).
        access: null,
        // The transport this host declares, resolved once here so every later step (and the
        // receipt) speaks about the same endpoint. A declared daemon this producer refuses to
        // drive is reported as a blocker rather than silently skipped.
        daemonTransport: null,
      };
      // The ACCESS contract first: it is what decides how this host's original session is
      // reached, and it is where an incomplete fixture is named precisely (which endpoint or
      // credential is missing) instead of the transport being refused wholesale.
      try {
        record.access = hostAccessContract(host);
      } catch (error) {
        if (!(error instanceof IsolatedGatewayError)) throw error;
        record.access = null;
        blockers.push({
          kind: "host-access-contract",
          hostId: host.id,
          transport: host.transport,
          reason: error.reason,
          detail: error.detail,
        });
      }
      try {
        const transport = daemonTransportForHost(host);
        hostTransports.set(host.id, { transport });
        record.daemonTransport = describeDaemonTransport(transport, null);
      } catch (error) {
        if (!(error instanceof ProvisionError)) throw error;
        hostTransports.set(host.id, { error });
        blockers.push({
          kind: "host-daemon-transport",
          hostId: host.id,
          transport: host.transport,
          reason: error.reason,
          detail: error.detail,
        });
      }
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
          // A handoff must be bounded, so a retained gateway always carries a deadline the
          // runner checks before it adopts.
          const lease = args.retainGateway
            ? {
                ownerPid: process.pid,
                createdAt: Date.now(),
                deadlineAt: Date.now() + args.retainDeadlineMs,
                scope: "adoption-only",
              }
            : null;
          const gateway = await startIsolatedGateway(config, host, ledger, args, lease);
          record.started = true;
          // The endpoint this run actually drove for this host, from the launcher's own
          // handle - a configured host.daemon and a launched gateway are the same field.
          record.daemonTransport = gateway.daemonTransport;
          // This run's own daemon replaces whatever the config declared for this host: the
          // PTY spawn below must go to the daemon this run started, whose profile is the one
          // the launcher just created.
          hostTransports.set(host.id, { transport: gateway.controlTransport });
          // Recorded as EVIDENCE of this launch. Whether it also becomes the fixture's host
          // url depends on the handoff: a gateway this run stops at exit would be a dead URL
          // for the runner, so only a RETAINED one is published with its real url and
          // credential (see below).
          record.gateway = {
            contract: gateway.contract,
            urlRedacted: redactUrl(gateway.url),
            boundAddress: gateway.boundAddress,
            pinnedPort: gateway.pinnedPort,
            portRequirement: gateway.portRequirement,
            pid: gateway.entry.pid,
            executablePath: gateway.entry.executablePath,
            daemonTransport: gateway.daemonTransport,
            daemonSocketPath: gateway.daemonSocketPath,
            daemonPid: gateway.daemonPid,
            daemonEpoch: gateway.daemonEpoch,
            devicePermission: gateway.devicePermission,
            referenceHostId: gateway.referenceHostId,
            // Published beside the host id by the same incarnation, so a later read or mutation
            // can echo the OWNING gateway's owner instead of a value the config declared.
            referenceOwnerId: gateway.referenceOwnerId,
          };
          if (args.retainGateway) {
            // Handed off, not stopped: the scenario runner drives THIS daemon, so the
            // sessions the PTYs were spawned in, their epoch and their recorded identity all
            // stay valid.
            //
            // The lease is an ADOPTION deadline only - how long the handoff stays adoptable.
            // It is not a lifetime and it does not clean anything up: nothing in this harness
            // reaps a retained daemon that no runner adopts, because a reaper would be a
            // second service. The verifier reaps it, by the exact PID below, in its own
            // finally - including when provisioning succeeded and the runner never started.
            retainedGateways.add(gateway);
            record.ownedGateway = gateway.ownership;
            record.url = gateway.url;
            record.urlRedacted = redactUrl(gateway.url);
            record.credentialFile = gateway.credentialFile;
          } else {
            startedGateways.push(gateway);
          }
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
      // A host that declares no daemon is simply not a daemon-bearing host; one whose declared
      // daemon was refused above is already reported. A host that declares a daemon which
      // cannot be reached is reported here, never skipped: the epoch and PID that follow are
      // the daemon's own report and cannot be inferred.
      const resolved = hostTransports.get(host.id);
      if (!resolved || resolved.error || !resolved.transport) continue;
      const transport = resolved.transport;
      try {
        const client = await connectHostDaemon(transport, args.timeoutMs);
        try {
          record.daemonEpoch = String(client.handshake.epoch);
          record.daemonPid = client.handshake.pid;
          record.daemonTransport = describeDaemonTransport(transport, client.endpoint);
        } finally {
          client.close();
        }
      } catch (error) {
        if (!(error instanceof ProvisionError)) throw error;
        blockers.push({
          kind: "host-daemon-unreachable",
          hostId: host.id,
          transport: host.transport,
          reason: error.reason,
          detail: error.detail,
        });
      }
    }

    // Sessions: the authoritative target tuple is assembled from the host plus the daemon's
    // own report. A field the harness cannot obtain is recorded as missing.
    //
    // The owner every row carries is the LOCAL gateway's published one, not the session host's:
    // the runner binds every reference read and mutation through the local host's url, so the
    // authority that will answer TARGET_EXPIRED for a foreign owner is the local gateway. A run
    // whose gateway published no owner cannot produce a usable target, so that is recorded as a
    // BLOCKER here - never as a row with no owner, which would read as a pass with nothing bound.
    const localHostRecord = hosts.find((entry) => entry.transport === "local") || null;
    const publishedOwnerId =
      localHostRecord &&
      typeof localHostRecord.referenceOwnerId === "string" &&
      localHostRecord.referenceOwnerId.trim().length > 0
        ? localHostRecord.referenceOwnerId
        : null;
    if (publishedOwnerId === null) {
      blockers.push({
        kind: "reference-owner-unavailable",
        hostId: localHostRecord ? localHostRecord.id : null,
        reason:
          "the gateway published no referenceOwnerId in /api/v1/capabilities; a configured " +
          "ownerId is not an authority and none may be substituted for it",
      });
    }
    for (const host of configuredHosts) {
      const hostRecord = hosts.find((entry) => entry.id === host.id);
      for (const session of host.sessions || []) {
        const row = {
          hostId: host.id,
          // The owner a read or a mutation must carry is the one the OWNING gateway published
          // beside its reference host id. The config's ownerId is an INPUT, never an authority:
          // it is recorded for audit and is never what a target is built from.
          ownerId: publishedOwnerId,
          configuredOwnerId:
            typeof session.ownerId === "string" && session.ownerId.trim().length > 0
              ? session.ownerId
              : null,
          referenceOwnerId: publishedOwnerId,
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
          // The daemon this spawn goes to: the one this run launched for the host when it did,
          // otherwise the transport the config declares for it. Never a guess.
          const hostTransport = hostTransports.get(host.id);
          const transport = hostTransport && !hostTransport.error ? hostTransport.transport : null;
          // An ssh session goes through the product's own REMOTE startup: a different request,
          // no local registration and no local wrapper. The local path below is unchanged.
          const spawned = host.transport === "ssh"
            ? await spawnRemotePty(host, session, args, transport)
            : await spawnOriginalPty(host, {
                ...session,
                cols: session.cols,
                rows: session.rows,
              }, ledger, args, transport);
          row.backendSessionId = spawned.backendSessionId;
          row.epoch = spawned.epoch;
          row.cols = spawned.cols;
          row.rows = spawned.rows;
          row.daemonPid = spawned.daemonPid;
          row.daemonTransport = spawned.daemonTransport;
          row.workspaceRegistration = spawned.workspaceRegistration;
          row.spawnedByThisRun = true;
          row.ptyIdentity = spawned.ptyIdentity;
          row.ptyIdentityPath = spawned.ptyIdentityPath;
          // The REAL create record, from the daemon's own spawn response. The correlation is
          // verified against this rather than against anything the config asserted.
          if (spawned.createResponse) row.createResponse = spawned.createResponse;
          if (spawned.pid) {
            row.pid = spawned.pid;
            row.executablePath = spawned.executablePath;
            row.missingIdentity = row.missingIdentity.filter((field) =>
              !["backendSessionId", "epoch", "pid", "executablePath", "cols", "rows"].includes(field));
          } else {
            row.missingIdentity = row.missingIdentity.filter((field) =>
              !["backendSessionId", "epoch", "cols", "rows"].includes(field));
          }
        }

        // A remote ssh session this run did not create is bound to the owning daemon's own report
        // of it. That reply is the correlation - the daemon holds both its own session id and the
        // helper's target - and the create record is verified against it: the daemon's own spawn
        // response when this run created the session, the config's declaration only when the
        // session already existed. A discovered pid carries NO cleanup authority: it is evidence,
        // not an owned process, so
        // nothing here reaches the owned-process ledger.
        if (!row.ptyIdentity && host.transport === "ssh" && host.acquireSshReceipt === true && !row.spawnReceiptPath) {
          try {
            const resolved = hostTransports.get(host.id);
            const acquired = await acquireSshOwnerHostReceipt({
              host,
              // The daemon's own spawn response when this run created the session; the config's
              // declaration only when the session already existed.
              create: row.createResponse || session.createResponse || null,
              args,
              ledger,
              transport: resolved && !resolved.error ? resolved.transport : null,
              candidate: {
                candidateId: candidateManifest.candidateId,
                sourceManifestSha256: candidateManifestSha256,
                binarySha256: candidateManifest.binary ? candidateManifest.binary.sha256 : null,
              },
            });
            row.acquiredSshReceiptPath = acquired.receiptPath;
            // The helper's own id, kept BESIDE the daemon's rather than relabelled as it.
            row.sshHelperSessionId = acquired.receipt.correlation.helperSessionId;
            row.pid = acquired.receipt.pid;
            row.pidSource = "owner-host-spawn-receipt";
            row.executablePath = acquired.receipt.executable;
            row.missingIdentity = row.missingIdentity.filter((field) => !["pid", "executablePath"].includes(field));
          } catch (error) {
            if (!(error instanceof ProvisionError)) throw error;
            blockers.push({
              kind: "owner-host-spawn-receipt",
              hostId: host.id,
              transport: host.transport,
              backendSessionId: session.backendSessionId || null,
              reason: error.reason,
              detail: error.detail,
            });
          }
        }
        // A session this run did not spawn (ssh, paired, account-relay) must be bound to the
        // OWNING HOST's own spawn receipt. A pid that merely appears in the config is not
        // ownership proof: it could be stale, belong to another session, or have been typed by
        // hand. So the receipt is mandatory, it must agree with this run on host, transport,
        // session, incarnation and candidate provenance, and an absent or mismatched receipt is
        // a BLOCKER rather than a stamped identity.
        if (!row.ptyIdentity) {
          const receiptPath =
            session.spawnReceiptPath || host.spawnReceiptPath || row.acquiredSshReceiptPath || null;
          const requirement = receiptPath ? null : ownerHostReceiptRequirement(host, row);
          if (requirement) {
            // One contract, shared with the runner: the missing prerequisite is named field by
            // field rather than reported as a blanket non-local refusal.
            blockers.push(requirement);
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
    if (retainedGateways.size > 0) {
      for (const gateway of retainedGateways) {
        process.stdout.write(
          "RETAINED gateway pid=" + gateway.ownership.daemonPid +
            " executable=" + gateway.ownership.daemonExecutablePath +
            " url=" + redactUrl(gateway.url) +
            " leaseDeadlineAt=" + gateway.ownership.lease.deadlineAt +
            " - NOT stopped here. A runner that adopts it reaps it; otherwise the verifier must" +
            " stop exactly this PID and remove profileRoot=" + gateway.ownership.profileRoot + "\n",
        );
      }
    }
    completedSuccessfully = true;
    return EXIT.OK;
  } finally {
    // Teardown is paired and recorded: every gateway this run started AND still owns is
    // stopped, and the ledger kills only PIDs whose live executable still matches the
    // spawn-recorded one. A gateway handed to the runner is no longer owned here: it is
    // removed from the ledger so this teardown cannot kill the daemon the runner will adopt,
    // and it is named in the receipt by exact PID, executable and lease instead.
    for (const gateway of retainedGateways) {
      const retainedPid = gateway.ownership.daemonPid;
      try {
        gateway.persistDaemonOutput(args.out, "daemon-output-" + String(retainedPid) + ".log");
      } catch {
        /* Evidence that cannot be written must not replace the failure it explains. */
      }
      ledger.entries = ledger.entries.filter((entry) => entry.pid !== retainedPid);
      ledger.receipts.push({
        pid: retainedPid,
        action: "retained-for-runner",
        executablePath: gateway.ownership.daemonExecutablePath,
        lease: gateway.ownership.lease,
        profileRoot: gateway.ownership.profileRoot,
        reapRequirement: gateway.ownership.reapRequirement,
      });
      // Announced here only when the run did NOT reach its success path - a failure after
      // retention, before fixtures.json exists, is exactly the case where nothing else names
      // this PID. On success the stdout receipt below is the single announcement.
      if (!completedSuccessfully) {
        process.stderr.write(
          "RETAINED (not stopped, no reaper service): pid=" + retainedPid +
            " executable=" + gateway.ownership.daemonExecutablePath +
            " profileRoot=" + gateway.ownership.profileRoot +
            " adoptionDeadlineAt=" + gateway.ownership.lease.deadlineAt +
            " - reap this exact PID in your finally if no runner adopts it\n",
        );
      }
    }
    for (const gateway of startedGateways) {
      if (retainedGateways.has(gateway)) continue;
      // Persist the daemon's own output BEFORE it is stopped: this is what attributes a
      // mid-run transport failure, and it is the only place the whole run's daemon log is
      // still available. Bounded and redacted by the writer.
      try {
        gateway.persistDaemonOutput(args.out, gateway.evidenceName || ("daemon-output-" + String(gateway.entry.pid) + ".log"));
      } catch {
        /* Evidence that cannot be written must not replace the failure it explains. */
      }
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

// Importing this module for its pure helpers must not run a provisioning pass, so the CLI
// entry point runs only when this file itself was invoked.
const invokedAsScript = process.argv[1]
  ? resolve(process.argv[1]) === fileURLToPath(import.meta.url)
  : false;
if (invokedAsScript) {
  main()
    .then((code) => process.exit(code))
    .catch((error) => {
      const code = error instanceof ProvisionError ? error.code : EXIT.INTERACTION;
      process.stderr.write((error.reason || "provision-failed") + ": " + (error.detail || error.message) + "\n");
      process.exit(code);
    });
}

