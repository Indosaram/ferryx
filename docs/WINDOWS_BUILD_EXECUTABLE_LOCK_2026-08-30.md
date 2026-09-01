# Windows Build Executable Lock Diagnosis — 2026-08-30

## Result

The reported Windows build failure was reproduced in the active working checkout:

```text
C:\Users\sook\ferryx-winbuild\orca-lite
```

It was not a Rust, TypeScript, Zig, Ghostty, MSVC, or Windows SDK compilation failure. Eight existing Ferryx processes from the same checkout held the debug executable open, so Cargo could not replace it:

```text
error: failed to remove file `C:\Users\sook\ferryx-winbuild\orca-lite\src-tauri\target\debug\ferryx.exe`

Caused by:
  Access is denied. (os error 5)
```

Windows does not allow Cargo to replace a running executable.

## Recovery

Only processes whose executable path exactly matched the affected checkout were stopped:

```text
C:\Users\sook\ferryx-winbuild\orca-lite\src-tauri\target\debug\ferryx.exe
```

Process count:

```text
LOCKING_BEFORE=8
LOCKING_AFTER=0
```

No source file was modified as part of the recovery.

## Verification

The same dirty working tree then passed:

```text
cargo build --manifest-path src-tauri/Cargo.toml
```

The supported desktop command also completed its build and started the application:

```text
bun tauri dev
Finished `dev` profile [unoptimized + debuginfo] target(s) in 45.60s
Running `target\debug\ferryx.exe`
```

Observed runtime processes:

```text
GUI_COUNT=1
DAEMON_COUNT=1
```

The GUI and daemon launched for QA were then stopped. The final nonzero wrapper status was caused by intentionally stopping the running QA application, not by a build failure.

## User recovery command

If this exact error returns, close every Ferryx window and daemon built from the checkout, then run:

```powershell
Get-Process ferryx -ErrorAction SilentlyContinue | Stop-Process -Force
bun tauri dev
```

When multiple Ferryx checkouts are in use, prefer terminating only the process whose `ExecutablePath` points to the checkout being rebuilt.
