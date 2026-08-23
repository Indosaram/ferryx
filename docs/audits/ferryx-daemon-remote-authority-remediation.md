# Ferryx Daemon Remote Authority Remediation Audit

Date: 2026-08-23

## Resolved review findings

### 1. GUI project registration now reaches the daemon before terminal spawn

`cmd_project_register` now registers the canonical workspace in both the GUI registry and the daemon via `DaemonClient::register_workspace`. The daemon-backed spawn path therefore resolves the same workspace identity as the GUI.

- RED: `tauri_mock_terminal_events_use_registered_workspace` failed with `Workspace 'workspace-test' is not registered`.
- GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --lib tauri_mock_terminal_events_use_registered_workspace -- --nocapture` passed.

### 2. The daemon is the one Remote authority

Ferryx Desktop no longer creates a second GUI-local `RemoteGatewayState`, listener, or `AuthManager`.

- `create_app` manages a `RemoteGatewayManager::from_daemon(Arc<DaemonClient>)` forwarding handle.
- GUI `cmd_remote_*` commands forward status, enable/disable, pairing, device list/revoke, and active selection through daemon protocol calls.
- The daemon owns the persistent Remote configuration, listener handle, terminal service, active selection, pairing tokens, and revocation state.

Direct coverage: `test_gui_remote_forwarding_and_no_gui_gateway_ownership` and `test_daemon_pairing_and_revocation_authority`.

### 3. Pairing, tokens, and revoke state share one authority

Pairing code generation, exchange, device listing, and revoke now operate against the daemon-owned `AuthManager`. A revoked token is rejected by the same daemon HTTP/WebSocket server that issued it.

Direct coverage: `test_daemon_pairing_and_revocation_authority`, plus existing persistent device/revocation tests.

### 4. A terminal WebSocket cannot outlive desktop focus

`RemoteGatewayState` publishes active-session changes through a watch channel. `handle_terminal_socket` observes that channel and ends a terminal connection if its session stops matching the active desktop terminal.

- RED: a socket connected to session A continued receiving output after focus changed to B.
- GREEN: `test_connected_terminal_websocket_closed_when_active_selection_changes` passes and requires EOF or a WebSocket Close frame after the change.

### 5. Failed listener binds do not persist auto-start intent

Daemon Remote configuration is persisted only after a listener successfully binds. If enable fails, the in-memory mode and durable config roll back to `Off`; explicit Disable stops the listener and persists `Off`.

- RED: `test_occupied_port_enable_does_not_persist_enabled_intent` observed `LocalNetwork` after an occupied-port bind failure.
- GREEN: the same test passes; `test_successful_enable_restores_and_disable_remains_off` proves successful restore and persistent disable behavior.

### 6. Terminal IPC hardening is daemon-backed again

`ipc_hardening_contract` now initializes a test daemon/client, registers workspace identity on both sides, calls daemon-backed terminal IPC, and reads PTY state from the daemon. Existing hardening assertions remain in place.

- RED: the contract failed to compile because it supplied `TerminalService` after commands moved to `DaemonClient`.
- GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --test ipc_hardening_contract` passes all 7 tests.

## Daemon-owned Remote transport proof

The durable regression `test_daemon_owned_remote_chain_end_to_end` proves the complete chain using an isolated daemon:

1. Register a temporary workspace.
2. Spawn a real daemon PTY and publish it as the active desktop terminal.
3. Bind daemon Remote on an ephemeral local port.
4. Create and exchange a Control pairing code through daemon HTTP.
5. Verify authenticated health, session list, and workspace state expose only the focused PTY.
6. Verify `/api/v1/terminal/{sessionId}` completes an authenticated `101 Switching Protocols` attach.
7. Disable the daemon listener, close the PTY, and tear down temporary daemon state.

It passes with:

```text
cargo test --manifest-path src-tauri/Cargo.toml --lib test_daemon_owned_remote_chain_end_to_end
test result: ok. 1 passed; 0 failed
```

An additional ephemeral live daemon QA driver bound port `43892`, registered a temporary workspace, spawned and activated a real daemon PTY, then returned this literal probe response:

```text
curl -i http://127.0.0.1:43892/api/v1/health

HTTP/1.1 200 OK
content-type: application/json

{"status":"ok","version":"0.1.0"}
```

Cleanup receipt:

```text
driver received Ctrl-C -> STOPPED
lsof confirmed port 43892 closed
temporary QA example and health transcript removed
```

## Final verification

| Command | Result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib` | PASS — 192 tests |
| `cargo test --manifest-path src-tauri/Cargo.toml --test ipc_hardening_contract` | PASS — 7 tests |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | PASS |
| `cargo check --manifest-path src-tauri/Cargo.toml` | PASS (existing worktree dead-code warnings only) |
| `cd ui && npm test` | PASS — 76 files, 553 tests |
| `cd ui && npm run build` | PASS |
| `cd ui && npx tsc --noEmit` | PASS |
| Changed Rust/TypeScript LSP diagnostics | PASS — zero errors |

The UI suite initially identified one stale test-only fixture: the transport implementation intentionally normalizes omitted optional `daemonEpoch` to `null`, while its expected DTO omitted the field. `ui/src/lib/terminalTransport/terminalTransport.test.ts` now asserts `daemonEpoch: null`; the focused transport suite passes 8 tests and the full UI suite passes.

## Manual desktop QA

Desktop UI automation was not used. Perform these checks in Ferryx Desktop:

1. Register/open a project and create a terminal. Confirm terminal creation succeeds after daemon startup.
2. Enable Remote, pair a mobile browser as Control, and open Remote. Confirm exactly the focused desktop terminal is displayed.
3. Focus a different terminal pane or browser tab. Confirm the old mobile terminal stream closes and a browser/no-terminal focus shows the Remote no-focused-terminal state.
4. In Mobile Remote, request a workspace/worktree context change. Confirm Ferryx Desktop receives the request and the mobile display updates only after desktop focus changes.
5. Revoke the mobile device in Desktop settings. Reload Mobile Remote and confirm its stored token is rejected.
6. Stop Ferryx, reopen it after a successful Remote enable, and confirm the paired mobile reconnects without a new pairing code. Then Disable Remote, restart Ferryx, and confirm the listener stays off until explicitly enabled again.

These steps validate the native GUI shell’s visible behavior; automated daemon and transport evidence above validates the underlying authority and security boundaries without operating the user’s desktop.
