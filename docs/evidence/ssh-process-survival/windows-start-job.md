# Windows SSH detached startup investigation

## RED observation

The verifier's actual OpenSSH process 26394 remained open for over 15 minutes
after `ferryx-remote-helper start` had returned. Remote process inspection found
only daemon PID 17364 (parent 25868, already absent), with this exact QA ownership:

- Binary: `C:\Users\sook\AppData\Local\Temp\test-build-probe\remote-helper\target\debug\ferryx-remote-helper.exe`.
- Root: `C:\Users\sook\AppData\Local\Temp\test-root-direct`.
- Host: `test-host`.

The start command itself no longer existed, so its readiness loop was not the
blocking operation. A detached console/process group does not by itself remove
a process from its parent's Windows job. `CREATE_BREAKAWAY_FROM_JOB` is being
tested on an isolated copied debug helper as the next hypothesis, not claimed
as a proven fix. Existing processes and fixed-path fixtures must be cleaned by
exact verified ownership, never broad process-name matching.

## Rejected flag-only attempt

The isolated helper with `CREATE_BREAKAWAY_FROM_JOB` compiled and printed ready,
and its SSH command exited 0. A new SSH bridge nevertheless failed with Windows
error 10061 (connection refused). A remote process query confirmed no helper
for that unique fixture. This attempt is rejected; those flags are not the fix.

## Independent process creation proof

A direct `Win32_Process.Create` call with detached startup created only the QA
helper as PID 26980. After the creating SSH connection exited 0, a separate real
OpenSSH binary-framed handshake returned:

```json
{
  "ok": true,
  "data": {
    "protocol": 1,
    "hostId": "qa-cim-b20977b8-da14-4961-b1d5-fad16d809f8f",
    "ownerId": "ae5cc881-a98c-4671-96bc-87328c795978",
    "epoch": "1514131288936775983",
    "os": "windows",
    "arch": "x86_64",
    "capabilities": ["sshHelperV1"]
  }
}
```

PID 26980 was identity-checked and stopped before rebuilding the fixture.
The production start path now isolates this Windows process-creation operation
in `process/process_windows.rs`; POSIX retains its session-detached child path.
The comment distinguishing CIM ownership from console detachment is necessary
to preserve this non-obvious, experimentally verified lifetime requirement.
Windows-specific commands have bounded completion and process-ID-scoped cleanup.
The helper authenticates readiness before returning, not just process existence.

Current validation: Windows debug build passed (`windows-cim-build.log`), and
native helper regression passed 23/23 (`startup-recovery-native.log`). Final
start/reattach and complete fixture cleanup were subsequently verified below.

## Final real-surface result

`windows-cim-start.log` records the actual helper `start` command returning ready
and its OpenSSH parent exiting 0. A fresh SSH connection then authenticated to
the still-running helper. One nonce-bearing PowerShell process was spawned.
Only its local QA SSH transport (PID 47164) was killed, exit 137.

`windows-cim-session.log` records the new SSH connection's `pty.describe` and
counter interaction, with no second spawn:

- Remote PID: 27964, unchanged.
- Remote session: `dc0ad535-d7d4-4f0f-96ad-6b02d0ba2842`, unchanged.
- Nonce: `e5f64559-1c4e-4fae-a6e1-ca2fd5cb951c`, unchanged.
- In-memory counter: 1 to 2.
- Helper epoch: `3930053837692487182`, unchanged.

The helper was explicitly told to stop that PTY; the final SSH exited 0.
`windows-cim-cleanup.log` records exact QA helper PID 20324 reaped and the unique
runtime/build directory and both transferred archives removed. Both local
archives and the source-recovery directory were removed too.
The abandoned earlier setup fixture was also confirmed removed in
`windows-old-qa-cleanup.log`; no helper processes were reported afterward.

The integrated desktop crate passed `cargo check` (`startup-recovery-check.log`).
Known-host/config SHA-256 values still match `ssh-trust-baseline.md`.
This verifies the Windows remote-helper/SSH layer, not the still-unimplemented
desktop daemon restart or UI automatic recovery.
