# Manual QA Matrix — `st_01a07fc8` (Host Configuration & Process Transport)

**Goal:** Implement and verify the independent host configuration and process-transport boundary for local releases (`scripts/lib/release-hosts.mjs`, `scripts/release-hosts.test.mjs`, `scripts/release-hosts.example.json`, `docs/releases/local-release-hardening-evidence/hosts.md`) against the binding contract (`docs/releases/local-pipeline-audit-2026-09-08/IMPLEMENTATION_CONTRACT.md`).

**Overall verdict: PASS (Zero Actionable Blockers)**

All criteria and adversarial cases have been verified against real Node.js/Bun runtimes, process spawning surfaces, executable SSH transport fixtures, and live read-only host endpoints:
1. **Host Configuration Contract (`validateHostConfig` / `loadHostConfig`):** Enforces strict `schemaVersion: 1`, repo `Indosaram/ferryx`, absolute paths for `repository` and `ghosttyRepository`, and strictly three builders (`macbook`, `omaki`, `maho-win`). Rejects unknown root or host properties, relative paths, Windows backslashes in roots, and roots equal to repository.
2. **Zero Credential & Secret Storage:** Prohibits credential/secret property keys (`secret`, `password`, `token`, `credential`, `private`, `auth`, `bearer`, `apikey`) and scans all string values for secret patterns (private key blocks, OAuth/PAT tokens, passwords, and `.pem`/`.key`/`.p12`/`id_ed25519` keyfile paths).
3. **Transport Protocol Binding & Injection Prevention:** Restricts `macbook.ssh` to strictly `null`. Remote builder SSH destinations must be valid hostnames/IPs and reject option injection (e.g. `-oProxyCommand=...`).
4. **Platform-Specific Execution Protocol (`runHostScript`):**
   - `macbook` (darwin): Local `bash -s` with POSIX script streamed to stdin.
   - `omaki` (linux): SSH transport with `BatchMode=yes`, `ConnectTimeout=10`, `ServerAliveInterval=5`, `ServerAliveCountMax=2` piping POSIX script to remote `bash -s`.
   - `maho-win` (win32): SSH transport executing PowerShell with `BatchMode=yes`, `-NoProfile`, `-NonInteractive`, and base64 UTF-16LE `-EncodedCommand` payload ensuring `$ProgressPreference = 'SilentlyContinue'` and explicit exit status handling.
5. **Robust Subprocess Isolation (`runProcess`):** Uses `node:child_process.spawn` with argv array; registers error and exit subscriptions before writing to stdin; cleanly terminates detached process groups upon bounded timeout without orphaned child processes; reports typed errors without dumping environment secrets.
6. **Literal Shell Quoting (`quoteSh`, `quotePowerShell`):** Safely escapes POSIX strings via single-quote wrapping and Windows PowerShell strings via single-quote doubling, eliminating JS backslash escaping issues.

---

## surfaceEvidence

| Scenario ID | Criterion reference | Surface | Exact invocation | Verdict | Evidence / exact result | artifactRefs |
|---|---|---|---|---|---|---|
| SURF-01 | Host Config Loading & Normalization | Pure ESM API / JSON Parsing | `loadHostConfig("scripts/release-hosts.example.json")` | **PASS** | Successfully parsed and normalized 3 hosts (`macbook`, `omaki`, `maho-win`), verified schemaVersion 1, valid platform mappings and forward slash Windows paths. | ART-01 |
| SURF-02 | POSIX Shell Quoting | Pure ESM API | `quoteSh(value)` across spaces, quotes, newlines, and unicode | **PASS** | Validated escaping: `'don'\''t'`, preserved newlines, variable syntax, and unicode `'한글 경로'`. | ART-02 |
| SURF-03 | Windows PowerShell Quoting | Pure ESM API | `quotePowerShell(value)` across Windows paths and special characters | **PASS** | Validated single-quote doubling: `'C:\Users\sook\path with spaces'`, no JS backslash escaping required. | ART-03 |
| SURF-04 | Subprocess Stdout/Stderr Buffering | Node Subprocess CLI | `runProcess("node", ["-e", "process.stdout.write(...); process.stderr.write(...)"])` | **PASS** | Captured stdout and stderr independently; exitCode 0. | ART-04 |
| SURF-05 | Subprocess Stdin Streaming | Node Subprocess CLI | `runProcess("node", ["-e", "process.stdin.pipe(process.stdout)"], { input })` | **PASS** | Successfully piped stream payload via stdin; registered listeners before input without EPIPE errors. | ART-05 |
| SURF-06 | Local macOS Transport Execution | Subprocess CLI (`bash -s`) | `runHostScript(hosts.macbook, { posix: "uname -s" })` | **PASS** | Executed locally via `bash -s`; returned `Darwin\n` with exitCode 0. | ART-06 |
| SURF-07 | Remote Linux SSH Transport | Executable SSH Transport Fixture | `runHostScript(hosts.omaki, { posix: "echo LINUX_REMOTE_RUN" }, { sshCommand: fakeSsh })` | **PASS** | Fixture verified exact argv (`-o BatchMode=yes -o ConnectTimeout=10 ... omaki bash -s`) and piped script payload to stdin. | ART-07 |
| SURF-08 | Remote Windows SSH PowerShell Transport | Executable SSH Transport Fixture | `runHostScript(hosts["maho-win"], { powershell: "Write-Output 'POWERSHELL_PAYLOAD'" }, { sshCommand: fakeSshWin })` | **PASS** | Fixture decoded base64 UTF-16LE payload; verified `$ProgressPreference = 'SilentlyContinue'` and exit status trapping. | ART-08 |
| SURF-09 | Live Remote Windows Smoke Probe | Remote SSH / PowerShell | `runHostScript(hosts["maho-win"], { powershell: "Write-Output ('LIVE_PROBE_HOST=' + $env:COMPUTERNAME)" })` | **PASS** | Live probe to `maho-win` returned `LIVE_PROBE_HOST=DESKTOP-1LAPJMP` with exitCode 0; read-only with zero filesystem writes. | ART-09 |

---

## adversarialCases

| Scenario ID | Criterion reference | Adversarial class | Expected behavior | Verdict | Evidence | artifactRefs |
|---|---|---|---|---|---|---|
| ADV-01 | Host Matrix Completeness | Missing required host | Omitting any host (`omaki`) must reject immediately. | **PASS** | Throws: `Missing required host: 'omaki'`. | ART-10 |
| ADV-02 | Host Matrix Integrity | Rogue / extra host | Declaring unauthorized host (`rogue-builder`) must reject immediately. | **PASS** | Throws: `Unknown host in config: 'rogue-builder'`. | ART-11 |
| ADV-03 | Schema Strictness | Hosted schema URL ($schema) / unknown field | Supplying `$schema` or arbitrary root property must fail closed. | **PASS** | Throws: `Unknown property in host config: '$schema'`. | ART-12 |
| ADV-04 | Credential Protection | Secret keyword in property name | Declaring `apiToken` or `privateKey` in config must fail closed. | **PASS** | Throws: `Secret or credential field detected: 'apiToken' at config.hosts.omaki`. | ART-13 |
| ADV-05 | Credential Protection | Embedded secret token value | Declaring PAT or private key block in value must fail closed. | **PASS** | Throws: `Secret or credential pattern detected in value at config.hosts.macbook.signingIdentity`. | ART-14 |
| ADV-06 | Credential Protection | Credential file path | Supplying private key path (`id_ed25519`) must fail closed. | **PASS** | Throws: `Credential file path detected in config at config.hosts.omaki.path: '/Users/indo/.ssh/id_ed25519'`. | ART-15 |
| ADV-07 | Transport Security | SSH option injection | Supplying `-oProxyCommand=...` in SSH destination must fail closed. | **PASS** | Throws: `Invalid SSH destination for host 'omaki': '-oProxyCommand=rm -rf /'`. | ART-16 |
| ADV-08 | Transport Security | Malformed SSH destination | Supplying whitespace or shell characters in destination must fail closed. | **PASS** | Throws: `Invalid SSH destination for host 'omaki': 'user host with spaces'`. | ART-17 |
| ADV-09 | Platform Binding | Host platform mismatch | Declaring `macbook` as `linux` must fail closed. | **PASS** | Throws: `Host 'macbook' platform must be 'darwin', got 'linux'`. | ART-18 |
| ADV-10 | Path Safety | Relative repository path | Supplying `./relative/orca-lite` must fail closed. | **PASS** | Throws: `repository must be an absolute path: './relative/orca-lite'`. | ART-19 |
| ADV-11 | Path Safety | Windows root backslashes | Supplying `C:\Users\sook\ferryx-releases` must fail closed. | **PASS** | Throws: `Windows root must use forward slashes: 'C:\Users\sook\ferryx-releases'`. | ART-20 |
| ADV-12 | Path Safety | Root equals repository | Setting build root equal to source repository must fail closed. | **PASS** | Throws: `Host root must not equal repository: '/Users/indo/code/project/orca-lite'`. | ART-21 |
| ADV-13 | Budget Safety | Invalid minFreeBytes | Negative, zero, or float disk budget must fail closed. | **PASS** | Throws: `minFreeBytes must be a positive integer for host 'macbook', got -500`. | ART-22 |
| ADV-14 | macOS Profile Scope | notaryProfile on non-macOS host | Specifying `notaryProfile` on Linux or Windows builder must fail closed. | **PASS** | Throws: `notaryProfile is only permitted on macOS (darwin), got on 'omaki'`. | ART-23 |
| ADV-15 | Toolchain Validation | Unknown toolchain tool | Supplying `go` in `expectedTools` must fail closed. | **PASS** | Throws: `Unknown tool in expectedTools: 'go'`. | ART-24 |
| ADV-16 | Information Disclosure | Non-zero exit secret redaction | Process failure must not leak secret environment variables in message. | **PASS** | Error message `Command failed with exit code 42: node` does not contain secret token; typed as `ProcessExecutionError`. | ART-25 |
| ADV-17 | Process Lifecycle | Bounded timeout process kill | Hanging subprocess must be killed within bounded time without leaving orphan processes. | **PASS** | Rejected after 150ms with `ProcessTimeoutError`; process group killed with zero orphan leaks. | ART-26 |
| ADV-18 | Protocol Enforcement | Host script type mismatch | Passing `powershell` script to macOS host must fail closed. | **PASS** | Throws: `POSIX script required for darwin host`. | ART-27 |

---

## artifactRefs

| ID | Kind | Description | Path |
|---|---|---|---|
| ART-01 | JSON Log | Output of `loadHostConfig("scripts/release-hosts.example.json")` | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/surf-01-load-config.log` |
| ART-02 | JSON Log | Results of `quoteSh` across POSIX test string corpus | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/surf-02-quote-sh.log` |
| ART-03 | JSON Log | Results of `quotePowerShell` across Windows test string corpus | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/surf-03-quote-powershell.log` |
| ART-04 | JSON Log | Subprocess stdout and stderr capture output | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/surf-04-run-process-stdout.log` |
| ART-05 | JSON Log | Subprocess stdin piping and buffered echo output | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/surf-05-run-process-stdin.log` |
| ART-06 | JSON Log | Local macOS host script `uname -s` execution record | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/surf-06-macbook-host-script.log` |
| ART-07 | JSON Log | Linux SSH transport shim recorded args and stdin payload | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/surf-07-omaki-ssh-transport.log` |
| ART-08 | JSON Log | Windows SSH transport shim recorded args and decoded UTF-16LE script | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/surf-08-maho-win-powershell-transport.log` |
| ART-09 | JSON Log | Real read-only smoke probe response from live `maho-win` builder | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/surf-09-maho-win-live-smoke.log` |
| ART-10 | Error Text | Rejection record for missing required host (`omaki`) | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-01-missing-host.log` |
| ART-11 | Error Text | Rejection record for unauthorized host (`rogue-builder`) | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-02-unknown-host.log` |
| ART-12 | Error Text | Rejection record for `$schema` unknown property | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-03-unknown-property.log` |
| ART-13 | Error Text | Rejection record for secret keyword in property key (`apiToken`) | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-04-secret-key.log` |
| ART-14 | Error Text | Rejection record for embedded PAT token value | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-05-secret-token-value.log` |
| ART-15 | Error Text | Rejection record for private keyfile path in config | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-06-credential-path.log` |
| ART-16 | Error Text | Rejection record for SSH option injection (`-oProxyCommand`) | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-07-ssh-option-injection.log` |
| ART-17 | Error Text | Rejection record for malformed SSH destination with spaces | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-08-ssh-malformed-destination.log` |
| ART-18 | Error Text | Rejection record for host platform mismatch | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-09-platform-mismatch.log` |
| ART-19 | Error Text | Rejection record for relative repository path | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-10-relative-path.log` |
| ART-20 | Error Text | Rejection record for Windows backslash root path | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-11-win-backslash-path.log` |
| ART-21 | Error Text | Rejection record for host root matching repository | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-12-root-equals-repo.log` |
| ART-22 | Error Text | Rejection record for negative minFreeBytes value | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-13-invalid-min-free-bytes.log` |
| ART-23 | Error Text | Rejection record for notaryProfile on non-macOS host | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-14-notary-on-non-mac.log` |
| ART-24 | Error Text | Rejection record for unknown tool in expectedTools | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-15-unknown-tool.log` |
| ART-25 | JSON Log | Validation record proving secret environment variables are not leaked on process error | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-16-no-secret-leak.log` |
| ART-26 | JSON Log | Validation record of bounded timeout killing process tree | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-17-timeout-kill-tree.log` |
| ART-27 | Error Text | Rejection record for script type mismatch on darwin | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/adv-18-script-mismatch.log` |
| ART-28 | Test Runner Log | Initial RED failure log before implementation | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/qa-01-initial-red.log` |
| ART-29 | Test Runner Log | Complete Node.js test runner output (28 passed) | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/qa-02-node-test-suite.log` |
| ART-30 | Test Runner Log | Complete Bun test runner output (28 passed) | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc8/qa-03-bun-test-suite.log` |
