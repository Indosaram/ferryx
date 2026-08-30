# Frozen Windows Verification Commands

Target commit: `7e47eb0545ad971856aad062ec800dccf8e268d6`

Target checkout: `C:\Users\sook\ferryx-ulw-01a04fcf`

## Strict Native Command Rule

PowerShell does not throw when a native executable exits nonzero. Every verification command must check `$LASTEXITCODE` immediately. A trailing PASS sentinel is invalid unless every preceding command had exit code 0.

## Focused Native Tests

Run sequentially to limit disk use:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib daemon::server::tests::test_daemon_exclusive_lock_semantics_rejects_duplicates_and_releases_on_drop -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib worktree::git::tests::git_path_argument_normalization_strips_windows_verbatim_prefixes -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::cli_install -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::native_terminal -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_surface_host_contract -- --test-threads=1
```

## Daemon Happy Path

1. Remove stale verification runtime files only after confirming no Ferryx daemon process is running.
2. Start `src-tauri\target\debug\ferryx.exe --daemon` from the isolated checkout.
3. Wait for `FERRYX_DAEMON_READY` in redirected stdout with a bounded deadline.
4. Run:
   ```powershell
   bun script/qa/win-daemon-e2e.mjs C:\Users\sook\ferryx-ulw-01a04fcf
   ```
5. Require the machine-readable summary fields `handshakeOk`, `errorProbeOk`, `spawnOk`, `writeOk`, `outputMarkerOk`, and `cwdOk` to all be true.
6. Require decoded PTY output to include `FERRYX_MARKER=` and `FERRYX_CWD=C:\Users\sook\ferryx-ulw-01a04fcf` (normalized comparison allowed).

## Duplicate Daemon Edge

While daemon 1 is alive, launch daemon 2 with redirected stderr. Require:

- daemon 2 exits nonzero,
- stderr contains `Another daemon instance is already holding the lock.`,
- `daemon.port` remains unchanged,
- daemon 1 remains alive and still responds to the E2E probe.

## Cleanup Receipt

After tests:

- close the E2E PTY session,
- terminate the verification daemon,
- confirm no verification `ferryx.exe --daemon` process remains,
- remove only the isolated runtime `daemon.port` if left stale,
- retain `daemon.lock` as an unlocked regular file if the application contract does so,
- record free disk space and clean Git status.
