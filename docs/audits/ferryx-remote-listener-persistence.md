# Ferryx Remote Listener Persistence Audit

Date: 2026-08-23

## User-visible lifecycle

Ferryx Remote now keeps the listener available across Ferryx Desktop restarts after the user has enabled it once.

- The saved Remote configuration persists the enabled network mode, port, and control setting.
- On the next Ferryx Desktop startup, `create_app(...).setup(...)` calls `RemoteGatewayManager::restore_persisted_listener()` asynchronously.
- A saved non-`Off` mode starts a new listener and stores its new, process-local server handle in the manager.
- Pairing records, device tokens, permissions, last-seen timestamps, and revocations remain persisted independently of the listener handle.
- **Disable Remote** is the opt-out: it stops the current listener, writes `Off` to persisted configuration, and causes later startup restoration to do nothing.

The resulting expected flow is:

```text
Enable Remote once -> pair mobile once -> restart Ferryx Desktop -> listener restores ->
the paired mobile reconnects with its saved token.
```

The mobile does not need to pair again unless its browser storage/token is removed, its device is revoked, or it is a different browser profile/device.

## Implementation evidence

| Requirement | Actual implementation | Direct test coverage |
| --- | --- | --- |
| Persist approved listener configuration | `src-tauri/src/remote/state.rs` serializes `mode`, `port`, `allowControl`, and `restartPolicy`; config load preserves saved non-`Off` mode. | `persisted_enabled_config_file_loads_enabled_mode_with_unbound_initial_state`; `enabled_gateway_persists_config_and_restores_on_reopen` |
| Restore after Desktop restart | `src-tauri/src/lib.rs` starts `restore_persisted_listener()` from Tauri `setup`; `src-tauri/src/ipc/remote.rs` creates and retains a new `RemoteServerHandle`. | `test_remote_listener_persistence_and_startup_restoration_lifecycle` |
| Explicit disable remains opt-out | `cmd_remote_disable` sets mode to `Off`, persists it, and stops the current handle; restore returns false for `Off`. | `test_remote_listener_persistence_and_startup_restoration_lifecycle`; `remote_status_after_reopen_persists_config_mode_until_started` |
| Pairing and permission persistence survives listener reopen | Existing auth persistence loads device/token records and preserves revocation status. | `paired_devices_and_revocations_survive_reopen`; `validate_token_throttles_disk_persistence` |

### RED -> GREEN proof

The new lifecycle test initially failed while restore remained session-only:

```text
test_remote_listener_persistence_and_startup_restoration_lifecycle ... FAILED
Restoration hook must report listener was restored
```

After the persistence and startup restoration implementation, the focused native library suite passed with 17 tests, including the listener lifecycle, paired-device persistence, revocation, and explicit-disable cases:

```text
cargo test --manifest-path src-tauri/Cargo.toml --lib remote
test result: ok. 17 passed; 0 failed
```

## Live restored-listener proof

This QA used an ephemeral configuration and auth directory, wrote an enabled `LocalNetwork` listener configuration, discarded the first state, built a fresh state/manager, and called the same `restore_persisted_listener()` method used by app startup.

Literal health probe:

```text
curl -i http://127.0.0.1:43891/api/v1/health
```

Captured response:

```http
HTTP/1.1 200 OK
content-type: application/json
content-length: 33

{"status":"ok","version":"0.1.0"}
```

Cleanup receipt:

```text
driver received Ctrl-C -> STOPPED
lsof confirmed port 43891 had no listener
temporary QA example, health transcript, auth/config files, and temporary directory removed
```

## Verification

| Command | Result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib remote` | PASS — 17 tests passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | PASS |
| `cargo check --manifest-path src-tauri/Cargo.toml` | PASS (pre-existing worktree dead-code warnings only) |
| LSP error diagnostics for `remote/state.rs`, `ipc/remote.rs`, `lib.rs`, `remote/tests.rs`, and `ipc/tests.rs` | PASS — zero errors |

### Separate pre-existing all-target blocker

`cargo test --manifest-path src-tauri/Cargo.toml remote` cannot currently compile the unrelated `src-tauri/tests/ipc_hardening_contract.rs` integration target. At lines 82 and 111, that shared test supplies `State<Arc<TerminalService>>` where a migrated terminal command expects `State<Arc<DaemonClient>>` (`E0308`). The Remote listener persistence change did not modify terminal IPC handlers or that test. The focused `--lib remote` suite above is green and covers this listener lifecycle change; the unrelated integration mismatch remains intentionally unmodified.

## Manual desktop check

No desktop UI automation was used. To confirm the released native Desktop behavior:

1. In Ferryx Desktop, enable Remote and pair a mobile browser once.
2. Close and reopen Ferryx Desktop. Do not generate a new pairing code.
3. Open the existing Mobile Remote URL/browser profile. Confirm it reconnects using the existing pairing token.
4. In Ferryx Desktop, choose **Disable Remote**, close and reopen Ferryx Desktop, then reload Mobile Remote. Confirm no listener is reachable until Remote is enabled again.
5. Re-enable Remote and confirm the same paired mobile reconnects without entering a pairing code.

This manual validation is the only remaining native desktop-surface confirmation; automated lifecycle and live HTTP evidence above prove the underlying listener behavior without controlling the user's desktop.
