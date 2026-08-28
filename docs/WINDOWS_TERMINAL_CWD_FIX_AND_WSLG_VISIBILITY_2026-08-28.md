# Windows Terminal CWD Fix and WSLg Visibility — 2026-08-28

## Corrected findings

The earlier verification was insufficient:

- A live Windows Ferryx process did not prove that its terminal opened in the
  requested directory.
- A mapped WSLg X11 window did not prove that the window was visible to the
  user; it was covered by Windows Terminal and File Explorer windows created or
  left in front during remote QA.

## Windows terminal root cause

`fs::canonicalize` returns verbatim drive paths on Windows, for example:

```text
\\?\C:\Windows\System32
```

The daemon passed that path directly to `portable-pty` as the child process
working directory. `cmd.exe` interprets the verbatim prefix as a UNC path,
prints the following error, and falls back to `C:\Windows`:

```text
UNC paths are not supported. Defaulting to Windows directory.
```

The fix keeps the canonical/verbatim path for all workspace security and
containment comparisons. Only the final value passed to
`CommandBuilder::cwd` is converted to a process-compatible path:

- `\\?\C:\path` becomes `C:\path`.
- `\\?\UNC\server\share` becomes `\\server\share`.
- Normal drive, UNC, Unix, relative, and unsupported verbatim volume paths are
  unchanged.

## Test evidence

The regression test was observed failing before the implementation:

```text
left:  "\\\\?\\C:\\Windows\\System32"
right: "C:\\Windows\\System32"
test result: FAILED. 0 passed; 1 failed
```

After the fix:

```text
test daemon::server::tests::test_normalize_process_cwd_windows_verbatim_paths ... ok
test result: ok. 1 passed; 0 failed
```

Additional checks:

- `cargo check --manifest-path src-tauri/Cargo.toml --lib`: passed.
- `cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-gnu --lib`: passed.
- Windows native `cargo build --release`: passed after stopping the old Ferryx
  process that had locked `target\release\ferryx.exe`.
- `rustfmt --edition 2021 --check src-tauri/src/daemon/server.rs`: passed.
- LSP diagnostics for `src-tauri/src/daemon/server.rs`: no errors.

## Real Windows PTY verification

The rebuilt Windows binary was started in daemon mode. The production daemon
TCP protocol was used to register `C:\Windows\System32`, spawn a real ConPTY
session, write a command, and attach to its output.

Observed output:

```text
Microsoft Windows [Version 10.0.26200.9168]
sook@DESKTOP-1LAPJMP C:\Windows\System32>echo FERRYX_WINDOWS_CWD_FIX_OK
FERRYX_WINDOWS_CWD_FIX_OK
sook@DESKTOP-1LAPJMP C:\Windows\System32>cd
C:\Windows\System32
sook@DESKTOP-1LAPJMP C:\Windows\System32>
WINDOWS_PTY_CWD_OK=1
```

The previous UNC warning was absent.

## WSLg visibility

The WSLg Ferryx process, daemon, and runtime sockets were alive. X11 reported:

```text
Map State: IsViewable
Absolute upper-left X: 986
Absolute upper-left Y: 77
Width: 900
Height: 900
```

This proves the WSLg window was mapped and inside the desktop bounds. It does
not prove that the user could see it, because other Windows windows were above
it. No Ferryx code change is justified for that observation.

## Required desktop confirmation

Because direct manipulation of the user's desktop is not permitted, the final
visual confirmation must be performed by the user:

1. Minimize the Windows Terminal and File Explorer windows currently covering
   Ferryx.
2. Confirm the native Windows Ferryx window is visible.
3. In that window, open a new terminal and run `cd`; confirm it prints the
   selected workspace directory rather than `C:\Windows`.
4. Start WSLg Ferryx from an existing WSL shell:

   ```bash
   DISPLAY=:0 GDK_BACKEND=x11 \
     /home/sook/ferryx-wsl-build/src-tauri/target/release/ferryx
   ```

5. Confirm the separate WSLg Ferryx window is visibly unobscured.
