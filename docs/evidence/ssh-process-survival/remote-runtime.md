# Remote session runtime - st_01a084c0

This task continued the existing st_01a0848d runtime and service integration in the shared worktree. It changed only remote.rs, remote_runtime_tests.rs, and this evidence document. Prior implementation evidence is retained below as historical evidence, not claimed as commands executed by this task.

## Delivered public contract

`TerminalService::remote()` exposes the single `RemoteRuntime` session map/controller. SSH child ownership remains exclusively in `SshBridgeClient` / `BridgeConnection`. No daemon routing, bridge, helper setup, UI, trust configuration, or local process lifecycle implementation was changed.

- `create(config, SpawnParams, client_request_id).await`: the only runtime path to `pty.spawn`; requires a nonempty immutable request ID and rejects conflicting IDs in params. Registers the project and preserves worktree and opaque agent identity. The caller must durably allocate the request ID before calling. An uncertain create result is returned as an error, never silently retried with a fresh ID.
- `restore(RemoteSessionDescriptor)`: inserts the stable local backendSessionId and starts same-TargetRef reattachment. Never calls spawn. Duplicate registered local IDs and duplicate TargetRefs under another local ID are rejected rather than resetting output history or installing a second controller.
- Descriptor serializes `backendSessionId`, `target`, configured SSH host, remote environment, helper executable/root, project ID/path, worktree, agent identity, immutable `clientRequestId`, decimal-string `remoteCursor`, cols/rows. Remote target backendSessionId is deliberately distinct from the local backendSessionId.
- `details(id)` / `subscribe(id)` expose watch snapshots: connected/reconnecting/disconnected/expired, generation, retry attempts, typed failure, remote PID, remote replay gap, current descriptor. `list()` returns registered remote IDs. TerminalService list/attach/attach_with_sequence/close_session include remote sessions. `get_session()` continues to return only local PtySession objects; daemon consumers must use remote details for remote sessions.
- `write(id, generation, bytes)` and `resize(id, generation, cols, rows)` return `Result<RemoteOperation, RemoteFailure>` synchronously. They reject outages/stale generations/busy control without queueing. The completion future must be awaited; it obtains the gate with try_lock and rechecks state/generation before dispatch. The gate serializes control RPCs against generation replacement. Input is single-attempt and never replayed.
- TerminalService `write_input_operation` and `resize_operation` route remote operations while preserving synchronous local behavior. Their admission errors use existing PtyError::Other to avoid an out-of-scope exhaustive IPC error-mapper change. For machine-readable admission errors use `service.remote().write/resize`; completion errors remain typed. Old synchronous write_input explicitly rejects remote IDs.
- `retry(id)` is deduplicated during connected/reconnecting states; otherwise only reattaches the existing target. Automatic retry is capped at five retries, with delays 250/500/1000/2000/4000 ms. Missing/expired/authentication/protocol failures terminate automation. Explicit retry resets the budget. The cap is per controller run, not reset after every successful reconnect.
- `close(id).await` aborts retry and issues `pty.stop` only for the stored TargetRef, connecting solely to stop if necessary. Failed stop retains the descriptor in disconnected state for actionable retry. Runtime Drop aborts reader/reconnect tasks and releases SSH transports, never calling pty.stop. RemotePid never enters a local process API.

## Output and persistence integration obligations

Remote cursors are record cursors, never output-hub byte offsets or local sequence numbers. Describe's current cursor does not advance the persisted cursor. Read ordering/identity is validated, already-seen records are discarded, and recovered bytes enter the existing hub in order. Cursor advances only with publication. Cols/rows changes leave existing output-hub resize markers.

Remote retention gaps are reported separately as `details.replayGap { requestedAfterCursor, availableFromCursor }`. The daemon producer must subscribe to remote details and emit that boundary alongside its remote state protocol. The existing worktree's `TerminalOutputHub::publish_gap` also emits a sequenced empty `OutputChunk` with a replay-gap boundary before recovered bytes, clears obsolete retained history, and reports the boundary to later attachments. Those sequence values are local hub sequence numbers, not remote cursors. This task did not change output_hub.rs.

The daemon producer is responsible for durably storing descriptor updates and routing the remote generation supplied by clients. There is no agent --resume behavior. Real daemon startup/shutdown, restored routing, and wire/UI delivery of typed states/gaps are intentionally outside this task.

## Current task verification

Two behavioral regressions were added before the production fix. A control-side failure previously left the controller blocked on its independent read; the controller now subscribes to state before waiting and cancels that read on loss of connectivity. Duplicate remote targets previously admitted another controller; registration now rejects that alias while holding the session-map lock.

RED, one execution:

```
cargo test --manifest-path src-tauri/Cargo.toml --lib terminal::remote_runtime_tests -- --nocapture
called `Result::unwrap_err()` on an `Ok` value: ()
state deadline: Elapsed(())
test result: FAILED. 7 passed; 2 failed; 0 ignored; 0 measured; 829 filtered out; finished in 3.01s
```

GREEN, one execution covering the ssh_process_survival, ssh_daemon_restart, and ssh_reconnect_safety runtime tests plus terminal regressions:

```
cargo test --manifest-path src-tauri/Cargo.toml --lib terminal::
test terminal::remote_runtime_tests::ssh_daemon_restart_descriptor_only_and_drop_does_not_stop ... ok
test terminal::remote_runtime_tests::ssh_daemon_restart_rejects_duplicate_target_controller ... ok
test terminal::remote_runtime_tests::ssh_reconnect_safety_authentication_on_redial_stops_retries ... ok
test terminal::remote_runtime_tests::ssh_process_survival_same_target_replay_and_close ... ok
test terminal::remote_runtime_tests::ssh_reconnect_safety_erased_ipc_errors_are_not_classified_by_prose ... ok
test terminal::remote_runtime_tests::ssh_reconnect_safety_failure_classification ... ok
test terminal::remote_runtime_tests::ssh_reconnect_safety_control_failure_interrupts_pending_read ... ok
test terminal::remote_runtime_tests::ssh_reconnect_safety_setup_classifies_structured_transport_and_authentication ... ok
test terminal::remote_runtime_tests::ssh_reconnect_safety_retry_cap_and_terminal_failure ... ok
test result: ok. 242 passed; 0 failed; 0 ignored; 0 measured; 596 filtered out; finished in 5.01s
```

```
cargo build --manifest-path src-tauri/Cargo.toml --lib
warning: `ferryx` (lib) generated 16 warnings (run `cargo fix --lib -p ferryx` to apply 2 suggestions)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 16.28s
```

LSP diagnostics requested for remote.rs, remote_runtime_tests.rs, service.rs, and mod.rs all returned `LSP daemon unreachable`. Compiler verification above succeeded, with the same 16 warnings present in RED. Logs: `/tmp/st_01a084c0-red.log`, `/tmp/st_01a084c0-green.log`, `/tmp/st_01a084c0-build.log`.

The new tests use exact watch-state subscriptions, channel-driven reads, and a semaphore-controlled retry clock with bounded state deadlines. They start no helper processes. The terminal suite exercises local PTY entry points. No production startup, daemon QA, live SSH host, or UI surface was executed; live end-to-end reconnection remains outside this runtime verification. No commits were created.

Assumptions: the caller durably persists request IDs and descriptors; the existing bridge owns SSH processes and enforces helper identity; daemon routing is owned by the next task. The named foundation commits are present; bridge.rs and helper_setup.rs were introduced in 02ecab9, not the earlier helper/trust commits.

## Historical verification from st_01a0848d

RED was recorded before remote.rs existed:

```
cargo test --manifest-path src-tauri/Cargo.toml --lib --no-default-features ssh_reconnect_safety
error[E0583]: file not found for module `remote`
```

That configuration also exposes unrelated current-worktree errors for missing native-terminal preedit command macros. It is not reported as green. A new PtyError variant initially caused an exhaustive match error in ipc/error.rs; the variant was removed rather than modifying the foreign file.

First native runtime run:

```
cargo test --manifest-path src-tauri/Cargo.toml --lib terminal::remote_runtime_tests
running 4 tests
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 822 filtered out; finished in 0.01s
```

Review then caught and fixed a prior transport failure masking an authentication error on the next dial. A deterministic regression was added. Final related suite (one execution, no retry-to-green):

```
cargo test --manifest-path src-tauri/Cargo.toml --lib terminal::
test terminal::remote_runtime_tests::ssh_process_survival_same_target_replay_and_close ... ok
test terminal::remote_runtime_tests::ssh_daemon_restart_descriptor_only_and_drop_does_not_stop ... ok
test terminal::remote_runtime_tests::ssh_reconnect_safety_failure_classification ... ok
test terminal::remote_runtime_tests::ssh_reconnect_safety_authentication_on_redial_stops_retries ... ok
test terminal::remote_runtime_tests::ssh_reconnect_safety_retry_cap_and_terminal_failure ... ok
test result: ok. 236 passed; 0 failed; 0 ignored; 0 measured; 591 filtered out; finished in 5.06s
```

```
cargo build --manifest-path src-tauri/Cargo.toml --lib
warning: `ferryx` (lib) generated 16 warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 22.51s
```

All five new tests drive the actual controller and output hub via an internal transport/connector seam, exact watch/channel subscriptions, a manually released semaphore clock, and bounded three-second state deadlines. No test helper process or daemon is spawned by these new tests. Existing terminal regression tests exercise real local PTYs. No remote SSH host or production daemon was exercised; these tests establish controller behavior, not a new live-host survival claim.

LSP diagnostics were requested on all four changed Rust files; each failed because the LSP daemon socket was unreachable. Rust compilation and the related suite above provide compiler verification instead. Build logs are `/tmp/st_01a0848d-build.log`, `/tmp/st_01a0848d-terminal.log`, `/tmp/st_01a0848d-green-native.log`, and `/tmp/st_01a0848d-red.log` on this workstation. No commits were created.
