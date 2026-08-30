# Windows Edge Completion Audit

Audit target: final evidence tree at `04ed078`

Host: `maho-win`

Isolated checkout: `C:\Users\sook\ferryx-ulw-01a04fcf`

The audit was performed because criterion C002 literally names edge surfaces that were not individually enumerated in the original final summary.

## Isolation

The user's active dirty-checkout Ferryx processes held the normal `%LOCALAPPDATA%\Ferryx\runtime\daemon.lock`. They were not stopped or modified.

The audit daemon used isolated environment roots under:

`C:\Users\sook\ferryx-ulw-01a04fcf\.edge-runtime`

This supplied private `LOCALAPPDATA`, `APPDATA`, `daemon.lock`, and `daemon.port` paths.

## Results

```text
STALE_RUNTIME_RECOVERY_PASS port=64106 lockExists=True
UNREGISTERED_WORKSPACE_PASS message="Workspace 'edge-ws' is not registered"
MISSING_SHELL_DEFAULT_PASS
EMPTY_SHELL_DEFAULT_PASS
INVALID_SHELL_PASS message="Failed to spawn process ... os error 3"
MISSING_CWD_PASS message="CWD does not exist: ...\\definitely-missing-cwd"
OUTSIDE_CWD_PASS message="CWD 'C:\\Users\\sook' is outside workspace ..."
WSL_OUTPUT_NOT_RUN environment=direct-wsl-create-instance-failed
WINDOWS_PROTOCOL_EDGES_PASS
MANIFEST_TEST_STARTUP_PASS
EDGE_AUDIT_CLEANUP daemonProcesses=0 portExists=False
WINDOWS_UNCOVERED_EDGES_PASS
```

### Shell selection

- Omitted `shell` spawned the normal Windows default shell.
- Empty `shell` was normalized to the default shell.
- A nonexistent absolute shell path returned a structured daemon error with `CreateProcessW` `os error 3`.

### Workspace and CWD boundaries

- Spawn before workspace registration returned `Workspace 'edge-ws' is not registered`.
- A nonexistent CWD returned `CWD does not exist`.
- An existing absolute path outside the registered root returned a structured workspace-escape error.

### Stale runtime state

The isolated runtime was pre-seeded with:

- `daemon.port` containing invalid stale port `9`;
- an unlocked stale `daemon.lock` regular file.

The daemon acquired the stale lock file and replaced port `9` with a valid bound port. Cleanup removed the isolated runtime tree.

### Executable manifest/startup boundary

The final Windows Rust test executable launched and ran six CLI launcher tests successfully. This is a real linked test executable and therefore exercises the Common-Controls v6 manifest startup boundary that previously failed before test execution.

### WSL and WSLg boundary

Direct host execution, outside Ferryx, produced:

```text
WSL version: 2.7.10.0
WSLg version: 1.0.73.2
Distribution: Ubuntu, version 2, state Stopped
Services: WslService Running, vmcompute Running, hns Running
wsl.exe -e sh -lc ... -> Wsl/Service/CreateInstance/E_FAIL, exit -1
```

Ferryx could spawn `wsl.exe`, but the guest never reached a shell prompt because the host WSL service could not create the Ubuntu instance. The same failure occurred when invoking `wsl.exe` directly, so no Ferryx product failure is established. No destructive `wsl --shutdown`, service restart, or user-process termination was performed.

The original manual WSLg pixel checklist remains required after the host's WSL instance is operational. WSL output is correctly marked NOT RUN rather than PASS.

## Reproducible artifacts

- `evidence/windows-edges/probe-daemon-edges.mjs`
- `evidence/windows-edges/run-edge-probes.ps1`

## Verdict

All headless Ferryx edge requirements named by C002 are now directly covered. WSL shell output is blocked by an independently reproduced host WSL failure, and visible WSLg presentation remains explicitly user-owned. This does not justify a Ferryx code change or a false PASS.
