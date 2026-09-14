# A11 relay reset repair - st_01a09914

## Outcome

Required full relay_pairing_generation_regression invocation passed: **8 passed, 0 failed**, including a11_joint_machine_relay_runtime and a11_real_pty_failure_containment together. Actual relay HTTP/WS, production reverse client/gateway and original real PTYs were used. No reset was swallowed or retried. Acceptance beyond this target remains blocked by composed-source/test failures below.

## Root cause and focused repair

A13-issuer-repeat-A11-reset-red.log reproduced current failure (7 passed/1 failed), without reverting the shared PIN fix. Isolated trace also failed. A13-issuer-repeat-A11-reset-reason.log:416 recorded gateway output validation exiting with MACHINE_SERVICE_UNAVAILABLE; line419 recorded reverse WebSocket reset immediately afterward. validate_machine_target used try_read/try_lock: ordinary metadata/catalog writer contention was interpreted as loss of authority. This closed a valid PTY stream without handshake. The relay merely propagated that gateway reset.

Changed only these source seams:
- src-tauri/src/daemon/session_service.rs: existing validation becomes async on Arc<Self>; existing exact metadata/epoch/workspace/PTY checks execute in spawn_blocking. Both authority locks share a bounded 10-second deadline rather than failing on ordinary contention. A cancelled socket cannot send input through the read-only worker; pending lock work is bounded.
- src-tauri/src/remote/server.rs: four existing validation callers await it. Existing controller lock, lease fencing, revocation and socket lifetime selects remain.
- src-tauri/tests/relay_pairing_generation_regression.rs: marker failures identify their marker. Real retained-history overflow uses sixteen 64KiB shell outputs, each triggered only after the previous exact PTY marker. It still fills >=1MiB and asserts explicit replay gap and unchanged PID. The prior single 1MiB burst mixed history overflow with intentional 1MiB queued-output eviction; after fixing authority contention it failed specifically at REAL_GAP. No output budget was increased. The final unique marker distinguishes the end of retained replay from earlier acknowledged bursts.

No relay_client.rs, relay_server.rs, PIN coordinator, owner adapter, native inventory source or metadata event protocol was changed by this lane. Shared seam notification: A11-relay-reset-shared-request.md. All temporary tracing was removed.

## Commands and evidence

All logs/receipts below use prefix `docs/evidence/paired-daemon/A13-issuer-repeat-A11-reset-`. Existing `A13-issuer-repeat-run.sh LABEL COMMAND...` provides env -i, private HOME/runtime/data/sessions/XDG/TMP, explicit CARGO_HOME/RUSTUP_HOME, existing target, jobs2/debug0/incremental0 and monitored background Cargo.

- `final.log`: `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test relay_pairing_generation_regression -- --nocapture` -> 8 passed. Both live-PTY scenarios pass in this one run. Original PID79381 survives suffix/gap/resize/interrupt and is reaped by explicit DELETE; lost reply reconciliation, ticket replay/host/target/query denials and DELETE body preservation pass.
- `stream.log`: same Cargo options, `--test machine_terminal_stream -- --nocapture` -> 1 passed. Original-owner/focus independence, stale controller input/resize, other-device conflict, HTTP close, replay/gap/empty history, ticket reuse, size limits and revoked input pass. Listener refused, original PTYs reaped, private root removed.
- `build.log`: `cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli --bin ferryx-relay` -> exit0.
- `cli-help.log`: actual headless CLI reports its usage, exits1 (CLI has no --help success contract). `relay-help.log`: actual relay rejects unsupported --help, exits2. These are process execution checks, not claimed success scenarios. Real successful runtime evidence is the HTTP/WS/PTYS above.
- LSP error diagnostics on all three changed Rust files: none. Focused git diff --check: passed. Compiler's existing warnings retained.

## Honest broader verification blockers

`security.log`: `cargo test ... --no-default-features --lib remote:: -- --nocapture` -> **271 passed, 6 failed**. Relay/client tests, ticket tests, controller generation/reservation, revocation and original-owner stream checks pass. Failures:
1. Three machine_protocol fixture equality tests: newly composed title/agentType null fields absent from fixture expected JSON.
2. a10_pending_machine_input_is_dropped_on_disconnect_and_revoke: timeout at security_socket_tests.rs:129 waiting for fake SocketBackend input entry. Current production machine input goes directly to pty.write_input_cancellable, not that fake backend; this test seam is disconnected from production. Not changed/skipped here.
3. followthrough_write_failures_non_head_and_prunable_preview: assertion at worktree_authority_tests.rs:124 expected only durable catalog event; concurrent metadata events alter observation.
4. r12_unreadable_root: nested runtime panic workspace_api.rs:247.

`contracts.log`: narrower verification could not compile after concurrent native source composition: E0583 missing paired_host modules (see log). Native inventory is explicitly out of scope. This prevents current-tree revalidation; earlier green target/build evidence is not represented as verifying later native edits. No unrelated source fix was attempted.

Other intermediate logs are retained: green reached REAL_GAP and failed queued eviction; paced encountered metadata forward_metadata argument composition error; composed exposed an incorrect nonunique replay marker assertion (8 vs15), corrected with unique per-burst markers. These are not passed runs.

## Isolation / cleanup

Every owned command has .monitor (PID/exact command/exit/reaped), .exit and .cleanup private-root removal receipts. Full and injected-failure tests log original PTYs reaped and gateway/relay runtime tasks joined, private stores removed and listener refusal. No desktop, canonical daemon, user PTY, public relay, release, commit or deployment was used. Temporary tracing and debug journal were removed; launcher logs outside the worktree were removed. Existing shared modifications were preserved; this lane did not restore whole files. Git submodule inspection needs `-c diff.ignoreSubmodules=all` because the inherited ghostty path is a symlink.

## Architectural review

Validation owns exact target authority; the wrapper is async isolation of that same check, not a new trust boundary. No tagged variants, unsafe, casts, parameter bloat, defensive null layer, negative names, or input queue were introduced. All four callers use the shared repair. Existing real regression supplied failing-first behavior; the root reason was captured before repair. Existing files exceed the skill size ceiling (session_service1323/server2751/regression571 pure LOC at measurement). Broad splitting was deliberately not performed in concurrently owned files under the focused-repair/no-formatting constraint; this debt is disclosed. The new blocking helper has one caller because it is the necessary spawn_blocking boundary, not a hypothetical abstraction.
