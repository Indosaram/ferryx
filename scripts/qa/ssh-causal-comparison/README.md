# Isolated Windows OpenSSH causal comparison

This is an opt-in experiment harness for `maho-win`; it does not build or
modify Ferryx, restart the installed service, edit global OpenSSH configuration,
or target an existing process. `start` creates a unique run directory and
unique Task Scheduler task, then starts existing inbox `sshd.exe` using SYSTEM
on `127.0.0.1` only. It returns without stopping the private server, enabling a
separately launched Ferryx phase to connect. Authentication uses public key
as the existing local account `sook`. The fixture creates a dedicated client
key and `authorized_keys` file; that client key is distinct from the SSH
server host key. It never creates or changes a Windows account.

The task is created and run only when the parent executes `start`. The unique
task is deleted only by explicit `cleanup` (`/End` then `/Delete`); the private
server PID is then considered only if both executable path and process creation
time match the manifest. No global task query/stop or process-name kill is used.
SSH clients are spawned/waited as direct children, then terminated by their own
`Popen` handles if still active. The installed SSH listener is port 22; 20044
is a PID and is not a port. The fixture uses its own loopback port.

## Prerequisites

- Windows 11 Pro, baseline build 26200; elevated PowerShell.
- Inbox OpenSSH binaries at `%WINDIR%\System32\OpenSSH\sshd.exe`,
  `ssh.exe`, and `ssh-keygen.exe`; override paths only if the same inbox
  version is installed elsewhere.
- Python 3.10+ on PATH, and this directory present on `maho-win`.
- The parent must first copy the two new files to `maho-win`; deployment and
  remote execution are intentionally outside this turn.
- The existing local `sook` account must be enabled and SSH-capable. The
  harness checks that precondition and does not modify account state.
- Port 40222 available on loopback. Select another unused unprivileged port
  with `--port` if needed; the script binds only `127.0.0.1`.
- Elevated rights to register/run a SYSTEM scheduled task. Its one-time trigger
  is dated one year ahead and disabled immediately after `/Run`; explicitly
  delete it with the cleanup command after phase 2.
- The machine's `C:\ProgramData` must be writable. A unique child directory
  grants full access only to SYSTEM and the invoking user; private host keys
  receive the same restricted ACL.

## Exact launch command

From elevated PowerShell in the repository root on `maho-win`, launch the
private listener. `start` prints the structured connection contract as JSON;
the last JSON record can be saved as `$result` as shown:

```powershell
$output = python .\scripts\qa\ssh-causal-comparison\run_windows_experiment.py start --port 40222 --timeout 15 --output "$env:ProgramData\Ferryx\ssh-causal-evidence"
if ($LASTEXITCODE -ne 0) { throw "Private sshd start failed: $output" }
$result = $output | ConvertFrom-Json
$evidence = $result.evidence_dir
if (-not (Test-Path (Join-Path $evidence 'READY'))) { throw 'Private sshd readiness sentinel missing' }
$env:FERRYX_SSH_TEST_HOST = '127.0.0.1'
$env:FERRYX_SSH_TEST_PORT = [string]$result.port
$env:FERRYX_SSH_TEST_KEY = $result.private_host_key
$env:FERRYX_SSH_TEST_CLIENT_KEY = $result.private_client_key
$env:FERRYX_SSH_TEST_IDENTITY = $result.identity_file
$env:FERRYX_SSH_TEST_KNOWN_HOSTS = $result.known_hosts
$env:FERRYX_SSH_TEST_CONFIG = $result.client_config
Write-Host "READY evidence=$evidence port=$($result.port) key=$($result.private_host_key) known_hosts=$($result.known_hosts) config=$($result.client_config)"
```

Keep that PowerShell session open while phase 2 runs. Pass the exported
`FERRYX_SSH_TEST_*` values to worker `st_01a0fd49`; `FERRYX_SSH_TEST_KEY` is
the fixture's server host private key and is not a client credential.
`FERRYX_SSH_TEST_IDENTITY` is the generated client key for account `sook`.
The worker's client must use `ClearAllForwardings=yes` and preserve the pinned
`known_hosts` entry. Once phase 2
and the plain client trials finish, use the exact cleanup command below:

```powershell
python .\scripts\qa\ssh-causal-comparison\run_windows_experiment.py cleanup --evidence $evidence
```

Plain SSH phases (run before cleanup):

```powershell
python .\scripts\qa\ssh-causal-comparison\run_windows_experiment.py trials --evidence $evidence --timeout 12
```

To make one direct client connection using the same pinned fixture inputs:

```powershell
& "$env:WINDIR\System32\OpenSSH\ssh.exe" -F $env:FERRYX_SSH_TEST_CONFIG -vv ferryx-causal-private "cmd.exe /d /c exit 0"
```

Expected result is successful public-key authentication as `sook`, execution
of `cmd.exe /d /c exit 0`, and normal disconnect. The measurement is whether
normal authenticated teardown causes a private sshd child to enter the same
CPU-bound preauth path.

Expected budget: 20 sequential authenticated commands, 5 waves of 4
concurrent authenticated commands, then 20 KEX-time auth cancellations and
one authenticated retry per cancellation (80 clients total). Normal trials
authenticate, run the exit command and disconnect; cancellation trials
terminate only after the OpenSSH debug reader observes
`SSH2_MSG_KEXINIT received`; they disable public-key authentication and send no
remote command. The subsequent retry re-enables the private client key and runs
the same `exit 0` command. No sleep is used to guess protocol state.
Each client and concurrent wave has a bounded timeout. The readiness sentinel
is `$evidence\READY`; cleanup removes it and creates `$evidence\CLEANED` only
after successful process-tree and task cleanup verification.

## Evidence and interpretation

Each unique `run-*` directory contains:

- `events.jsonl`: UTC timestamps, trial type/outcome, elapsed time, client PID,
  owned sshd child PIDs and their cumulative CPU seconds, task lifecycle.
- `manifest.json`: loopback endpoint, isolated server PID and process creation
  identity, private key path, pinned `known_hosts` and client config paths.
- `READY` / `CLEANED`: phase handoff and completed explicit cleanup sentinels.
- `sshd.stderr.log`: private instance log.
- Per-client `*-<pid>.stderr.log`: OpenSSH debug trace, including observed
  preauth protocol stage.
- `sshd_config` and generated private host-key files for reproducibility.

The baseline machine inventory reported three existing production workers
(PIDs 8760, 15092, 21056). They are recorded in the baseline inventory, outside
the private sshd PID tree, and are never cleanup targets. No global sshd-name
matching is used for trial CPU measurements or cleanup.
Evidence of a new sshd child spinning in this isolated listener establishes
the inbox server's local trigger independently of Ferryx. It does not establish
whether Ferryx causes the production service's trigger; the parent must perform
the separately authorized matched Ferryx comparison and classify that result.

Cleanup snapshots all descendants before ending the unique task; validates
each PID, parent PID, executable path and creation time; opens retained process
handles; ends/deletes only that task; terminates and waits for those exact
handles (10-second bounded wait per handle); then verifies no recorded process
or task remains. Any nonzero task, identity, handle, wait, or verification
result leaves cleanup failed and `CLEANED` absent. The process snapshot is
rooted at the unique private sshd PID and never selects the three baseline
production workers by name.

The cancellation/retry rounds are matched pairs within one server lifetime.
They measure whether retry after cancellation differs from cancellation alone;
they are not the separate reconnect comparison. To make reconnect rounds
meaningful, the parent should compare identical isolated-server runs with a
cleanly stopped and relaunched private instance between blocks, preserving the
same inbox binary, token, account probe name, and client options. No script
attempt is made to alter or restart the production service.

## Token caveat

The SYSTEM task gives the isolated server the same LocalSystem identity as the
installed service. Its scheduled-task process is a separately launched
`sshd.exe`, not the installed service and not the existing listener. If Windows
Task Scheduler denies registration or launch, stop: do not substitute an
elevated interactive sshd, since that changes the virtual-account path. The
parent should record the baseline token details and confirm the task process
identity before interpreting a negative result.
