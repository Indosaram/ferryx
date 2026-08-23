# Audit: daemon-session
Repo: /Users/indo/code/project/orca-lite
Scanned: src-tauri/src/daemon/mod.rs, src-tauri/src/daemon/server.rs, src-tauri/src/daemon/client.rs, src-tauri/src/daemon/protocol.rs, src-tauri/src/daemon/launchd.rs, src-tauri/src/session/mod.rs, src-tauri/src/ipc/session.rs, src-tauri/tests/session_persistence_integration.rs, src-tauri/tests/daemon_persistence_contract.rs, src-tauri/Cargo.toml
Date: 2026-08-22

## Findings
### F-daemon-session-01
- Severity: High
- File: src-tauri/src/daemon/client.rs:43
- Mechanism: `DaemonClient::send_request` connects to the Unix domain socket anew on every single request, performing a full handshake roundtrip followed by the request roundtrip before dropping the connection. On interactive terminal input (`write_terminal`, `resize_terminal`, `signal_terminal`), every keystroke and resize event incurs new socket connection setup, handshake serialization/deserialization, and connection teardown.
- Hot path: yes
- Suggested fix: Maintain a persistent, reusable `UnixStream` connection or connection pool inside `DaemonClient`, completing handshake once on initial connection and multiplexing/reusing requests across the open socket.
- Write scope: src-tauri/src/daemon/client.rs
- RED proof:
```rust
pub async fn send_request(&self, req: DaemonRequest) -> Result<DaemonResponse, IpcError> {
    let stream = self.connect_or_spawn().await?;
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Perform handshake first
    let handshake = DaemonRequest::Handshake {
        version: DAEMON_PROTOCOL_VERSION,
    };
    let mut json = serde_json::to_string(&handshake).unwrap();
    json.push('\n');
    write_half.write_all(json.as_bytes()).await.map_err(|e| {
        IpcError::new(IpcErrorCode::IoError, format!("Handshake write failed: {e}"))
    })?;

    let mut line = String::new();
    reader.read_line(&mut line).await.map_err(|e| {
        IpcError::new(IpcErrorCode::IoError, format!("Handshake read failed: {e}"))
    })?;
    let hs_resp: DaemonResponse = serde_json::from_str(line.trim()).map_err(|e| {
        IpcError::new(IpcErrorCode::ParseError, format!("Handshake parse failed: {e}"))
    })?;
```
Every PTY input keystroke calls `write_terminal`, which invokes `send_request`, incurring 2 full request-response cycles, JSON parsing of handshake messages, and socket churn per keystroke.

### F-daemon-session-02
- Severity: High
- File: src-tauri/src/daemon/server.rs:252
- Mechanism: In `DaemonServer::pump_stream`, each PTY output chunk from the broadcast channel is wrapped in `DaemonStreamMessage::Output { session_id, data: Vec<u8> }` and serialized with `serde_json::to_string`. By default, `serde_json` serializes `Vec<u8>` as an array of JSON numbers (e.g. `[104,101,108,108,111]`), multiplying output payload byte volume by ~3-4x, triggering continuous heap allocations, string conversions, and unbuffered individual socket writes on high-throughput terminal streams.
- Hot path: yes
- Suggested fix: Use binary framing, length-prefixed chunks, or base64-encoded strings with buffered writes for PTY streaming output over the Unix domain socket.
- Write scope: src-tauri/src/daemon/server.rs, src-tauri/src/daemon/protocol.rs
- RED proof:
```rust
async fn pump_stream(
    session_id: String,
    mut rx: broadcast::Receiver<Vec<u8>>,
    hub: Arc<TerminalOutputHub>,
    mut writer: tokio::net::unix::OwnedWriteHalf,
) {
    loop {
        match rx.recv().await {
            Ok(data) => {
                let msg = DaemonStreamMessage::Output {
                    session_id: session_id.clone(),
                    data,
                };
                let mut json = serde_json::to_string(&msg).unwrap();
                json.push('\n');
                if writer.write_all(json.as_bytes()).await.is_err() {
                    break;
                }
            }
```
During high-frequency terminal output (e.g., `cat` of large files or rapid build logs), `pump_stream` clones the session ID string, serializes raw byte arrays into bloated JSON integer lists, allocates fresh JSON strings, and issues unbuffered socket writes for every broadcast chunk.

### F-daemon-session-03
- Severity: Medium
- File: src-tauri/src/daemon/server.rs:188
- Mechanism: In `DaemonServer::handle_client`, `DaemonRequest::SaveSession`, `DaemonRequest::LoadSession`, and `DaemonRequest::ClearSession` synchronously invoke `save_session_to_path`, `load_session_from_path`, and `clear_session_from_path` directly on the Tokio worker thread without `tokio::task::spawn_blocking`. `save_session_to_path` performs blocking pretty-printed JSON serialization, file writes, `file.sync_all()`, atomic rename, and parent directory `sync_all()`. Synchronous fsync blocks the Tokio reactor thread and stalls other tasks sharing the executor.
- Hot path: no
- Suggested fix: Offload synchronous session persistence and fsync operations in `DaemonServer::handle_client` to `tokio::task::spawn_blocking` (matching the pattern used in `src-tauri/src/ipc/session.rs`).
- Write scope: src-tauri/src/daemon/server.rs
- RED proof:
```rust
Ok(DaemonRequest::SaveSession { session }) => {
    let path = get_default_session_path();
    match save_session_to_path(&path, &session) {
        Ok(()) => DaemonResponse::SaveSessionOk,
        Err(e) => DaemonResponse::Error {
            message: e.to_string(),
        },
    }
}
Ok(DaemonRequest::LoadSession) => {
    let path = get_default_session_path();
    match load_session_from_path(&path) {
        Ok(session) => DaemonResponse::LoadSessionOk { session },
        Err(e) => DaemonResponse::Error {
            message: e.to_string(),
        },
    }
}
```
`save_session_to_path` and `load_session_from_path` execute blocking disk I/O directly within Tokio async task handlers instead of delegating to a blocking worker thread pool.

### F-daemon-session-04
- Severity: Medium
- File: src-tauri/src/session/mod.rs:152
- Mechanism: `save_session_to_path` uses `serde_json::to_string_pretty(session)` to serialize the complete workspace session hierarchy (including all workspaces, worktrees, tab trees, browser metadata, and terminal sessions) and performs two synchronous fsync calls (`file.sync_all()` on the temp file and `dir_file.sync_all()` on the parent directory). Pretty-printing introduces unnecessary string formatting allocations and whitespace overhead across monolithic state snapshots.
- Hot path: no
- Suggested fix: Replace `serde_json::to_string_pretty` with compact `serde_json::to_vec` or `to_string`, and avoid redundant parent directory `sync_all` where atomic rename durability is already guaranteed by filesystem semantics.
- Write scope: src-tauri/src/session/mod.rs
- RED proof:
```rust
let serialized = serde_json::to_string_pretty(session).map_err(|e| {
    IpcError::new(
        IpcErrorCode::ParseError,
        format!("Failed to serialize session state: {}", e),
    )
})?;
```
Monolithic session persistence serializes the entire session state graph as formatted multi-line text with redundant whitespace, increasing memory allocations and write size on every persistence cycle.

## Non-findings / accepted
- `src-tauri/src/daemon/launchd.rs`: Synchronous execution of `launchctl` commands via `std::process::Command` is acceptable because launchd agent installation and uninstallation are one-time administration commands triggered explicitly outside the application hot path.
- `src-tauri/src/ipc/session.rs`: Tauri IPC commands `cmd_session_save`, `cmd_session_load`, and `cmd_session_clear` correctly offload disk operations to worker threads using `run_blocking`, preventing UI and IPC handler thread stalls.
- `src-tauri/src/daemon/server.rs`: `DaemonServer::run_server` uses non-blocking file locking (`libc::LOCK_NB`), non-blocking asynchronous socket binding with `tokio::net::UnixListener::accept().await`, and spawns an independent Tokio task per client stream, avoiding accept starvation.

## Scan coverage
- `src-tauri/src/daemon/mod.rs`: Re-exports daemon submodules; no runtime logic.
- `src-tauri/src/daemon/server.rs`: Examined socket listener lifecycle, client stream handler, PTY output stream pump, session persistence calls, and Axum remote server spawning.
- `src-tauri/src/daemon/client.rs`: Audited UDS connection handling, handshake negotiation, request-response execution, and terminal I/O IPC wrappers.
- `src-tauri/src/daemon/protocol.rs`: Audited protocol data types, request/response enums, stream messages, and JSON serialization representations.
- `src-tauri/src/daemon/launchd.rs`: Audited launchd agent plist generation and launchctl process invocations.
- `src-tauri/src/session/mod.rs`: Audited persisted session structs, JSON serialization routines, atomic file write and fsync lifecycle, corrupted session recovery, and unit tests.
- `src-tauri/src/ipc/session.rs`: Audited Tauri IPC commands for session save, load, and clear with thread offloading.
- `src-tauri/tests/session_persistence_integration.rs`: Reviewed test assertions for session serialization compatibility and roundtrip integrity.
- `src-tauri/tests/daemon_persistence_contract.rs`: Reviewed test assertions for daemon UDS handshake, ping, and fsync session persistence.
- `src-tauri/Cargo.toml`: Checked dependencies and feature flags affecting daemon and session persistence.
