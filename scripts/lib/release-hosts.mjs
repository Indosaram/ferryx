import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { normalize } from "node:path";

export const ALLOWED_ROOT_KEYS = Object.freeze([
  "schemaVersion",
  "repository",
  "ghosttyRepository",
  "repo",
  "hosts",
]);

export const ALLOWED_HOST_KEYS = Object.freeze([
  "ssh",
  "platform",
  "root",
  "minFreeBytes",
  "path",
  "expectedTools",
  "notaryProfile",
  "signingIdentity",
]);

export const ALLOWED_TOOL_KEYS = Object.freeze([
  "bun",
  "node",
  "zig",
  "rust",
  "tauri",
]);

export const REQUIRED_HOSTS = Object.freeze(["macbook", "omaki", "maho-win"]);

export const EXPECTED_PLATFORMS = Object.freeze({
  macbook: "darwin",
  omaki: "linux",
  "maho-win": "win32",
});

const SECRET_KEY_REGEX = /(secret|password|token|credential|private|auth|bearer|passphrase|apikey)/i;
const SECRET_VALUE_REGEX = /(-----BEGIN [A-Z0-9_-]+ PRIVATE KEY|ghp_[a-zA-Z0-9]{36}|github_pat_[a-zA-Z0-9_]{82}|gho_[a-zA-Z0-9]{36}|glpat-[a-zA-Z0-9_-]{20}|bearer\s+|password\s*=|pwd\s*=)/i;
const CREDENTIAL_PATH_REGEX = /(\.(pem|key|p12|pfx)$|(\/|\\|^)id_(rsa|dsa|ecdsa|ed25519)$)/i;
const SSH_DEST_REGEX = /^([a-zA-Z0-9_.-]+@)?[a-zA-Z0-9_.:-]+$/;

function isPathAbsolute(p) {
  if (typeof p !== "string" || p.trim() === "") return false;
  if (p.includes("\0")) return false;
  return p.startsWith("/") || /^[a-zA-Z]:[/\\]/.test(p);
}

function scanForSecrets(val, path = "config") {
  if (val === null || val === undefined) return;
  if (typeof val === "string") {
    if (SECRET_VALUE_REGEX.test(val)) {
      throw new Error(`Secret or credential pattern detected in value at ${path}`);
    }
    if (CREDENTIAL_PATH_REGEX.test(val)) {
      throw new Error(`Credential file path detected in config at ${path}: '${val}'`);
    }
    return;
  }
  if (typeof val === "object") {
    for (const [k, v] of Object.entries(val)) {
      if (SECRET_KEY_REGEX.test(k)) {
        throw new Error(`Secret or credential field detected: '${k}' at ${path}`);
      }
      scanForSecrets(v, `${path}.${k}`);
    }
  }
}

/**
 * Validates and normalizes host configuration according to the local release contract.
 */
export function validateHostConfig(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Host config must be an object");
  }

  // Scan all keys and values for secrets/credentials before processing
  scanForSecrets(value);

  // Check unknown root properties
  for (const key of Object.keys(value)) {
    if (!ALLOWED_ROOT_KEYS.includes(key)) {
      throw new Error(`Unknown property in host config: '${key}'`);
    }
  }

  // schemaVersion
  if (value.schemaVersion !== 1 || !Number.isInteger(value.schemaVersion)) {
    throw new Error(`Invalid schemaVersion: expected 1, got ${value.schemaVersion}`);
  }

  // repo
  if (value.repo !== "Indosaram/ferryx") {
    throw new Error(`Invalid repo: expected 'Indosaram/ferryx', got '${value.repo}'`);
  }

  // repository
  if (!isPathAbsolute(value.repository)) {
    throw new Error(`repository must be an absolute path: '${value.repository}'`);
  }

  // ghosttyRepository
  if (!isPathAbsolute(value.ghosttyRepository)) {
    throw new Error(`ghosttyRepository must be an absolute path: '${value.ghosttyRepository}'`);
  }

  // hosts object
  if (!value.hosts || typeof value.hosts !== "object" || Array.isArray(value.hosts)) {
    throw new Error("Host config 'hosts' must be an object");
  }

  // Check required hosts
  for (const hostName of REQUIRED_HOSTS) {
    if (!(hostName in value.hosts)) {
      throw new Error(`Missing required host: '${hostName}'`);
    }
  }

  // Check unknown hosts
  for (const hostName of Object.keys(value.hosts)) {
    if (!REQUIRED_HOSTS.includes(hostName)) {
      throw new Error(`Unknown host in config: '${hostName}'`);
    }
  }

  const normalizedHosts = {};

  for (const hostName of REQUIRED_HOSTS) {
    const host = value.hosts[hostName];
    if (!host || typeof host !== "object" || Array.isArray(host)) {
      throw new Error(`Host '${hostName}' definition must be an object`);
    }

    // Check unknown host keys
    for (const key of Object.keys(host)) {
      if (!ALLOWED_HOST_KEYS.includes(key)) {
        throw new Error(`Unknown property in host '${hostName}': '${key}'`);
      }
    }

    // platform
    const expectedPlatform = EXPECTED_PLATFORMS[hostName];
    if (host.platform !== expectedPlatform) {
      throw new Error(
        `Host '${hostName}' platform must be '${expectedPlatform}', got '${host.platform}'`
      );
    }

    // ssh destination
    if (hostName === "macbook") {
      if (host.ssh !== null) {
        throw new Error(`macbook ssh destination must be null, got '${host.ssh}'`);
      }
    } else {
      if (typeof host.ssh !== "string" || host.ssh.trim() === "") {
        throw new Error(`Invalid SSH destination for host '${hostName}': empty or non-string`);
      }
      if (host.ssh.startsWith("-") || !SSH_DEST_REGEX.test(host.ssh)) {
        throw new Error(`Invalid SSH destination for host '${hostName}': '${host.ssh}'`);
      }
    }

    // root path
    if (typeof host.root !== "string" || host.root.trim() === "") {
      throw new Error(`Host '${hostName}' root must be a non-empty string`);
    }

    if (host.platform === "win32") {
      if (host.root.includes("\\")) {
        throw new Error(
          `Windows root must use forward slashes: '${host.root}'`
        );
      }
      if (!/^[a-zA-Z]:\/[^\\:*?"<>|\r\n]*$/.test(host.root)) {
        throw new Error(
          `Windows root must be an absolute path with forward slashes: '${host.root}'`
        );
      }
    } else {
      if (!host.root.startsWith("/")) {
        throw new Error(`Host '${hostName}' root must be an absolute path: '${host.root}'`);
      }
    }

    // Run root must not equal repository
    if (normalize(host.root) === normalize(value.repository)) {
      throw new Error(
        `Host root must not equal repository: '${host.root}'`
      );
    }

    // minFreeBytes
    if (
      typeof host.minFreeBytes !== "number" ||
      !Number.isInteger(host.minFreeBytes) ||
      host.minFreeBytes <= 0
    ) {
      throw new Error(
        `minFreeBytes must be a positive integer for host '${hostName}', got ${host.minFreeBytes}`
      );
    }

    // Optional path
    if (host.path !== undefined) {
      if (typeof host.path !== "string" || host.path.trim() === "") {
        throw new Error(`Host '${hostName}' path must be a non-empty string if specified`);
      }
    }

    // Optional expectedTools
    if (host.expectedTools !== undefined) {
      if (
        !host.expectedTools ||
        typeof host.expectedTools !== "object" ||
        Array.isArray(host.expectedTools)
      ) {
        throw new Error(`Host '${hostName}' expectedTools must be an object`);
      }
      for (const [tool, ver] of Object.entries(host.expectedTools)) {
        if (!ALLOWED_TOOL_KEYS.includes(tool)) {
          throw new Error(`Unknown tool in expectedTools: '${tool}'`);
        }
        if (typeof ver !== "string" || ver.trim() === "") {
          throw new Error(`Invalid version for tool '${tool}': must be a non-empty string`);
        }
      }
    }

    // notaryProfile and signingIdentity: only permitted on macOS (darwin)
    if (host.notaryProfile !== undefined) {
      if (host.platform !== "darwin") {
        throw new Error(
          `notaryProfile is only permitted on macOS (darwin), got on '${hostName}'`
        );
      }
      if (typeof host.notaryProfile !== "string" || host.notaryProfile.trim() === "") {
        throw new Error(`Host '${hostName}' notaryProfile must be a non-empty string`);
      }
    }

    if (host.signingIdentity !== undefined) {
      if (host.platform !== "darwin") {
        throw new Error(
          `signingIdentity is only permitted on macOS (darwin), got on '${hostName}'`
        );
      }
      if (typeof host.signingIdentity !== "string" || host.signingIdentity.trim() === "") {
        throw new Error(`Host '${hostName}' signingIdentity must be a non-empty string`);
      }
    }

    normalizedHosts[hostName] = Object.freeze({
      ssh: host.ssh,
      platform: host.platform,
      root: host.root,
      minFreeBytes: host.minFreeBytes,
      ...(host.path !== undefined ? { path: host.path } : {}),
      ...(host.expectedTools !== undefined ? { expectedTools: Object.freeze({ ...host.expectedTools }) } : {}),
      ...(host.notaryProfile !== undefined ? { notaryProfile: host.notaryProfile } : {}),
      ...(host.signingIdentity !== undefined ? { signingIdentity: host.signingIdentity } : {}),
    });
  }

  return Object.freeze({
    schemaVersion: value.schemaVersion,
    repository: value.repository,
    ghosttyRepository: value.ghosttyRepository,
    repo: value.repo,
    hosts: Object.freeze(normalizedHosts),
  });
}

/**
 * Loads and validates host configuration from a JSON file.
 */
export function loadHostConfig(configPath) {
  if (typeof configPath !== "string" || configPath.trim() === "") {
    throw new Error("configPath must be a non-empty string");
  }
  const content = readFileSync(configPath, "utf8");
  let parsed;
  try {
    parsed = JSON.parse(content);
  } catch (err) {
    throw new Error(`Invalid JSON in host config '${configPath}': ${err.message}`);
  }
  return validateHostConfig(parsed);
}

/**
 * Safely quotes a string for POSIX sh/bash.
 */
export function quoteSh(value) {
  const str = String(value ?? "");
  return "'" + str.replace(/'/g, "'\\''") + "'";
}

/**
 * Safely quotes a string for Windows PowerShell literal single-quoted strings.
 * Internal single quotes are doubled; no backslash escaping needed.
 */
export function quotePowerShell(value) {
  const str = String(value ?? "");
  return "'" + str.replace(/'/g, "''") + "'";
}

export class ProcessExecutionError extends Error {
  constructor(message, { exitCode, stdout, stderr, command, signal } = {}) {
    super(message);
    this.name = "ProcessExecutionError";
    this.exitCode = exitCode ?? null;
    this.signal = signal ?? null;
    this.stdout = stdout ?? "";
    this.stderr = stderr ?? "";
    this.command = command;
  }
}

export class ProcessTimeoutError extends Error {
  constructor(message, { timeoutMs, stdout, stderr, command } = {}) {
    super(message);
    this.name = "ProcessTimeoutError";
    this.timedOut = true;
    this.timeoutMs = timeoutMs;
    this.exitCode = null;
    this.signal = "SIGKILL";
    this.stdout = stdout ?? "";
    this.stderr = stderr ?? "";
    this.command = command;
  }
}

export function redactProcessOutput(text, effectiveEnv = process.env) {
  const values = new Set();
  for (const [name, value] of Object.entries(effectiveEnv ?? {})) {
    if (!/(PRIVATE_KEY|PASSWORD|SECRET|TOKEN|APPLE_API_KEY)/i.test(name) || value == null || value === "") continue;
    const secret = String(value);
    values.add(secret);
    values.add(JSON.stringify(secret).slice(1, -1));
  }
  let result = String(text ?? "");
  for (const value of [...values].sort((a, b) => b.length - a.length)) {
    result = result.split(value).join("[REDACTED]");
  }
  return result;
}

/**
 * Executes a subprocess with argv isolation, stdout/stderr buffering, and process group lifecycle control.
 */
export function runProcess(command, args = [], options = {}) {
  return new Promise((resolvePromise, rejectPromise) => {
    const {
      cwd,
      env,
      input,
      timeoutMs = 60000,
    } = options;
    const effectiveEnv = { ...(env ?? process.env) };
    const redact = (text) => redactProcessOutput(text, effectiveEnv);
    const displayCommand = redact(command);
    const startError = (error) => {
      const failure = new ProcessExecutionError(redact(error.message), { command: displayCommand });
      failure.code = error.code;
      return failure;
    };

    const isWin = process.platform === "win32";
    let child;
    try {
      child = spawn(command, args, {
        cwd,
        env: effectiveEnv,
        stdio: ["pipe", "pipe", "pipe"],
        detached: !isWin, // leader of process group for tree cleanup on POSIX
      });
    } catch (err) {
      return rejectPromise(startError(err));
    }

    const stdoutChunks = [];
    const stderrChunks = [];
    let settled = false;
    let timeoutTimer = null;
    let forceKillTimer = null;
    let didTimeout = false;
    let inputError = null;

    // Buffer output streams
    child.stdout.on("data", (chunk) => stdoutChunks.push(chunk));
    child.stderr.on("data", (chunk) => stderrChunks.push(chunk));

    const cleanupTimers = () => {
      if (timeoutTimer) {
        clearTimeout(timeoutTimer);
        timeoutTimer = null;
      }
      if (forceKillTimer) {
        clearTimeout(forceKillTimer);
        forceKillTimer = null;
      }
    };

    const killProcessTree = (sig = "SIGTERM") => {
      try {
        if (!isWin && child.pid) {
          process.kill(-child.pid, sig);
        } else if (child.pid) {
          child.kill(sig);
        }
      } catch {
        try {
          child.kill(sig);
        } catch {}
      }
    };

    if (typeof timeoutMs === "number" && timeoutMs > 0) {
      timeoutTimer = setTimeout(() => {
        didTimeout = true;
        killProcessTree("SIGTERM");
        forceKillTimer = setTimeout(() => {
          killProcessTree("SIGKILL");
        }, 250);
      }, timeoutMs);
    }

    // Subscribe to error and close BEFORE writing any input
    child.on("error", (err) => {
      cleanupTimers();
      if (settled) return;
      settled = true;
      rejectPromise(startError(err));
    });

    child.on("close", (code, signal) => {
      cleanupTimers();
      if (settled) return;
      settled = true;

      const stdout = redact(Buffer.concat(stdoutChunks).toString("utf8"));
      const stderr = redact(Buffer.concat(stderrChunks).toString("utf8"));

      if (inputError) {
        const failure = new ProcessExecutionError(
          `Command input failed: ${redact(inputError.message)}`,
          { exitCode: code, signal, stdout, stderr, command: displayCommand },
        );
        failure.code = inputError.code;
        return rejectPromise(failure);
      }

      if (didTimeout) {
        return rejectPromise(
          new ProcessTimeoutError(
            `Command timed out after ${timeoutMs}ms: ${displayCommand}`,
            { timeoutMs, stdout, stderr, command: displayCommand }
          )
        );
      }

      if (code !== 0) {
        const exitMsg = code !== null
          ? `Command failed with exit code ${code}: ${displayCommand}`
          : `Command terminated by signal ${signal}: ${displayCommand}`;
        return rejectPromise(
          new ProcessExecutionError(exitMsg, {
            exitCode: code,
            signal,
            stdout,
            stderr,
            command: displayCommand,
          })
        );
      }

      resolvePromise({
        stdout,
        stderr,
        exitCode: code,
      });
    });

    // Write input strictly after event listener subscription
    if (input !== undefined && input !== null) {
      child.stdin.on("error", (error) => {
        if (error.code === "EPIPE") return;
        inputError = error;
        killProcessTree("SIGKILL");
      });
      child.stdin.end(input);
    } else {
      child.stdin.end();
    }
  });
}

/**
 * Executes a script on a configured host via the appropriate transport:
 * - macbook (darwin): local bash -s
 * - omaki (linux): SSH BatchMode + bash -s
 * - maho-win (win32): SSH BatchMode + PowerShell UTF-16LE Base64 -EncodedCommand
 */
export function runHostScript(host, scripts = {}, options = {}) {
  if (!host || typeof host !== "object") {
    throw new Error("Invalid host definition: host must be an object");
  }
  if (!host.platform) {
    throw new Error("Invalid host definition: host must specify platform");
  }

  const { posix, powershell } = scripts;
  const { timeoutMs, input } = options;
  const sshCommand = options.sshCommand || process.env.FERRYX_SSH_COMMAND || "ssh";

  if (host.platform === "darwin") {
    if (typeof posix !== "string" || posix.trim() === "") {
      throw new Error("POSIX script required for darwin host");
    }
    return runProcess("bash", ["-s"], { input: posix, timeoutMs });
  }

  if (host.platform === "linux") {
    if (typeof posix !== "string" || posix.trim() === "") {
      throw new Error("POSIX script required for linux host");
    }
    if (!host.ssh || typeof host.ssh !== "string") {
      throw new Error("SSH destination required for linux host");
    }

    const args = [
      "-o", "BatchMode=yes",
      "-o", "ConnectTimeout=10",
      "-o", "ServerAliveInterval=5",
      "-o", "ServerAliveCountMax=2",
      host.ssh,
      "bash",
      "-s",
    ];

    return runProcess(sshCommand, args, { input: posix, timeoutMs });
  }

  if (host.platform === "win32") {
    if (typeof powershell !== "string" || powershell.trim() === "") {
      throw new Error("PowerShell script required for win32 host");
    }
    if (!host.ssh || typeof host.ssh !== "string") {
      throw new Error("SSH destination required for win32 host");
    }

    // PowerShell script with SilentlyContinue and explicit exit status handling
    const wrappedScript = [
      "$ProgressPreference = 'SilentlyContinue';",
      "try {",
      powershell,
      "    if ($LASTEXITCODE -ne $null -and $LASTEXITCODE -ne 0) {",
      "        exit $LASTEXITCODE",
      "    }",
      "} catch {",
      "    [Console]::Error.WriteLine($_)",
      "    exit 1",
      "}",
    ].join("\r\n");

    const encoded = Buffer.from(wrappedScript, "utf16le").toString("base64");

    const args = [
      "-o", "BatchMode=yes",
      "-o", "ConnectTimeout=10",
      "-o", "ServerAliveInterval=5",
      "-o", "ServerAliveCountMax=2",
      host.ssh,
      "powershell",
      "-NoProfile",
      "-NonInteractive",
      "-EncodedCommand",
      encoded,
    ];

    return runProcess(sshCommand, args, { input, timeoutMs });
  }

  throw new Error(`Unsupported host platform: '${host.platform}'`);
}
