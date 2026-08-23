# Fix: P-ipc-batch (F-terminal-03, F-terminal-04)

## Summary
- **Packet ID**: P-ipc-batch
- **Finding IDs**: F-terminal-03, F-terminal-04
- **Severity**: High / Medium
- **Description**:
  - **F-terminal-03**: Coalesced and micro-batched PTY streaming output in `src-tauri/src/ipc/terminal.rs`. Replaced 1:1 chunk-to-IPC event forwarding with an accumulated byte buffer and bounded flush window (10ms interval or 32KB max batch). This reduces Tauri IPC event emissions and Base64 string serialization overhead during high-throughput terminal bursts without compromising interactive typing latency.
  - **F-terminal-04**: Replaced `/usr/sbin/lsof` child process execution in `cmd_terminal_get_cwd` on macOS with direct in-process kernel memory inspection via `libproc` (`proc_pidinfo` with `PROC_PIDVNODEPATHINFO`). Added in-memory CWD caching (`CWD_CACHE`) with a 500ms TTL and session invalidation on close to eliminate repeated disk/process queries.

## Files Changed
- `src-tauri/src/ipc/terminal.rs` (production implementation)
- `src-tauri/src/ipc/tests.rs` (RED/GREEN test suite)

## Production Change Location
- File: `src-tauri/src/ipc/terminal.rs`
- Key changes:
  - Lines 18–20: Defined batching and cache constants (`BATCH_FLUSH_INTERVAL = 10ms`, `BATCH_MAX_BYTES = 32KiB`, `CWD_CACHE_TTL = 500ms`).
  - Lines 22–47: Implemented `CWD_CACHE` with `get_cached_cwd`, `update_cached_cwd`, and `invalidate_cached_cwd`.
  - Lines 116–133: Implemented `flush_terminal_output` helper for buffered base64 encoding and event emission.
  - Lines 206–278: In `cmd_terminal_spawn`, implemented buffered async select loop with `try_recv` drainage and 10ms deadline flush.
  - Lines 317–342: Updated `cmd_terminal_get_cwd` with cache lookup and async `run_blocking` resolution.
  - Lines 344–428: Implemented `macos_proc::get_proc_cwd` via `proc_pidinfo(PROC_PIDVNODEPATHINFO)` and updated `process_cwd`.
  - Lines 459–467: Invalidated session CWD in `cmd_terminal_close`.

## RED Test Run

Command:
```bash
cargo test --manifest-path /Users/indo/code/project/orca-lite/src-tauri/Cargo.toml --lib ipc::
```

Output:
```
   Compiling ferryx v0.1.0 (/Users/indo/code/project/orca-lite/src-tauri)
error[E0425]: cannot find function `get_cached_cwd` in this scope
   --> src/ipc/tests.rs:274:18
    |
274 |     let cached = get_cached_cwd(&spawned.session_id);
    |                  ^^^^^^^^^^^^^^ not found in this scope

error[E0425]: cannot find function `get_cached_cwd` in this scope
   --> src/ipc/tests.rs:280:16
    |
280 |     assert_eq!(get_cached_cwd(&spawned.session_id), None);
    |                ^^^^^^^^^^^^^^ not found in this scope

For more information about this error, try `rustc --explain E0425`.
error: could not compile `ferryx` (lib test) due to 2 previous errors
```

## GREEN Test Run

Command:
```bash
cargo test --manifest-path /Users/indo/code/project/orca-lite/src-tauri/Cargo.toml --lib ipc::
```

Output:
```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 5.79s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/ferryx_lib-d3b00f8cfc2e62f0)

running 18 tests
test ipc::notifications::tests::picker_filters_only_offer_decodable_formats ... ok
test ipc::notifications::tests::dispatch_result_round_trips_across_the_ipc_boundary ... ok
test ipc::notifications::tests::probe_result_round_trips_across_the_ipc_boundary ... ok
test ipc::notifications::tests::permission_status_round_trips_across_the_ipc_boundary ... ok
test ipc::notifications::tests::permission_request_result_round_trips_across_the_ipc_boundary ... ok
test ipc::notifications::tests::picked_audio_result_round_trips_including_cancellation ... ok
test ipc::notifications::tests::dispatch_command_payload_matches_the_frontend_contract ... ok
test ipc::tests::terminal_process_cwd_resolves_accurately ... ok
test ipc::notifications::tests::open_system_settings_command_returns_a_structured_result ... ok
test ipc::blocking_contract_tests::blocking_ipc_helper_runs_operation_off_async_caller_thread ... ok
test ipc::notifications::tests::play_sound_command_rejects_unsupported_formats ... ok
test ipc::notifications::tests::play_sound_command_tolerates_omitted_optional_arguments ... ok
test ipc::notifications::tests::play_sound_command_reports_missing_file_without_erroring ... ok
test ipc::tests::terminal_cwd_cache_and_resolution_contract ... ok
test ipc::tests::tauri_mock_worktree_commands_use_identity_contract ... ok
test ipc::tests::terminal_output_batching_coalesces_rapid_bursts ... ok
test ipc::tests::tauri_mock_terminal_events_use_registered_workspace ... ok
test ipc::tests::terminal_global_events_preserve_raw_bytes_and_lifecycle ... ok

test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 111 filtered out; finished in 1.49s
```

## Invariants & Leftover Risk
- **Byte Fidelity**: Preserved exact raw bytes and sequence ordering across batched Base64 encodings and Tauri IPC event broadcasts.
- **Latency**: Interactive keystroke echo latency is bounded to <=10ms while high-throughput bursts flush immediately upon hitting 32KB.
- **Process Safety**: macOS `libproc` queries kernel vnode information safely via `proc_pidinfo` and falls back gracefully if unavailable.
- **No Residual Locks**: CWD cache uses short-lived granular parking_lot mutex locks and auto-evicts on session teardown.
