# src-tauri/src/daemon

<!-- Score: 12 | Domain: Background PTY daemon, UDS protocol v2, launchd service -->

## OVERVIEW
Standalone background daemon managing persistent terminal PTY processes, session metadata, and Unix domain socket IPC.

## WHERE TO LOOK
| Task | Location | Notes |
|---|---|---|
| UDS client & reconnect logic | `client.rs` | Connection retry classification, streaming channels, request proxy |
| UDS server & concurrency | `server.rs` | Socket listener, `flock` lockfile, UID validation, session registry |
| Wire protocol serialization | `protocol.rs` | Wire schemas v2 (`DaemonRequest`/`Response`), Base64 stream encoding |
| macOS launchd integration | `launchd.rs` | `com.rorca.daemon` plist generation, install, uninstall |
| Module entry & exports | `mod.rs` | Public re-exports of client, server, protocol, and launchd |

## CONVENTIONS
- **Socket Security & Isolation**: Sockets reside in `/tmp/rorca-{uid}/daemon.sock` (mode 0700); verifies UID ownership, strictly forbids symlinks, and locks `daemon.lock` via `flock`.
- **Protocol Versioning**: Current protocol is `DAEMON_PROTOCOL_VERSION = 2`. Raw byte payloads are Base64 framed.
- **Retry-Safety Classification**: Idempotent reads/spawns/handshakes are retry-safe; mutating commands (`write`, `resize`, `signal`, `close`, `saveSession`) are NEVER retried if the connection drops post-send.

## ANTI-PATTERNS
- **DO NOT Auto-Respawn Missing Sessions**: On cold restore or epoch mismatch, missing backend sessions must be marked exited rather than spawning unexpected replacement shells.
- **DO NOT Treat Query Failures as Empty Lists**: Connection errors during session polling must bubble up; do not collapse failures to `[]` which destroys live session tracking.
- **NEVER Skip Socket Path & UID Validation**: Socket operations must reject paths outside `/tmp/rorca-{uid}/` or owned by different UIDs.
- **NEVER Forcibly Kill or Restart Daemon on App Replacement**: When deploying release bundles, replacing `/Applications/Ferryx.app`, or relaunching the GUI, never terminate `ferryx --daemon` unless strictly required (e.g. breaking daemon protocol mismatch). Daemon death drops PTY master file descriptors in the kernel and immediately terminates all running terminal processes and agent tasks.

## COMMANDS
```bash
cargo test --manifest-path src-tauri/Cargo.toml daemon::
cargo test --manifest-path src-tauri/Cargo.toml --test daemon_persistence_contract -- --test-threads=1
```
