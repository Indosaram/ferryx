# Writer/Paste Isolated Implementation Gate Review

## Verdict: BLOCK

The delivered patch (`writer-paste-forward.patch`) advances the implementation by routing daemon and primary remote WebSocket write paths through `TerminalService`'s transaction lock and fenced coordinator. However, critical production paths remain unrouted, errors during paste commit are swallowed, coordinator sequence advancement occurs before asynchronous PTY delivery resolves, and non-Unix platforms lack partial-write telemetry.

## Source Evidence & Hashes

- Frozen patch: `.omo/evidence/superlogical-parity/writer-paste-forward.patch`
  SHA-256: `390df2ee4c9553318fed12811f07ff0a49a5166b703880987924dd1a766c75d3`
- Scratch repository: `/tmp/ferryx-superlogical-writer-wave1` (base `a6076ddd79e5a48293ec1de364383b9c1813b29e`)
  - `src-tauri/src/terminal/writer_fenced_input.rs`: `8920b9daf71e5e78e0db5ec7472fd20c2f73b8d385339371997f5e18d8debb70`
  - `src-tauri/src/terminal/service.rs`: `e77f921b79db33c1abfefabcba556b870c512183bb9ccaebc15e0b52b28da0d8`
  - `src-tauri/src/terminal/mod.rs`: `d8db66425f82b413d6627ba9cd6bfada9fb79a4675513f0dcbeb1d974b5510eb`
  - `src-tauri/src/daemon/protocol.rs`: `13f46e8b04d9a49e0a27bb3a168dbb13f0fbb570833ec2eaf2848369249c6416`
  - `src-tauri/src/daemon/server.rs`: `0a17ca27337074f71a523234918de88119f20b2b655cf56f0b76d92f85eb6b3e`
  - `src-tauri/src/daemon/session_service.rs`: `e0ccc0d9273feb9595364ae7931cc71f7cfcf8fada1f33f30298ac298e6756cb`
  - `src-tauri/src/remote/protocol.rs`: `f6993ea0d754b29fde3417573701a58337acb31a778ebb05927c499748709951`
  - `src-tauri/src/remote/server.rs`: `31bd9182a925008109f1de280541eb63acf3b8fe79bae066362ac23594a54bbe`
  - `src-tauri/tests/superlogical_writer_paste.rs`: `3ca825d2118e36503f92f55a908e2fc11f9db9de0970d80b6bc6e3233bf6d946`

## Detailed Findings

1. **Unrouted Production Routes (Interleaving & Bypass Risk)**
   - `remote/server.rs:2275-2282` (`handle_machine_terminal_socket`): Paired machine terminal writes write directly to `pty.write_input_cancellable(&bytes)`. They do not acquire `input_transaction_lock` or touch the coordinator, interleaving with active paste or fenced input.
   - `remote/server.rs:2534-2541, 2744-2751` & `remote/backend.rs:209`: Gateway SSH writes via `ssh_control` dispatch to `backend.write_generation`, which calls `write_input_operation` directly without acquiring `input_transaction_lock`.
   - `daemon/proxy.rs:990`: `DaemonProxy::write_input` calls `self.terminal_service.write_input_operation` directly without transaction locking.
   - `terminal/service.rs:569-576`: Synchronous `TerminalService::write_input` bypasses transaction coordination and calls `pty_manager.write_input` directly.

2. **Swallowed PTY Delivery Error on Paste Commit**
   - `remote/server.rs:2639-2642` & `:3172-3175`: In `PasteCommit`, `let _ = op.await;` explicitly discards PTY delivery failure. Because `coord.commit_paste()` already de-staged `active_paste`, if `write_input_operation` fails (transport drop or partial PTY write), staged bytes are permanently discarded while the client receives no error notification.

3. **Premature Sequence Advancement Before Async PTY Delivery**
   - `terminal/service.rs:98-120` (`write_fenced_input`) & `:150-175` (`write_legacy_input`): `coord.admit_input()` advances `last_accepted_seq` and drains `gap_queue` *before* `self.write_input_operation().await` completes. If the backend PTY write fails or times out, the coordinator has already marked the sequence accepted. Retried frames will be discarded as `DuplicateDropped` (`writer_fenced_input.rs:631-636`).

4. **Partial Write Asymmetry on Non-Unix Platforms**
   - `terminal/service.rs:275-290`: The `#[cfg(not(unix))]` branch in `write_input_operation` dispatches to `session.write_input(&data)` in `spawn_blocking` and formats a plain string error. `PartialPtyWriteFailure` is only constructed under `#[cfg(unix)]` (`:250-264`). Furthermore, within a 64 KiB chunk, if `session.write_input_slice` partially writes to kernel ConPTY/pipe before failing, `written_bytes` is unmeasured.

5. **Verified Invariants in Current Patch**
   - **Server-Derived Identity & Epoch**: `remote/server.rs:2432-2438` derives `client_id` from authenticated `device.id`, role from `DevicePermission::Control`, and epoch from `state.daemon_epoch`. Same-epoch takeover by a differing client is rejected (`writer_fenced_input.rs:528-535`).
   - **Gap Bounds & Queue Dedup**: `writer_fenced_input.rs:639-645` enforces `MAX_GAP_DISTANCE = 256`. Duplicate gap frames are deduplicated without re-accounting bytes (`:648-662`). Queue frames capped at 128 and 256 KiB (`:664-672`).
   - **Disconnect & Revocation Cleanup**: `handle_disconnect` and viewer registration clear `active_paste`, purge `gap_queue`, reset queue bytes to 0, and increment `active_epoch` (`writer_fenced_input.rs:510-520, 543-550`).
   - **Zero Bytes Before Commit**: `start_paste` and `append_paste_chunk` stage strictly in memory (`writer_fenced_input.rs:741-815`). Zero bytes are sent to PTY before `commit_paste`.

## Gate Verification Status

- Source inspection and invariant tracing completed strictly read-only.
- All unit, integration, and platform tests are explicitly **UNRUN** in this workspace pending execution by the sole designated remote verifier.
- No binaries compiled, no local test commands executed, no daemons spawned, and no shared working-tree source files touched.
