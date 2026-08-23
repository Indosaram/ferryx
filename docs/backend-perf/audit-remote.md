# Audit: remote
Repo: /Users/indo/code/project/orca-lite
Scanned: src-tauri/src/remote/server.rs, src-tauri/src/remote/auth.rs, src-tauri/src/remote/state.rs, src-tauri/src/remote/protocol.rs, src-tauri/src/remote/tailscale.rs, src-tauri/src/remote/mod.rs, src-tauri/src/ipc/remote.rs, src-tauri/src/remote/tests.rs, src-tauri/Cargo.toml
Date: 2026-08-22

## Findings
### F-remote-01
- Severity: High
- File: src-tauri/src/remote/auth.rs:146
- Mechanism: On every authenticated HTTP request and WebSocket connection handshake, `validate_token` updates `device.last_seen_at` and unconditionally invokes `self.persist_best_effort()`. In `persist_best_effort()`, the entire `devices` and `tokens` hash maps are cloned under read locks, serialized to pretty-printed JSON, and synchronously written to disk via `write_private_json` (`std::fs::create_dir_all`, `std::fs::write` to `.tmp`, `std::fs::set_permissions`, and `std::fs::rename`). This forces synchronous disk I/O and JSON serialization directly onto the Tokio worker thread for every single client interaction (session listing, workspace polling, terminal upgrades), causing severe thread pool starvation and latency spikes under concurrent mobile requests.
- Hot path: yes
- Suggested fix: Decouple token validation from immediate synchronous persistence. Either debounce/throttle `last_seen_at` updates (e.g. only update/persist if more than 60s elapsed or on pairing/revocation events) or offload disk writes to a background task (`tokio::task::spawn_blocking` or a dirty-flag background flush) rather than blocking the Axum request handler.
- Write scope: src-tauri/src/remote/auth.rs
- RED proof:
```rust
    pub fn validate_token(&self, token: &str) -> Result<DeviceInfo, AuthError> {
        let device_id = {
            let tokens = self.tokens.read();
            tokens.get(token).cloned()
        }
        .ok_or(AuthError::Unauthorized)?;

        let result = {
            let mut devices = self.devices.write();
            let device = devices.get_mut(&device_id).ok_or(AuthError::Unauthorized)?;
            if device.revoked {
                return Err(AuthError::RevokedDevice);
            }
            device.last_seen_at = unix_now();
            device.clone()
        };
        self.persist_best_effort();
        Ok(result)
    }
```
Why slow: Synchronous `persist_best_effort` executes blocking filesystem syscalls (`std::fs::write`, `set_permissions`, `rename`) and JSON serialization synchronously on the Tokio runtime thread on every incoming request.

### F-remote-02
- Severity: High
- File: src-tauri/src/remote/server.rs:125
- Mechanism: On every call to `GET /api/v1/sessions` and `GET /api/v1/workspace/state`, `WorkspaceSnapshotCache::build(&state.workspace_registry)` is constructed synchronously inside the async request handler. For every registered workspace, `mgr.list_worktrees()` invokes a synchronous `git worktree list --porcelain` external subprocess (`std::process::Command::output`). Additionally, for every session in the response, `derive_session_metadata` calls `canonicalize_or_raw(path)` which executes recursive `std::fs::canonicalize` and `parent()` stat syscalls on the filesystem. When mobile clients poll workspace state or active sessions, this triggers repeated synchronous Git process spawning and recursive filesystem lookups on Tokio worker threads.
- Hot path: yes
- Suggested fix: Cache workspace metadata/worktree snapshots with a short TTL or invalidate on workspace change events rather than spawning `git worktree list` subprocesses on every HTTP request. Execute necessary Git queries or path canonicalizations within `tokio::task::spawn_blocking` or cache resolved canonical paths per session.
- Write scope: src-tauri/src/remote/server.rs
- RED proof:
```rust
impl WorkspaceSnapshotCache {
    pub(crate) fn build(registry: &crate::worktree::WorkspaceRegistry) -> Self {
        let mut entries = registry.list();
        // Deterministic order: `WorkspaceRegistry::list` iterates a `HashMap`, whose
        // order is unspecified and can vary between calls.
        entries.sort_by(|(a, _), (b, _)| a.cmp(b));
        let workspaces = entries
            .into_iter()
            .map(|(workspace_id, mgr)| WorkspaceSnapshot {
                workspace_id,
                root: canonicalize_or_raw(mgr.repo_root()),
                repo_root_display: mgr.repo_root().to_string_lossy().into_owned(),
                worktrees: mgr.list_worktrees().unwrap_or_default(),
            })
            .collect();
        Self { workspaces }
    }
```
Why slow: `mgr.list_worktrees()` spawns external `git` subprocesses synchronously on the async Tokio thread for every registered repository on every session list or workspace state HTTP query.

### F-remote-03
- Severity: Medium
- File: src-tauri/src/ipc/remote.rs:52
- Mechanism: `cmd_remote_status`, `cmd_remote_enable`, `cmd_remote_disable`, and `cmd_tailscale_status` are Tauri IPC commands called by the desktop UI. These handlers invoke `check_tailscale_status(&SystemCommandRunner)`, which runs two external child processes synchronously via `std::process::Command::output()` (`tailscale status --json` and `tailscale serve status --json`). Because they execute directly inside async Tauri command handlers without `tokio::task::spawn_blocking`, any Tailscale IPC delay, network discovery stall, or daemon unresponsiveness blocks the async runtime worker thread and induces desktop UI stutters. Furthermore, `get_local_ip()` performs synchronous UDP socket binding and routing table resolution on every status check.
- Hot path: yes
- Suggested fix: Wrap `check_tailscale_status` and `get_local_ip` execution inside `tokio::task::spawn_blocking` or cache Tailscale status with a debounce/polling interval so rapid status requests do not synchronously spawn multiple CLI subprocesses.
- Write scope: src-tauri/src/ipc/remote.rs, src-tauri/src/remote/tailscale.rs
- RED proof:
```rust
#[tauri::command]
pub async fn cmd_remote_status(
    manager: State<'_, Arc<RemoteGatewayManager>>,
) -> Result<RemoteGatewayStatusResponse, IpcError> {
    let state = manager.state();
    let config = state.config.read().clone();
    let is_running = *state.is_running.read();
    let bound_address = state.bound_address.read().clone();
    let tailscale = check_tailscale_status(&SystemCommandRunner);

    let local_ip = get_local_ip();
```
Why slow: Invoking blocking `std::process::Command` child processes directly in an async Tauri command handler blocks the async runtime thread during CLI execution.

### F-remote-04
- Severity: Medium
- File: src-tauri/src/remote/server.rs:354
- Mechanism: On every static asset request (HTML, JS bundles, CSS files, fonts, and SPA fallback routes), `serve_static_or_index` calls `resolve_dist_dir()`. `resolve_dist_dir()` iterates through up to 15 path candidates, performing multiple synchronous `std::fs::metadata` / `exists()` syscalls on candidate directories and files every time. After resolving the directory, it executes synchronous `file_path.exists()` and `file_path.is_file()` checks before invoking `tokio::fs::read`. Additionally, no caching headers (`Cache-Control`, `ETag`, `Last-Modified`) are attached to responses, forcing browsers to re-request all assets on every navigation and reload.
- Hot path: yes
- Suggested fix: Cache the resolved static asset directory once (e.g. using `std::sync::OnceLock<PathBuf>`), use async file checks, and include appropriate `Cache-Control` / `ETag` headers for immutable static assets (`.js`, `.css`, etc.) to eliminate redundant disk reads.
- Write scope: src-tauri/src/remote/server.rs
- RED proof:
```rust
fn resolve_dist_dir() -> PathBuf {
    let mut candidates = vec![
        PathBuf::from("ui/dist"),
        PathBuf::from("../ui/dist"),
        PathBuf::from("../../ui/dist"),
    ];
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("ui/dist"));
        candidates.push(cwd.join("dist"));
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe;
        for _ in 0..6 {
            if let Some(parent) = cur.parent() {
                candidates.push(parent.join("ui/dist"));
                candidates.push(parent.join("Resources/ui/dist"));
                candidates.push(parent.join("../Resources/ui/dist"));
                candidates.push(parent.join("../Resources"));
                cur = parent.to_path_buf();
            }
        }
    }
    for c in &candidates {
        if c.exists() && c.is_dir() && c.join("index.html").exists() {
            return c.clone();
        }
    }
    PathBuf::from("ui/dist")
}
```
Why slow: Performs dozens of synchronous filesystem existence checks on candidate paths on every static asset HTTP request instead of caching the resolved distribution directory and adding HTTP cache headers.

### F-remote-05
- Severity: Medium
- File: src-tauri/src/remote/server.rs:281
- Mechanism: In `handle_events_socket`, the message loop receives events via `while let Ok(msg) = rx.recv().await`. When the server experiences high event throughput or a client experiences transient network lag, the broadcast receiver returns `Err(broadcast::error::RecvError::Lagged(n))`. Using `while let Ok(...)` causes the loop to exit on `Lagged`, terminating the WebSocket connection prematurely and forcing the client into a reconnect/re-sync loop instead of skipping lagged messages and continuing to stream live events.
- Hot path: yes
- Suggested fix: Handle `broadcast::error::RecvError::Lagged(_)` explicitly by logging/continuing the loop, matching the resilient behavior in `handle_terminal_socket`.
- Write scope: src-tauri/src/remote/server.rs
- RED proof:
```rust
async fn handle_events_socket(mut socket: WebSocket, mut rx: broadcast::Receiver<String>) {
    while let Ok(msg) = rx.recv().await {
        if socket.send(Message::Text(msg.into())).await.is_err() {
            break;
        }
    }
}
```
Why slow: Prematurely terminates the WebSocket connection on temporary broadcast channel lag, causing connection churn, re-handshake overhead, and repeated full-state fetch queries from clients.

## Non-findings / accepted
1. **Bounded WebSocket broadcast channels**: `RemoteGatewayState::new_with_paths` allocates broadcast channels with bounded capacity (`broadcast::channel(1024)`), preventing unbounded memory growth during slow subscriber connections.
2. **Terminal WebSocket concurrency architecture**: `handle_terminal_socket` splits reader and writer tasks using `tokio::spawn` and manages task lifetime with `tokio::select!`. It handles `RecvError::Lagged(_)` gracefully and sends binary frames without intermediate JSON string encoding.
3. **No RwLock held across `.await` points**: All `parking_lot::RwLock` guards across `src-tauri/src/remote/server.rs`, `auth.rs`, `state.rs`, and `ipc/remote.rs` are scoped to synchronous blocks and dropped before invoking any asynchronous `.await` operations.
4. **Fast pairing code generation**: Pairing code generation and validation use lightweight in-memory `HashMap` operations with a 60-second window and single-use removal under short write locks.

## Scan coverage
- `src-tauri/src/remote/server.rs`: axum HTTP/WebSocket routes, request extractors, static file serving, terminal/events socket handlers, workspace snapshot cache.
- `src-tauri/src/remote/auth.rs`: device authentication, pairing exchange, token validation, auth state serialization and persistence.
- `src-tauri/src/remote/state.rs`: gateway state, config persistence, broadcast channel initialization.
- `src-tauri/src/remote/protocol.rs`: client/server message schemas and JSON serialization definitions.
- `src-tauri/src/remote/tailscale.rs`: tailscale CLI runner and JSON parsing for node/serve status.
- `src-tauri/src/remote/mod.rs`: module re-exports.
- `src-tauri/src/ipc/remote.rs`: Tauri IPC commands for remote gateway lifecycle, pairing, devices, and tailscale status.
- `src-tauri/src/remote/tests.rs`: integration and unit tests for auth manager, server lifecycle, and metadata derivation.
- `src-tauri/Cargo.toml`: dependency tree and feature flags verification.
