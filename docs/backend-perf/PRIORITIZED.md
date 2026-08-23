# Prioritized backend perf fixes
Date: 2026-08-22

## Packets

### P-ipc-agent-detection
- Severity: High
- Findings: F-browser-ipc-01
- Write scope (exclusive): src-tauri/src/ipc/agents.rs
- Must-do:
  1. Remove external subprocess execution of `Command::new("which")` inside `check_binary_available`.
  2. Implement an in-process binary lookup scanning directories in `std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())` with `Path::is_file()` and executable permission checks.
  3. Ensure `cmd_agents_detect` performs candidate lookups asynchronously without blocking the async IPC worker thread.
- RED proof: Add unit test in `src-tauri/src/ipc/agents.rs` verifying `check_binary_available` resolves binaries against a custom `PATH` without spawning child processes.
- GREEN proof: `cargo test --lib ipc::`
- Invariant: Known installed CLI agent binaries (`claude`, `cursor`, `aider`, `copilot`, `goose`, `cline`, etc.) present on `PATH` must be accurately detected as available.

### P-browser-bounds-state
- Severity: High
- Findings: F-browser-ipc-02, F-browser-ipc-03
- Write scope (exclusive): src-tauri/src/ipc/browser.rs, src-tauri/src/browser/manager.rs
- Must-do:
  1. Add a direct `webview_label` return or lightweight lookup helper in `BrowserManager` for bounds, visibility, zoom, and focus operations to eliminate secondary `get_state` calls.
  2. Eliminate the double-lock pattern in `src-tauri/src/ipc/browser.rs` (`manager.set_bounds` write lock followed immediately by `manager.get_state` read lock and full 8-field struct clone).
  3. In `src-tauri/src/browser/manager.rs`, optimize `update_navigation_state`, `register_session`, and `get_state` to avoid cloning unchanged fields while holding locks.
- RED proof: Add unit tests in `src-tauri/src/browser/tests.rs` asserting `set_bounds` and `set_visible` return the target webview label without invoking full `BrowserState` heap extraction.
- GREEN proof: `cargo test --lib browser::tests`
- Invariant: Window resize, tab switching, and webview layout bounds updates must apply accurately without desynchronizing webview positions, visibility, or zoom factors.

### P-daemon-client-persistent-conn
- Severity: High
- Findings: F-daemon-session-01
- Write scope (exclusive): src-tauri/src/daemon/client.rs
- Must-do:
  1. Maintain a persistent `UnixStream` connection handle within `DaemonClient` protected by async synchronization.
  2. Perform protocol handshake negotiation once upon initial connection rather than reconnecting and repeating handshakes on every `send_request` / keystroke.
  3. Implement lazy transparent reconnection upon socket disconnection or I/O error to recover from daemon restarts.
- RED proof: Add integration test verifying `DaemonClient` executes sequential commands over a single persistent connection without repeating handshake exchanges on each invocation.
- GREEN proof: `cargo test --test daemon_persistence_contract`
- Invariant: Terminal input keystrokes, resizes, and session control requests must preserve byte-exact request-response ordering and transparently reconnect if the daemon restarts.

### P-daemon-stream-framing
- Severity: High
- Findings: F-daemon-session-02, F-daemon-session-03
- Write scope (exclusive): src-tauri/src/daemon/server.rs, src-tauri/src/daemon/protocol.rs
- Must-do:
  1. In `src-tauri/src/daemon/protocol.rs`, update PTY stream output serialization to avoid serializing `Vec<u8>` as JSON number arrays (use Base64 or byte-oriented chunk encoding).
  2. In `src-tauri/src/daemon/server.rs`, wrap socket writes in `BufWriter` inside `pump_stream` and eliminate redundant session ID string cloning per broadcast chunk.
  3. In `src-tauri/src/daemon/server.rs`, offload synchronous session persistence calls (`save_session_to_path`, `load_session_from_path`, `clear_session_from_path`) in `handle_client` to `tokio::task::spawn_blocking`.
- RED proof: Add test in `daemon_persistence_contract` asserting stream message framing does not serialize raw PTY bytes into multi-element JSON integer arrays.
- GREEN proof: `cargo test --test daemon_persistence_contract`
- Invariant: Terminal PTY byte streams over the daemon Unix domain socket must maintain bit-level fidelity without byte corruption or stream truncation.

### P-remote-auth-caching
- Severity: High
- Findings: F-remote-01
- Write scope (exclusive): src-tauri/src/remote/auth.rs
- Must-do:
  1. Decouple `validate_token` from unconditional synchronous disk persistence (`self.persist_best_effort()`).
  2. Throttle `last_seen_at` disk flushes (e.g. persist only on pairing, revocation, or after a 60-second dirty threshold) and offload JSON serialization and file writing to `tokio::task::spawn_blocking`.
- RED proof: Add unit test in `src-tauri/src/remote/tests.rs` / `persistence_tests` asserting rapid repeated token validation calls execute in memory without blocking file sync syscalls on each check.
- GREEN proof: `cargo test --lib remote::auth::persistence_tests`
- Invariant: Paired device authentication tokens, revocation statuses, and device identity mappings must persist durably across server restarts.

### P-remote-server-optimization
- Severity: High
- Findings: F-remote-02, F-remote-04, F-remote-05
- Write scope (exclusive): src-tauri/src/remote/server.rs
- Must-do:
  1. Cache `WorkspaceSnapshotCache` with an invalidation policy or short TTL, offloading any required Git subprocess calls or path canonicalizations to `spawn_blocking` rather than blocking `GET /api/v1/sessions` and `GET /api/v1/workspace/state`.
  2. Cache the static asset distribution directory in `resolve_dist_dir` using `std::sync::OnceLock<PathBuf>` to eliminate up to 15 path checks per request, and add standard HTTP caching headers to static bundle responses.
  3. In `handle_events_socket`, handle `tokio::sync::broadcast::error::RecvError::Lagged(_)` by logging and continuing the stream loop instead of closing the WebSocket connection.
- RED proof: Add test in `src-tauri/src/remote/tests.rs` asserting static asset resolution uses cached paths and event WebSocket stream handles broadcast channel lag without closing.
- GREEN proof: `cargo test --lib remote::tests`
- Invariant: Remote HTTP and WebSocket endpoints must return up-to-date session and workspace state and serve static web assets accurately.

### P-remote-tailscale-async
- Severity: Medium
- Findings: F-remote-03
- Write scope (exclusive): src-tauri/src/ipc/remote.rs, src-tauri/src/remote/tailscale.rs
- Must-do:
  1. In `src-tauri/src/ipc/remote.rs`, offload `check_tailscale_status` and `get_local_ip` calls to `tokio::task::spawn_blocking` across `cmd_remote_status`, `cmd_remote_enable`, `cmd_remote_disable`, and `cmd_tailscale_status`.
  2. In `src-tauri/src/remote/tailscale.rs`, ensure CLI output parsing cleanly handles command timeouts or non-zero exits without hanging.
- RED proof: Add async test verifying `cmd_remote_status` executes CLI subprocesses off the async Tokio reactor thread.
- GREEN proof: `cargo test --lib remote::tests::test_tailscale_status_parsing`
- Invariant: Tailscale connectivity status, node DNS, and local LAN IP detection must report accurate state to the UI without blocking async IPC dispatch.

### P-terminal-pty-pipeline
- Severity: High
- Findings: F-terminal-01, F-terminal-02, F-terminal-03
- Write scope (exclusive): src-tauri/src/terminal/pty.rs, src-tauri/src/terminal/session.rs, src-tauri/src/terminal/output_hub.rs, src-tauri/src/terminal/service.rs, src-tauri/src/ipc/terminal.rs
- Must-do:
  1. In `src-tauri/src/terminal/pty.rs`, eliminate the 20ms polling loop (`LIFECYCLE_POLL_INTERVAL`) in `start_lifecycle_watcher`; replace with event-driven child exit notification triggered on blocking reader completion or process wait.
  2. In `src-tauri/src/terminal/session.rs` and `src-tauri/src/terminal/output_hub.rs`, use shared reference-counted buffers (`bytes::Bytes` or `Arc<[u8]>`) or publish directly from the reader task to avoid double-cloning 4KB chunks and remove redundant intermediate channel hops.
  3. In `src-tauri/src/ipc/terminal.rs`, coalesce and micro-batch terminal output chunks (with an 8–16ms flush window or buffer size threshold) before Base64 encoding and emitting `terminal_output` Tauri events to prevent IPC flood and broadcast receiver lag.
- RED proof: Add test in `src-tauri/src/terminal/tests.rs` verifying child process exit is detected without periodic 20ms polling and high-throughput byte streaming delivers complete output without dropped chunks.
- GREEN proof: `cargo test --lib terminal::tests`
- Invariant: Terminal PTY input/output must remain interactive and low-latency (<16ms), process exit codes must be accurately captured, and terminal output history in `output_hub` must match exact terminal byte output.

### P-worktree-git-ops
- Severity: High
- Findings: F-worktree-01, F-worktree-02, F-worktree-03, F-worktree-04
- Write scope (exclusive): src-tauri/src/worktree/registry.rs, src-tauri/src/worktree/manager.rs, src-tauri/src/worktree/git.rs
- Must-do:
  1. In `src-tauri/src/worktree/registry.rs` and `src-tauri/src/worktree/manager.rs`, optimize `resolve_worktree` and `find_worktree_by_slug` to target the expected worktree path directly via deterministic slug path mapping (`worktree_path_for`) rather than enumerating and canonicalizing every worktree via `git worktree list --porcelain`.
  2. In `src-tauri/src/worktree/manager.rs`, eliminate the redundant `list_worktrees` subprocess call and quadratic canonicalization loop in `create_worktree`.
  3. In `src-tauri/src/worktree/manager.rs` and `src-tauri/src/worktree/git.rs`, replace `git branch --merged HEAD` in `branch_is_merged` with `git merge-base --is-ancestor <branch> HEAD`.
  4. In `src-tauri/src/worktree/manager.rs`, narrow `delete_lock` hold duration during worktree deletions to prevent blocking unrelated writer lease requests across the repository.
- RED proof: Add test in `src-tauri/src/worktree/tests.rs` asserting `resolve_worktree` resolves known worktrees directly without repository-wide worktree enumeration, and `branch_is_merged` correctly identifies merged branches via merge-base ancestor checks.
- GREEN proof: `cargo test --lib worktree::tests`
- Invariant: Worktree isolation, dirty status detection, writer lease safety contracts, and safe unmerged branch deletion guards must not be compromised.

## Deferred / accepted

- **F-browser-ipc-04 (Medium)** — `src-tauri/src/browser/cookies.rs`: Two-pass `serde_json::Value` parsing of cookie files. Deferred because cookie file import is a rare manual user action, not on the application hot path.
- **F-daemon-session-04 (Medium)** — `src-tauri/src/session/mod.rs`: `serde_json::to_string_pretty` and parent directory `sync_all` during session persistence. Deferred because Tauri IPC already wraps session persistence in `run_blocking` off the main thread, and session saving occurs on window unload or debounced state changes.
- **F-terminal-04 (Medium)** — `src-tauri/src/ipc/terminal.rs`: macOS `/usr/sbin/lsof` subprocess execution in `cmd_terminal_get_cwd`. Deferred because terminal CWD lookup is queried only on explicit prompt/tab events and is non-blocking to terminal streaming throughput; replacing with `libproc` requires FFI bindings.
- **F-browser-ipc-05 (Low)** — `src-tauri/src/ipc/project.rs`: `cmd_project_branches` sequential git commands and redundant in-memory sort. Deferred because project branch listing occurs infrequently upon repository selection and is already wrapped in `run_blocking`.
- **F-worktree-05 (Low)** — `src-tauri/src/worktree/git.rs`: Redundant `git rev-parse` commit verification in `git_worktree_add`. Deferred because `git worktree add` itself verifies the target reference atomically, and worktree creation is infrequent.
- **F-terminal-05 (Low)** — `src-tauri/src/terminal/session.rs`: Eight individual `Arc<Mutex<Option<T>>>` allocations on `PtySession`. Deferred because session construction happens once per terminal tab/split; memory footprint is negligible relative to PTY buffers.
- **F-notification-01 (Low)** — `src-tauri/src/terminal/preferences.rs`: `load_terminal_preferences` spawns `ghostty +show-config` CLI before checking disk candidate files. Deferred because preferences are loaded once on startup and already offloaded via `run_blocking`.
- **F-notification-02 (Low)** — `src-tauri/src/notification/permission.rs`, `src-tauri/src/notification/service.rs`: macOS `UNUserNotificationCenter` synchronous settings query without in-memory TTL caching. Deferred because notification dispatches are sparse user events, not high-throughput loops.

## Dropped

- **BrowserManager `parking_lot::RwLock` uncontended fast spinning**: Confirmed optimal (<100ns read lock overhead).
- **`run_blocking` helper pattern across IPC commands**: Confirmed proper offloading of heavy disk/git operations to Tokio blocking threadpool without starving async reactor workers.
- **Synchronous `launchctl` execution in `daemon/launchd.rs`**: Confirmed administrative one-off command outside runtime execution path.
- **Bounded PTY ring buffer capacity (512 KiB) in `output_hub.rs` and broadcast channel limits (1024) in `remote/state.rs`**: Confirmed bounded memory architecture prevents leaks.
- **Audio player volume clamping, Rodio offloading, and 400ms sound deduplication in `notification/audio.rs`**: Confirmed optimal implementation.
