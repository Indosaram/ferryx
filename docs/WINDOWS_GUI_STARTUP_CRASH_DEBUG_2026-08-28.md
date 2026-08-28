# Windows GUI Startup Crash Debug — 2026-08-28

## Outcome

The Windows GUI startup crash was reproduced, root-caused, fixed, rebuilt, and
verified on the remote Windows builder.

The fix is in `src-tauri/src/ipc/browser_cli.rs`. Windows now runs the browser
CLI protocol over a loopback TCP listener. The GUI writes the selected port to
`%LOCALAPPDATA%\Ferryx\runtime\browser.port`, and a second Ferryx process reads
that pointer to send browser CLI requests to the running GUI.

## Original failure

Running the release executable over SSH reproduced an immediate failure:

- Process: `ferryx.exe`
- Exit code: `101`
- Rust panic:

```text
Failed to setup app: error encountered during setup hook:
Platform unsupported: browser CLI transport requires a platform-specific local socket
```

WinDbg/CDB captured the same panic from the main thread. No Windows Error
Reporting crash dump was produced because this was a Rust panic during Tauri
setup rather than an access violation.

## Root cause

`create_app` calls `start_browser_cli_server` from the mandatory Tauri setup
hook. The Unix implementation starts a Unix-domain socket server. The previous
non-Unix implementation returned `BrowserError::PlatformUnsupported`.

Tauri propagates a setup-hook error as application setup failure, so the
optional browser CLI transport prevented the entire Windows desktop app from
starting.

The cause was confirmed by comparing launch paths:

| Launch path | Observed behavior |
| --- | --- |
| GUI | Immediate setup panic with exit code 101 |
| `--daemon` | Emitted `FERRYX_DAEMON_READY` and remained alive |
| `browser list` | Returned the expected platform-unsupported CLI error |

The executable's PE dependencies resolved, the pinned Ghostty library was
statically linked, and the daemon remained healthy. Those findings ruled out
missing DLLs, Ghostty linkage, and daemon startup as causes.

## Initial crash fix and completed transport

The first diagnostic build made unsupported server startup non-fatal to prove
that it was the cause. The final implementation does not leave the feature
disabled. It provides a real Windows transport:

- Synchronously bind `127.0.0.1:0` during the Tauri setup hook.
- Store the selected non-zero port in `browser.port`.
- Reject directory or symlink replacements for the pointer path.
- Replace stale regular pointer files on subsequent launches.
- Convert the nonblocking standard listener to a Tokio listener inside the
  Tauri async runtime.
- Reuse the same newline-delimited JSON protocol and request handler used by
  Unix-domain sockets.
- Read and validate `browser.port` in the Windows CLI process, then connect to
  `127.0.0.1:<port>`.
- Return typed unavailable errors for missing, malformed, zero, out-of-range,
  or stale port pointers.

## Regression coverage

Windows/non-Unix regression coverage includes:

1. Server startup without a caller-owned Tokio reactor.
2. TCP list request and response round-trip.
3. Port pointer read/write and replacement.
4. Missing, malformed, zero, and out-of-range port rejection.
5. Stale port connection failure.
6. Shared generic-stream request round-trip.

Verification performed:

- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::browser_cli`
  - 9 passed, 0 failed on the macOS host.
- `cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-gnu --tests`
  - Completed successfully, including compilation of the non-Unix regression
    tests.
- Rust language-server diagnostics for `browser_cli.rs`
  - No diagnostics.

The Windows MSVC test executable itself currently exits with
`STATUS_ENTRYPOINT_NOT_FOUND` before the test harness runs. This is an existing
native test-binary loader issue and is separate from the GUI startup panic. The
release application uses the same fixed Windows code path and was therefore
used for final runtime verification.

## Windows runtime verification

The corrected source was copied to the remote build tree and rebuilt with:

- Target: `x86_64-pc-windows-msvc`
- Zig: `0.16.0`
- Existing Tauri updater signing key
- Final Tauri build exit code: `0`

The rebuilt GUI executable was launched through the same SSH path that
previously reproduced the crash:

```text
SURVIVED_10S=True
RESPONDING=True
PANIC_PRESENT=False
CRASH_EVENTS=0
```

The Windows browser CLI transport was then exercised end to end with two real
processes:

```text
PORT_READY=True
PORT=57650
CLI_EXIT=0
CLI_STDOUT=[]
GUI_ALIVE=True
GUI_RESPONDING=True
```

`ferryx.exe browser list` therefore reaches the running Windows GUI and returns
the actual browser-session inventory. `[]` was correct because no browser tab
was open during that check.

It was also launched in the active Windows console session through a temporary
interactive scheduled task. Ferryx processes remained alive and responsive,
and no Application Error or Windows Error Reporting event was recorded. The
temporary processes and scheduled task were removed after verification.

Desktop visual interaction was not automated. A user-side visual check of the
window contents is still appropriate, but the reported startup crash is fixed
and no longer reproduces.
