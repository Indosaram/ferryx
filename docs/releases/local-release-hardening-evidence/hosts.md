# Ferryx Local Release Hardening: Host Configuration & Process Transport Evidence

Date: 2026-09-08  
Component: Host Configuration & Process Transport Boundary (`scripts/lib/release-hosts.mjs`, `scripts/release-hosts.test.mjs`, `scripts/release-hosts.example.json`)  
Contract: `docs/releases/local-pipeline-audit-2026-09-08/IMPLEMENTATION_CONTRACT.md` (Section 1, Section 7, Section 9)

---

## 1. Exported Canonical API

The module `scripts/lib/release-hosts.mjs` provides pure standard-library ESM host configuration validation and process execution boundaries compatible with Node.js >= 22.22 and Bun >= 1.4 with zero external npm dependencies.

```typescript
export function validateHostConfig(value: unknown): HostConfig;

export function loadHostConfig(configPath: string): HostConfig;

export function quoteSh(value: unknown): string;

export function quotePowerShell(value: unknown): string;

export class ProcessExecutionError extends Error {
  name: "ProcessExecutionError";
  exitCode: number | null;
  signal: string | null;
  stdout: string;
  stderr: string;
  command: string;
}

export class ProcessTimeoutError extends Error {
  name: "ProcessTimeoutError";
  timedOut: true;
  timeoutMs: number;
  exitCode: null;
  signal: "SIGKILL";
  stdout: string;
  stderr: string;
  command: string;
}

export function runProcess(
  command: string,
  args?: string[],
  options?: {
    cwd?: string;
    env?: NodeJS.ProcessEnv;
    input?: string | Buffer | Uint8Array;
    timeoutMs?: number;
  }
): Promise<{ stdout: string; stderr: string; exitCode: number }>;

export function runHostScript(
  host: HostDefinition,
  scripts: { posix?: string; powershell?: string },
  options?: {
    timeoutMs?: number;
    input?: string | Buffer | Uint8Array;
    sshCommand?: string;
  }
): Promise<{ stdout: string; stderr: string; exitCode: number }>;
```

---

## 2. Host Configuration Schema & Security Invariants

### 2.1 Schema Structure
- **`schemaVersion`**: Integer `1`.
- **`repo`**: Strictly string `'Indosaram/ferryx'`.
- **`repository`**: Absolute local repository path (e.g. `/Users/indo/code/project/orca-lite`).
- **`ghosttyRepository`**: Absolute local pinned Ghostty repository path (e.g. `/Users/indo/code/project/orca-lite/src-tauri/vendor/ghostty`).
- **`hosts`**: Object containing strictly the three required builders:
  1. `macbook`: Coordinator & macOS builder (`darwin`, `ssh: null`, local execution).
  2. `omaki`: Linux builder (`linux`, SSH destination string, `bash -s` transport).
  3. `maho-win`: Windows builder (`win32`, SSH destination string, PowerShell base64 UTF-16LE `-EncodedCommand` transport).

### 2.2 Strict Invariants Enforced by `validateHostConfig`
1. **Unknown Properties Rejection**: Unknown root or host-level properties (such as `$schema` or arbitrary fields) are rejected immediately.
2. **Secret & Credential Prohibition**: Keys and string values are scanned across all levels:
   - Secret key patterns (`secret`, `password`, `token`, `credential`, `private`, `auth`, `bearer`, `apikey`) cause immediate rejection.
   - Secret value patterns (`-----BEGIN ... PRIVATE KEY`, `ghp_...`, `github_pat_...`, bearer tokens) cause immediate rejection.
   - Credential file paths (`.pem`, `.key`, `.p12`, `.pfx`, `id_rsa`, `id_ed25519`) cause immediate rejection.
3. **Transport Security**:
   - `macbook.ssh` must strictly be `null`.
   - Remote SSH destinations (`omaki`, `maho-win`) must not start with `-` (preventing SSH option injection) and must not contain spaces or shell metacharacters.
4. **Platform Binding**: Each host is strictly bound to its assigned platform (`macbook: "darwin"`, `omaki: "linux"`, `maho-win: "win32"`).
5. **Path Normalization & Separation**:
   - Relative paths are rejected.
   - Windows build roots must use forward slashes (e.g. `C:/Users/sook/ferryx-releases`), forbidding backslash escaping issues.
   - Build roots must never equal `repository`.
6. **Disk Budget Safety**:
   - `minFreeBytes` must be a positive integer (> 0). Default policy budgets: Mac 30GiB (`32212254720`), Linux 20GiB (`21474836480`), Windows 20GiB (`21474836480`).
7. **Apple Identity & Notary Scope**:
   - `notaryProfile` and `signingIdentity` are restricted strictly to `macbook` (`darwin`) and rejected if configured on Linux or Windows builders.
8. **Toolchain Expectations**:
   - Optional `expectedTools` permits only `bun`, `node`, `zig`, `rust`, and `tauri` version strings.

---

## 3. Shell Quoting & Process Transport

### 3.1 Shell Quoting Primitives
- **`quoteSh(value)`**: Safely quotes strings for POSIX `sh`/`bash` using single-quote wrapping with internal quote replacement `'\''`. Preserves newlines, spaces, variable symbols, backticks, and unicode without accidental shell expansion.
- **`quotePowerShell(value)`**: Safely quotes strings for Windows PowerShell literal single quotes with internal quote doubling `''`. Eliminates JS backslash escaping issues and prevents PowerShell subexpression expansion.

### 3.2 Subprocess Transport (`runProcess`)
- Spawns subprocesses using `node:child_process.spawn` with an explicit `argv` array (no shell parsing).
- **Event Subscription Order**: `child.on("error")` and `child.on("close")` listeners are registered **before** any data is written to `child.stdin`. This prevents race conditions, crashes, or unhandled `EPIPE` exceptions if the target process exits immediately.
- **Process Group Isolation & Bounded Cleanup**:
  - On POSIX platforms, processes are spawned with `detached: true` so the child becomes the leader of its own process group (`PGID = child.pid`).
  - Upon `timeoutMs` expiry, the transport sends `SIGTERM` to the process group (`process.kill(-child.pid, "SIGTERM")`), escalating to `SIGKILL` after 250ms. This cleanly terminates hanging process trees without leaving orphaned background processes or affecting the coordinator.
- **Error Transparency & Secret Redaction**:
  - Non-zero exits reject with `ProcessExecutionError` containing `exitCode`, `stdout`, `stderr`, and `command`.
  - Process environment variables and secrets are never serialized into `error.message` or stderr summaries.

### 3.3 Host Transport Dispatch (`runHostScript`)
- **macOS (`macbook`)**: Executes scripts locally using `bash -s` with the POSIX script supplied via standard input.
- **Linux (`omaki`)**: Executes scripts over SSH with standard flags:
  `-o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=5 -o ServerAliveCountMax=2 <sshDest> bash -s`
  The POSIX script is streamed via stdin, preventing shell injection or unquoted username command issues.
- **Windows (`maho-win`)**: Executes scripts over SSH via PowerShell with flags:
  `-o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=5 -o ServerAliveCountMax=2 <sshDest> powershell -NoProfile -NonInteractive -EncodedCommand <base64>`
  The PowerShell payload is wrapped with `$ProgressPreference = 'SilentlyContinue'` and native `$LASTEXITCODE` / terminating error trapping, then encoded as UTF-16LE Base64.

---

## 4. Verification & Testing Evidence

### 4.1 Test Suite Summary (`scripts/release-hosts.test.mjs`)
- **Node.js Test Runner**: 28 passed, 0 failed (5.5s)
- **Bun Test Runner**: 28 passed, 0 failed (5.6s)

### 4.2 Key Scenarios Verified
1. Canonical configuration loading from `scripts/release-hosts.example.json`.
2. Rejection of unknown properties, `$schema` URLs, and non-canonical schema versions.
3. Secret scanning detecting embedded tokens, private keys, passwords, and credential paths.
4. Enforcement of `ssh: null` for `macbook` and rejection of option-like SSH destinations (`-oProxyCommand=...`).
5. Windows root forward-slash validation and path-to-repo collision checks.
6. POSIX and PowerShell quoting edge cases (spaces, quotes, newlines, backslashes, unicode).
7. Subprocess stdout/stderr buffering, stdin streaming, and fast-exit EPIPE immunity.
8. Bounded timeout killing detached process trees without orphans.
9. Secret-free error reporting on non-zero exit codes.
10. End-to-end SSH transport verification using executable shims that decode argv and base64 UTF-16LE PowerShell payloads.
11. Real read-only host command execution on `macbook` (`uname -s` -> `Darwin`).
12. Live read-only smoke probe on `maho-win` (`Write-Output ('LIVE_PROBE_HOST='+$env:COMPUTERNAME)` -> `DESKTOP-1LAPJMP`).
