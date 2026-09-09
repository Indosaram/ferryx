# Daemon SSH routing - st_01a08498

## Implemented

Registered SSH starts now resolve the registered project and enabled host using the daemon-configured inventory, detect the remote environment, derive the installed helper location, and call `TerminalService.remote().create`. There is no direct SSH login-shell PTY fallback. Existing `RemoteSsh.hostStorePath` is compatibility-only and must equal the configured path. Default inventory matches `com.ferryx.app[/dev]/ssh_hosts.json`; `FERRYX_DATA_DIR/ssh_hosts.json` is shared by desktop and daemon overrides. Isolated test constructors use their own inventory.

Remote descriptors and daemon metadata (original backend ID, immutable request ID, workspace/worktree/CWD and spawn fingerprint) are atomically persisted through `run_blocking` before SpawnOk. Immutable pending spawn requests are recorded before create; an ambiguous pending request fails closed rather than starting a replacement shell. Remote identities use a separate `remote_sessions.json` next to the session-directory override (otherwise runtime directory), leaving existing UI workspace/pane and `agentProviderSessions` payloads unchanged. Cursor updates are watched and persisted. Explicit close stops the remote target and removes its identity; local close does not touch remote persistence. Shutdown checkpoints but never calls remote stop.

Startup restores identities before daemon readiness, registers them with the router, and calls runtime restore, never spawn/resume. Describe/List/Attach include remote sessions even while disconnected. Legacy direct SSH sessions have no fabricated target; status reports `legacyDirectSsh` for daemon-owned legacy metadata. Handover-owned predecessor sessions remain routed to their existing peer.

## Additive API (protocol remains 3)

Requests:

- `{"type":"remoteSessionDetails","sessionId":"..."}` -> `remoteSessionDetailsOk { details: RemoteSessionDetails|null, legacyDirectSsh: boolean }`.
- `{"type":"retryRemoteSession","sessionId":"..."}` -> `retryRemoteSessionOk` or `remoteSessionError {failure:{kind,message}}`; retries stored target only.
- `remoteWrite {sessionId,generation,data}` (base64 data), `remoteResize {sessionId,generation,cols,rows}` -> existing success variants or typed remoteSessionError. No write retry is introduced.

Attach streams carry `remoteStatus {sessionId,state,generation,failure,replayGap}`. State is connected/reconnecting/disconnected/expired. Remote replayGap cursor strings are NOT local sequence numbers. Ordered hub gap markers are encoded as existing local `Gap` frames before recovered bytes, including batch-drain boundaries. Proxy subscriptions preserve the marker. Native consumers use their existing Gap reset behavior. Desktop event `terminal_remote_status` has the same sessionId/state/generation/failure/replayGap fields; the global remote event channel also forwards it.

Tauri commands implemented (lead must register in lib.rs):

- `cmd_terminal_remote_status {sessionId}` -> full daemon response above.
- `cmd_terminal_remote_retry {sessionId}` -> full daemon response above.
- `cmd_terminal_remote_write {sessionId,generation,data}` -> void or IpcError with typed failure in details (data is a plain string here).
- `cmd_terminal_remote_resize {sessionId,generation,cols,rows}` -> same error convention.
- `cmd_ssh_install_project_helper {workspaceId,localBinary}` -> HelperLocation. Explicit artifact installation only; host/environment/location are resolved backend-side, no compilation/download.

Existing local write/resize signatures and backendSessionId/sessionId semantics remain unchanged.

## Restart replay policy and remaining limitations

Per lead direction, restart retains the exact target/backend/request identity but resets the remote read cursor to zero before runtime restore. This rebuilds the fresh hub from all helper-retained output; helper retention gaps remain explicit ordered gap markers. Every remote Attach ignores stale local afterSequence, returns the complete currently retained hub history and segmented sizes, and includes an explicit reset gap. Subsequent bytes arrive on the subscriber created with that snapshot. This avoids racing an independent history checkpoint against cursor advancement and prevents appending replay to an old parser screen. Local Attach semantics are unchanged. No atomic checkpoint API is required for this chosen replay policy.

The request contract has no frontend pane ID. Existing saved workspace/pane mappings are preserved byte-for-byte by the untouched session DTOs, but a brand-new pane association cannot be invented before frontend supplies/saves it. Pending ambiguous spawn journals currently fail closed for manual recovery; no automatic target recovery API exists for that case.

Legacy peer status cannot yet identify a pre-feature direct SSH session after all of its metadata is gone. It reports no descriptor, never preserved identity.

`remote/server.rs` WebSocket batching ignores empty gap-marked chunks; this caller was reported to lead as outside scope. Native/output Attach subscribers are wired.

## Verification evidence

All commands run on macOS arm64. No production daemon, default socket daemon, or `bun tauri dev` was launched; no git commits.

- RED before production edits: `cargo test --lib ssh_daemon_restart` and `cargo test --lib ssh_reconnect_safety` failed on absent persistence/retry APIs. `/tmp/st_01a08498-red.log`, `...-red-safety.log`.
- Owned daemon surface tests: `cargo test --lib daemon::server::ssh_survival_tests`: 4 passed. Identity restore without local PTY; JSON handler Describe/Attach/status; ordered gap then recovered output; actual Unix proxy gap preservation. `/tmp/st_01a08498-proxy-gap.log`.
- Real loopback OpenSSH/helper/worktree/gateway regression: `cargo test --lib direct_ssh_real_transport_registration_and_pty`: 1 passed, 6.52s. Explicit existing helper artifact installed into unique short test homes. First run failed on macOS SUN_LEN; corrected test roots to unique `/tmp/fx*`. `/tmp/st_01a08498-ssh-worktree.log`, `...-ssh-worktree-shortpath.log`.
- Local agent reconnect: `cargo test --lib test_agent_resume`: 13 passed after fixing remote persistence being incorrectly invoked by local close. Initial failure and correction: `/tmp/st_01a08498-local-agent.log`, `...-local-agent-fixed.log`.
- Local spawn/idempotency/CWD: `cargo test --lib test_server_spawn`: 2 passed. `/tmp/st_01a08498-local-spawn.log`.
- `cargo build --lib`: passed with 16 existing warnings. `/tmp/st_01a08498-build.log`.
- `cargo check --tests`: unrelated existing scoped_design PNG `output_buffer_size()` Option/usize errors; not changed. `/tmp/st_01a08498-compile-fixed.log`.
- LSP requested on all changed files; unavailable daemon socket. Compiler/build results are the available diagnostics.

Replay-policy follow-up: `/tmp/st_01a08498-replay-red.log` records the behavioral RED (persisted cursor 73 was incorrectly restored into an empty hub). `/tmp/st_01a08498-replay-green.log` records 4 owned tests passing after the fix, including remote Attach with stale afterSequence 999 returning reset gap plus full retained replay. `/tmp/st_01a08498-replay-build.log` records a successful library build. Both IPC RemoteStatus cases and the lead-owned native case compile; earlier missing-arm logs are stale.

Live restart follow-up: `/tmp/st_01a08498-live-restart.log` records the expanded real loopback SSH test passing (1 test, 7.45s). It checkpoints the remote identity, drops the old DaemonServer/runtime (weak ownership proves it is gone), constructs a new isolated daemon server, restores from disk, and asserts identical TargetRef, remote PID and backend ID, no local PTY, and replay of the original output without rerunning the command. The existing worktree/gateway/explicit-close/revocation checks also run in that fixture. No duplicate ssh_reconnect_safety run was performed while lead owned that validation.

These results prove in-process daemon-object/runtime restart against a real SSH helper, not a full daemon Child process restart. The named final scenario harness belongs to the next verifier. No final acceptance criterion is marked complete.
