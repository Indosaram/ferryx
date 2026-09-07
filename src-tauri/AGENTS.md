# src-tauri

<!-- Score: 16 | Domain: Rust backend & Tauri desktop core -->

## OVERVIEW
Rust backend powering Ferryx: Tauri v2 desktop shell, headless background PTY daemon, and authenticated Axum remote gateway.

## WHERE TO LOOK
| Task / Subsystem | Location | Notes |
|---|---|---|
| Desktop app bootstrap & AppKit hooks | `src/lib.rs`, `src/main.rs` | Tauri builder, menu handlers, unshifted Cmd+W monitor |
| Headless daemon & UDS protocol | `src/daemon/` | Unix domain socket server/client, protocol v2, launchd agent |
| Frontend IPC command layer | `src/ipc/` | Tauri invoke commands, binary stream framing, blocking offload |
| Remote HTTP & WebSocket gateway | `src/remote/` | Axum server, pairing PIN auth, active session lock |
| PTY lifecycle & output hub | `src/terminal/` | `portable-pty`, 512 KiB ring buffer, Ghostty config auto-import |
| Native terminal & WGPU renderer | `src/native_terminal/` | `libghostty-vt` FFI, WGPU child surface, input/mouse translation |
| Git worktree & workspace engine | `src/worktree/` | `git worktree` operations, `orca/<ws-id>/<slug>` naming, dirty check |
| Native audio & notifications | `src/notification/` | Rodio audio player, UNUserNotificationCenter, dock badge sync |
| Integration test contracts | `tests/` | Deterministic lifecycle, persistence, and hardening tests |

## CONVENTIONS
- **Dual Launch Modes**: `ferryx --daemon` runs headless Tokio multi-thread runtime emitting `FERRYX_DAEMON_READY`; default starts Tauri GUI.
- **Wire Case Serialization**: All IPC DTOs and Daemon JSON protocols use `#[serde(rename_all = "camelCase")]` (kebab-case for notification reasons/probes).
- **Offload Blocking Tasks**: Wrap synchronous disk I/O, Git invocations, and native dialogs in `crate::ipc::run_blocking` (`tokio::task::spawn_blocking`).
- **Binary Channel Framing**: Terminal output channels use 20-byte fixed-header binary frames (`TERMINAL_OUTPUT_FRAME_FIXED_BYTES = 20`) with monotonic sequence numbers.

## ANTI-PATTERNS
- **NEVER Allow GUI to Own PTYs**: All PTY sessions belong strictly to the daemon; GUI components only hold streaming client attachments.
- **NEVER Block Tokio Reactor**: Synchronous disk I/O or external subprocess lookups must not execute on async reactor worker threads.
- **NEVER Expose Local Absolute Paths Over Remote**: Remote endpoints accept only workspace IDs and worktree slugs.
- **NEVER Delete Dirty Worktrees in Normal Flow**: Worktrees with uncommitted files require explicit destructive deletion APIs.
- **NEVER Forcibly Kill or Restart Daemon on App Replacement**: When deploying a release build, replacing the app bundle, or launching the GUI, never kill or restart `ferryx --daemon` unless strictly necessary (e.g. incompatible daemon protocol mismatch). The daemon owns all PTY master fds; terminating it immediately kills all active terminal sessions and agent workflows.

## COMMANDS
```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo test --manifest-path src-tauri/Cargo.toml --test daemon_persistence_contract -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --test ipc_hardening_contract
cargo tauri dev
```
