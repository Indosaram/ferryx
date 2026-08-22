# rorca Always-Persistent Daemon & Crash/Reboot Session Restore

## 1. Context & Motivation
In `orca-lite(rorca)`, users require uninterrupted continuity across two distinct lifecycle events:
1. **Desktop GUI Window Closing (`Cmd+Q`)**: A background daemon (`orca-lite --daemon`) must maintain PTY terminal sessions, the Axum Remote Gateway (port 43821), and mobile WebSocket connections alive 24/7 without killing active shell tasks.
2. **Cold Boot & OS Reboot**: If the entire computer restarts or crashes (destroying background processes), the complete workspace context (projects, active worktrees, tab order/pins, `paneTree` split ratios, CWDs, and recent scrollback buffers) must be restored atomically from disk (`session_state.json`) with zero duplicate spawn collisions against writer leases.

---

## 2. Target Architecture & IPC Boundary

```text
┌────────────────────────────────────────────────────────────────────────┐
│                         Desktop Tauri GUI App                          │
│   (Window / Webview, React Workspace, TabBar, TerminalPane)            │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
               Unix Domain Socket (`/tmp/rorca-$UID/daemon.sock`)
               Framing: Length-Prefixed / Newline JSON + Binary Stream
                                    │
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│                     rorca Background Daemon Engine                     │
│               (Managed via Launchd Agent or Auto-Spawn)                │
│                                                                        │
│  - PtyManager & TerminalService (24/7 Persistent PTY Processes)        │
│  - TerminalOutputHub (512 KiB Bounded Ring Buffer & Broadcast Stream)  │
│  - Axum Remote Gateway (Port 43821 Mobile Web & QR Pairing)            │
│  - Authoritative Session File Persister (Atomic Temp + Fsync)          │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
                            Atomic fsync Write
                                    ▼
                 ~/.local/share/rorca/session_state.json
```

---

## 3. Key Design Decisions & Invariants

### 3.1 Socket Path & Stale Socket Mutual Exclusion
- **Socket Location**: `/tmp/rorca-$UID/daemon.sock` (ensuring 0600 mode and avoiding the 104-byte `sun_path` limit on macOS).
- **Stale Socket Handling**: On daemon boot or GUI connect probe:
  - Attempt `UnixStream::connect`. If successful (`Ok`), daemon is alive.
  - If `ECONNREFUSED` / stale file, acquire a lockfile `/tmp/rorca-$UID/daemon.lock` via `flock`, unlink stale socket, and `bind()`.
- **Auto-Spawn Mutual Exclusion**: Multiple concurrent GUI launches or Launchd starts compete for the file lock; the winner binds and listens, while losers wait up to 2 seconds with exponential backoff and connect.

### 3.2 IPC Protocol & Streaming Pump
- **Framing**: Newline-delimited JSON for control RPCs (`Spawn`, `Write`, `Resize`, `Signal`, `Close`, `ListSessions`, `Attach`, `GetState`, `SaveState`), and multiplexed raw byte streams for terminal I/O.
- **Output Pumping**: The GUI connects to `daemon.sock` and subscribes to active terminal output streams. The GUI-side pump re-emits `terminal_output` Tauri events, keeping `ui/src/lib/terminalTransport/tauriTransport.ts` 100% backward compatible with zero breaking changes.
- **Lag Policy**: If a subscriber encounters `RecvError::Lagged`, the daemon sends a fresh snapshot of the `BoundedBuffer` history and resyncs the stream.

### 3.3 Session File Persistence & Authoritative Ownership
- **Single Authoritative Writer**: The background daemon is the sole writer to `session_state.json`. When GUI state updates (debounced 500ms), it sends a `SaveState` RPC to the daemon.
- **Durable Atomic Fsync**: Write to `session_state.json.tmp` -> `File::sync_all()` -> atomic `fs::rename` -> parent directory fsync.
- **Schema Versioning**: `version: 1` checked on load. If invalid or corrupt, quarantined to `session_state.json.corrupted` without crashing.

### 3.4 Scrollback Replay vs Live Output Precedence
- **Precedence Rule**:
  - **Warm Reconnect (Daemon Alive)**: The daemon's live `output_hub.subscribe()` ring buffer snapshot is replayed. Persisted scrollback from disk is ignored to prevent duplication.
  - **Cold Boot (Daemon Restarted)**: Persisted scrollback from `PersistedTerminalSession.recentScrollback` is injected into xterm before spawning the new PTY shell.
- **Buffer-Before-Write**: Live output chunks arriving during initial mount are queued and flushed immediately after history injection.

### 3.5 Writer Lease & Re-Attach Before Spawn
- On startup/restore, `App.tsx` first queries `cmd_terminal_list()` from the daemon:
  - If a session already exists for the worktree CWD, **re-attach** to the existing live session (releasing/preserving the existing lease).
  - Only if no session exists for that worktree, call `cmd_terminal_spawn` to create a fresh PTY shell.
  - This prevents `WriterAlreadyActive` errors during warm restarts.

---

## 4. Todos

- [ ] 1. Implement Daemon Engine, UDS Server, Version Handshake, & Gateway Hosting (`src-tauri/src/daemon/server.rs`)
  - Description: Create standalone daemon entry point (`orca-lite --daemon`), UDS listener at `/tmp/rorca-$UID/daemon.sock`, flock-based auto-spawn lock, protocol version handshake, and host both PtyManager and the Axum Remote Gateway inside the daemon.
  - Description: Create standalone daemon entry point (`orca-lite --daemon`), UDS listener at `/tmp/rorca-$UID/daemon.sock`, flock-based auto-spawn lock, and RPC message dispatcher.
  - Recommended task executor category: unspecified-high
  - Acceptance criteria: `orca-lite --daemon` binds socket, recovers stale files on `ECONNREFUSED`, handles ping RPC, and runs headless without initializing Tauri GUI/audio.
  - QA scenario: Unit and integration tests in `src-tauri/tests/daemon_uds_contract.rs` validating bind, concurrent spawn locking, and client connect.

- [ ] 2. Implement GUI Daemon Client & Output Stream Pump (`src-tauri/src/daemon/client.rs` & `src-tauri/src/ipc/terminal.rs`)
  - Description: Replace in-process `PtyManager` calls in `TerminalService` with a thin UDS client that auto-spawns the daemon if absent and pumps live byte frames to Tauri `app.emit(TERMINAL_OUTPUT_EVENT)`.
  - Recommended task executor category: unspecified-high
  - Acceptance criteria: Desktop Tauri commands (`cmd_terminal_spawn`, `cmd_terminal_write`, etc.) proxy transparently through `daemon.sock`.
  - QA scenario: Closing GUI window does not terminate daemon PID; reopening GUI immediately re-attaches to live shell session.

- [ ] 3. Implement Durable Fsync & Single-Writer Session Persister (`src-tauri/src/session/mod.rs`)
  - Description: Implement `File::sync_all()` on temp writes, parent directory sync, schema validation, and daemon-owned state persistence RPC.
  - Recommended task executor category: unspecified-low
  - Acceptance criteria: Simulated hard kill / zero-length write test recovers safely without corrupting previous valid session.
  - QA scenario: `cargo test --manifest-path src-tauri/Cargo.toml session` passes.

- [ ] 4. Implement Re-Attach-Before-Spawn and Scrollback Replay in Frontend (`ui/src/App.tsx` & `ui/src/lib/sessionPersistence.ts`)
  - Description: Update `App.tsx` restore loop to query active daemon sessions first before spawning to respect writer leases; inject scrollback with buffer-before-flush ordering.
  - Recommended task executor category: unspecified-low
  - Acceptance criteria: Vitest tests pass; warm restart preserves PTY session ID; cold restart restores scrollback and layout.
  - QA scenario: `bun run --cwd ui test` passes with 0 failures.

- [ ] 5. Implement macOS Launchd User Agent Installer (`src-tauri/src/daemon/launchd.rs`)
  - Description: Provide `orca-lite --install-daemon` and `--uninstall-daemon` managing `~/Library/LaunchAgents/com.rorca.daemon.plist` with `KeepAlive: true` and `launchctl bootout/bootstrap`.
  - Recommended task executor category: quick
  - Acceptance criteria: Launchd plist correctly structured, installs without root, and uninstalls cleanly.
  - QA scenario: Automated test verifying generated plist XML and command flags.

---

## 5. Final verification wave

- [ ] F1. Run full Cargo unit and integration test suites
  - Tool & command: `cargo test --manifest-path src-tauri/Cargo.toml`
  - Acceptance criteria: All tests pass with exit code 0.

- [ ] F2. Run Cargo Clippy linter
  - Tool & command: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
  - Acceptance criteria: Zero warnings with `-D warnings`.

- [ ] F3. Run Frontend Vitest suite and production build
  - Tool & command: `bun run --cwd ui test && bun run --cwd ui build`
  - Acceptance criteria: All test suites pass, TypeScript compiles cleanly, Vite build exits 0.

- [ ] F4. Integration test: Daemon persistence across GUI lifecycle
  - Tool & command: `cargo test --test daemon_persistence_contract --manifest-path src-tauri/Cargo.toml`
  - Acceptance criteria: PTY counter continues running across simulated GUI disconnect and reconnect.

---

## 6. TL;DR (For humans)
This plan establishes true 1:1 parity with original Orca's daemon architecture:
1. **24/7 Daemon Separation**: PTY processes and the Axum Remote Gateway live in a headless background daemon (`/tmp/rorca-$UID/daemon.sock`) that survives GUI `Cmd+Q`.
2. **Crash/Reboot Recovery**: Fsync-backed atomic session persistence restores tabs, split ratios, CWDs, and scrollback on cold reboots with writer-lease protection.
