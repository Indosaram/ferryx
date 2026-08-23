# Performance Fix: P-daemon-client-persistent-conn (F-daemon-session-01)

## Summary
- **Packet ID**: `P-daemon-client-persistent-conn`
- **Finding ID**: `F-daemon-session-01`
- **Severity**: High
- **Description**:
  Fixed `F-daemon-session-01`: Eliminated per-request Unix domain socket churn and repeated protocol handshakes in `DaemonClient`. Previously, `send_request` connected to the socket anew on every single request, performed a full handshake roundtrip followed by the request roundtrip, and dropped the connection. `DaemonClient` now holds a persistent `ActiveConnection` under an async `Mutex`, handshaking once upon initial connection and reusing the active stream across sequential requests (terminal keystrokes, resize commands, session state updates). If the connection breaks or the daemon restarts, `DaemonClient` transparently reconnects and re-establishes handshake negotiation.

## Files Changed
- `src-tauri/src/daemon/client.rs` (production fix & unit test suite)
- `docs/backend-perf/fix-P-daemon-client.md` (documentation & evidence)

## Production Change Details
- **File**: `src-tauri/src/daemon/client.rs`
  - **Lines 17–55**: Added `ActiveConnection` struct encapsulating `BufReader<OwnedReadHalf>` and `OwnedWriteHalf` with `request(&mut self, req: &DaemonRequest)` helper method for line-delimited JSON IPC and EOF detection.
  - **Lines 57–71**: Updated `DaemonClient` struct and `DaemonClient::new` constructor to initialize `connection: Arc<Mutex<Option<ActiveConnection>>>`.
  - **Lines 97–146**: Added `connect_and_handshake(&self)` helper performing socket connection/spawn, initial `DaemonRequest::Handshake`, and response verification returning an initialized `ActiveConnection`.
  - **Lines 148–165**: Rewrote `send_request(&self, req: DaemonRequest)` to reuse the locked `ActiveConnection` without repeating handshakes, and to lazily reconnect and retry transparently on socket error / EOF.

## Test Verification

### RED Phase
Added unit test `test_client_reuses_persistent_connection_without_rehandshaking` in `src-tauri/src/daemon/client.rs`. On the original implementation, the second request failed because the connection was closed after the first request and sent a new `Handshake` instead of `Ping` on a new connection.

**Command:**
```bash
cargo test --manifest-path /Users/indo/code/project/orca-lite/src-tauri/Cargo.toml --lib daemon::client
```

**Output (RED failure):**
```
running 2 tests
test daemon::client::tests::test_client_reconnects_transparently_when_socket_dropped ... ok
test daemon::client::tests::test_client_reuses_persistent_connection_without_rehandshaking ... FAILED

failures:

---- daemon::client::tests::test_client_reuses_persistent_connection_without_rehandshaking stdout ----

thread 'daemon::client::tests::test_client_reuses_persistent_connection_without_rehandshaking' (174351776) panicked at src/daemon/client.rs:265:13:
Expected second request on same persistent connection without disconnect
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread 'daemon::client::tests::test_client_reuses_persistent_connection_without_rehandshaking' (174351776) panicked at src/daemon/client.rs:280:67:
called `Result::unwrap()` on an `Err` value: IpcError { code: ParseError, message: "Handshake parse failed: EOF while parsing a value at line 1 column 0", details: None }


failures:
    daemon::client::tests::test_client_reuses_persistent_connection_without_rehandshaking

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 126 filtered out; finished in 0.01s
```

### GREEN Phase
Implemented persistent stream reuse and transparent reconnect in `DaemonClient`.

**Command:**
```bash
cargo test --manifest-path /Users/indo/code/project/orca-lite/src-tauri/Cargo.toml --lib daemon::client
```

**Output (GREEN pass):**
```
running 2 tests
test daemon::client::tests::test_client_reconnects_transparently_when_socket_dropped ... ok
test daemon::client::tests::test_client_reuses_persistent_connection_without_rehandshaking ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 126 filtered out; finished in 0.01s
```

**Integration Contract Verification:**
```bash
cargo test --manifest-path /Users/indo/code/project/orca-lite/src-tauri/Cargo.toml --test daemon_persistence_contract
```

**Output (GREEN pass):**
```
running 2 tests
test test_durable_fsync_session_persistence_lifecycle ... ok
test test_daemon_uds_handshake_and_ping ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s
```

## Invariants & Review Checklist
- Single responsibility: `DaemonClient` manages communication with the Unix domain socket server.
- Explicit async synchronization: Stream handle access is serialized via `tokio::sync::Mutex`, guaranteeing strict request-response ordering.
- Zero extra handshakes: Handshake occurs strictly upon connection creation / reconnection.
- Transparent recovery: Broken sockets or daemon restarts trigger seamless reconnect on the next request.
