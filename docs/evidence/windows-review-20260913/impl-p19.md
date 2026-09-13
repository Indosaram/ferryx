# P19 - daemon persistence fixture isolation

Owner: st_01a09a08. Date: 2026-09-13. Status: implementation staged for safe RED; execution blocked on lead-issued exclusive Darwin Cargo slot. **Not GREEN or complete.**

## Scope and registration

Only `src-tauri/tests/daemon_persistence_contract.rs` was modified, plus this report. The file was clean on initial and pre-edit scoped git checks; foreign dirty files were left untouched. Read root/backend AGENTS, programming Rust/debugging/ulw-loop skills, repair-packets P19, gap-packet-addendum, remaining register, current fixture and daemon endpoint/startup/shell/close implementation. No branch, commit, install, release or remote mutation.

Official `executeAgentToolkit` was imported from `/Users/indo/.bun/install/global/node_modules/omo-ai/plugin/extensions/omo-agent-toolkit.js`, bound to cwd `/Users/indo/code/project/orca-lite` and session `01a0983f-c995-753d-afa9-593f6d118788`. `steer/revise_criterion` appended the exact P19 prerequisite to C002 while retaining its full existing scenario. A follow-up read confirmed `P19 registered: true`. An official `annotate_ledger` execution-slot/P08 relay request returned `{"ok":true}`.

## Staged implementation

- Every real daemon launch now goes through one harness. Removed both uses of ambient `get_socket_path`/`get_lock_path`, including the separate headless cancellation fixture.
- Each harness owns a short `/tmp/p19-*` TempDir, runtime socket, home, data and session directories. `env_clear` plus child-only explicit HOME/USERPROFILE/XDG/runtime/data/session/PATH/SHELL settings prevents inherited endpoint/profile overrides. No process-global environment mutation.
- Child handle is immediately stored in the RAII harness before readiness reads/awaits. A subscribed oneshot bridges the blocking stdout readiness reader with a bounded async wait. Stderr is inherited, not an unread pipe that can deadlock.
- Cleanup kills/reaps only the actual `std::process::Child` handle, then TempDir removes only its owned tree. No wire-supplied PID kill, shared socket unlink, daemon Shutdown or reset. Cleanup failures are reported to stderr. Drop is shared by success, panic and readiness failure.
- Control and attach now share one handshake connection seam and bounded handshake/attach reads. The owned child PID is passed to that seam.
- **The PID check is deliberately not installed yet:** the file contains a labelled RED staging block. This retains historical admission behavior only behind isolated endpoints so the regression can demonstrate it safely before repair. Do not run the full target at this stage.
- Added `test_harness_rejects_foreign_handshake_before_commands`: real owned Unix listener receives the actual fixture handshake, returns matching version/wrong PID, observes EOF with zero commands, and asserts rejection. No daemon is spawned by this exact test.
- Added `test_harness_panic_cleanup_preserves_concurrent_owned_daemon`: starts two owned daemons concurrently, injects owner panic, requires first tree deletion and sibling handshake/Pong, then second tree deletion.
- Removed fixture sleep/polling after CloseOk: production close_session already mandates reap; subscribed stream Exit and CloseOk precede a process-status assertion.
- Sequence fixture now accumulates across arbitrary PTY chunks, uses a non-echoable expanded sentinel and empty shell prompt before asserting the final replay snapshot. `/bin/sh` is explicitly selected rather than user shell/preferences. Stream parse failures propagate instead of being ignored.

## Exact execution handoff (lead relay required)

No child task-send tool exists here. The following request was recorded through the official ledger; no Cargo command or daemon has been executed by this worker.

1. Grant the exclusive shared Darwin Cargo slot, or run and return the receipt for:

   ```sh
   cargo test --manifest-path src-tauri/Cargo.toml --test daemon_persistence_contract test_harness_rejects_foreign_handshake_before_commands -- --exact --nocapture --test-threads=1
   ```

   Intended RED: exactly one discovered test fails `foreign handshake PID must be rejected before commands`, after peer EOF/zero-command assertion passes. Compile errors, zero tests, unrelated fixture errors are not RED.

2. After that intended RED, replace the labelled ignored `expected_pid` staging block and permissive `HandshakeOk` match in `TestDaemonClient::connect` with acceptance only for `HandshakeOk { version, pid, .. } if version == DAEMON_PROTOCOL_VERSION && pid == expected_pid`; return an error otherwise. This same seam protects attach and control, including startup readiness probing. Keep the exact assertions unchanged.

3. Run the identical exact command GREEN, then:

   ```sh
   cargo test --manifest-path src-tauri/Cargo.toml --test daemon_persistence_contract -- --test-threads=1
   ```

   There are 11 test declarations in staged Unix source (previous nine plus two). Require all discovered tests passing, actual panic cleanup/Pong receipts, no residual owned roots/children. Use only the current Cargo-built debug `CARGO_BIN_EXE_ferryx`; record both executable and source SHA256, counts and exit status. Never use installed Ferryx. Full target includes the actual CLI surface.

4. P08 must confirm the Unix request still accepts `Handshake { version }` and responds with current `HandshakeOk { version, pid, .. }`. Current inspected protocol constant is 3. No foreign protocol files were modified. If P08 adds capability fields, relay its agreed Unix API before compiling the fixture.

5. Native Windows counterpart is not supplied by this UnixStream target. Keep `cfg(unix)`. Runtime owner st_01a099f8 must separately prove two owned native endpoints/profiles, matching child PID, unauthorized endpoint rejection, panic cleanup preserving sibling PTY/endpoint and no live-user endpoint contact. Zero tests on Windows is not acceptance.

## Actual verification and cleanup receipts

- `lsp_diagnostics(src-tauri/tests/daemon_persistence_contract.rs, all)` before and after formatting: `No diagnostics found`.
- Initial `rustfmt --edition 2021 --check ...`: exit 1, formatting differences only; formatted the scoped file with rustfmt.
- Final `rustfmt --edition 2021 --check src-tauri/tests/daemon_persistence_contract.rs`: exit 0.
- `git diff --check -- src-tauri/tests/daemon_persistence_contract.rs`: exit 0.
- Source SHA256 at staging: `517dbd5a4a991e039c988327b419a871ecba702cb81585b49fb9619d2b5371c1`.
- Search confirms no ambient endpoint getters, TokioCommand bypass or fixture `sleep(` remains.
- RED log: **not run, no exclusive slot issued**. GREEN/full Cargo/build logs: **not run**. Diagnostics are not compilation proof.
- No daemon, PTY, listener or runtime root was created by this worker. Thus there are no runtime resources to terminate. `/tmp/p19-rustfmt-check.log` was the only owned temporary diagnostic file and was removed after formatting validation; tool-managed stdout logs are retained by the harness.

All changes remain uncommitted in the shared tree. P19 requires the narrowly specified RED execution receipt before the final identity guard can be installed without violating the requested failing-first order. This report makes no aggregate completion claim.
