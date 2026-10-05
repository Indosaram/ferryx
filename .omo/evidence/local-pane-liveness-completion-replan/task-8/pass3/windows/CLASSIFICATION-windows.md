# Full `--lib` failure classification — windows

Candidate: `abd9e890`  Base: `d82b35e4`
Command (both sides): `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`

- **Candidate**: `test result: FAILED. 2201 passed; 70 failed; 8 ignored; 0 measured; 1 filtered out; finished in 1095.61s`
  (`windows/logs/full-lib.log`). The `1 filtered out` is
  `ssh::bridge::tests::ssh_bridge_live_loopback_openssh_connection`, which **hung** and was killed —
  reported **NOT_RUN**, never a pass.
- **Base**: `d82b35e4`, run against a properly rebuilt base tree (candidate-added modules removed, base
  `src-tauri` restored: `split_journal.rs` absent, `qa_barrier.rs` absent, `protocol.rs` = `83177837…`,
  `ipc/tests.rs` = `22939ea6…`). The run was **bounded at 2174 of 2280 tests** because it was spinning
  on `worktree::disk_tests::disk_scan_plain_folder_has_no_worktrees` (a Windows temp-dir scan hazard —
  the **candidate** run also spun on that same test for ~6 minutes before escaping). Partial log
  preserved as `windows/logs/full-lib-base-PARTIAL.log`.

| Classification | Count |
| --- | --- |
| candidate-caused | 5 |
| pre-existing | 65 |
| unclassified | 0 |
| **total** | **70** |

**Strict rule used, and why it matters.** `pre-existing` requires an explicit `test NAME ... FAILED`
line in the base log; `candidate-caused` requires an explicit `test NAME ... ok` line; a test that
merely *started* at base (no verdict line) is not credited either way. This is load-bearing: **9 tests
had started at base with no verdict line** when the run was bounded, so a looser rule would have
silently mislabelled them.

The single test the base full run never reached,
`worktree::tests::plain_folder_without_git_registers_and_guards_worktrees`, was then resolved by a
**two-repetition scoped A/B on both sides** (`windows/ab2-windows.log`): it is
**`test result: FAILED. 0 passed; 1 failed` on base AND on candidate, in both repetitions** —
```
AB2 side=base rep=1 native=101 | test result: FAILED. 0 passed; 1 failed
AB2 side=cand rep=1 native=101 | test result: FAILED. 0 passed; 1 failed
AB2 side=base rep=2 native=101 | test result: FAILED. 0 passed; 1 failed
AB2 side=cand rep=2 native=101 | test result: FAILED. 0 passed; 1 failed
```
At base the panic is
`assertion \`left == right\` failed / left: "\\\\?\\C:\\Users\\sook" / right: "\\\\?\\C:\\Users\\sook\\AppData\\Local\\Temp\\.tmpr6fraO"`
(`src/worktree/mod.rs:49`) — i.e. it fails at base too, so it is **pre-existing**. It is a Windows
long-path defect (`\\?\` stripping truncates the path to the user root), not a candidate change.

## candidate-caused (5)

| Test | base | candidate | panic |
| --- | --- | --- | --- |
| `daemon::session_service::machine_tests::interrupted_spawn_never_repeats_preallocated_intent` | **ok** | **FAILED** | `src\daemon\session_service_machine_tests.rs:344:63` — called `Result::unwrap()` on an `Err` value: Other("Timed out waiting for PTY reader shutdown") |
| `native_terminal::surface_host::tests::bounds_ipc_presents_when_browser_child_is_open` | **ok** | **FAILED** | `src\native_terminal\surface_host.rs:5961:9` — assertion failed: receipt.presented |
| `native_terminal::surface_host::tests::deferred_bounds_retry_does_not_restore_obsolete_width` | **ok** | **FAILED** | `src\native_terminal\surface_host.rs:6183:13` — assertion failed: latest.presented |
| `native_terminal::surface_host::tests::synchronized_output_bounds_ipc_finishes_on_detach_or_stream_end` | **ok** | **FAILED** | `src\native_terminal\surface_host.rs:6269:17` — assertion failed: result.unwrap().presented |
| `native_terminal::surface_host::tests::synchronized_output_bounds_ipc_waits_for_actual_presentation` | **ok** | **FAILED** | `src\native_terminal\surface_host.rs:6072:9` — assertion failed: receipt.presented |

## pre-existing (65)

Each carries an explicit `... FAILED` verdict in the base run (or, for the one scoped case, in its
scoped base run), so the candidate did not introduce it.

- `clipboard_image::tests::save_paste_file_in_dir_writes_file_and_prunes_stale_files`
- `daemon::session_service::machine_tests::dropped_http_reply_replays_original_process_and_controller_fences_close`
- `daemon::session_service::machine_tests::served_session_cwd_falls_back_when_the_stored_value_is_probe_output`
- `daemon::session_service::machine_tests::sixty_four_live_machine_sessions_are_the_admission_limit`
- `daemon::workspace_service::catalog_tests::post_rename_sync_failure_fences_publication`
- `daemon::workspace_service::catalog_tests::unrelated_workspace_registration_progresses_while_workspace_fenced`
- `ipc::agents::tests::test_agents_detect_existing_and_nonexistent_binaries`
- `ipc::dag::tests::test_dag_read_node_artifact_security_and_success`
- `ipc::terminal::tests::session_cwd_text_accepts_posix_and_windows_absolute_paths`
- `ipc::worktree_disk_tests::disk_scan_commands_emit_completion_and_reuse_cache_until_refresh`
- `native_terminal::platform::windows::pointer_tests::presented_terminal_yields_cross_thread_pointer_hit_testing_to_input`
- `paired_host::client::tests::real_machine_catalog_replay_checks_digest_and_preserves_metadata`
- `remote::auth::persistence_tests::test_p03_begin_transaction_runtime_io_failure_propagates_storage_error`
- `remote::machine_operation_journal::contention_tests::journal_writer_preserves_real_machine_socket_attachment`
- `remote::server::machine_output_writer::tests::blocked_tcp_write_expires_when_ten_seconds_elapse`
- `remote::server::machine_output_writer::tests::blocked_tcp_write_is_cancelled_when_output_overflows`
- `remote::server::tests::remote_sessions_use_the_desktop_tab_label_as_the_title`
- `remote::server::tests::test_get_active_running_sessions_independent_of_desktop`
- `remote::server::tests::test_worktree_list_failure_returns_structured_error_not_200_empty`
- `remote::state::tests::p19_overlay_proof_fails_closed_without_authoritative_cli_confirmation`
- `remote::state::tests::test_workspace_snapshot_expired_cache_awaits_rebuild_and_reflects_external_changes`
- `remote::workspace_api::worktrees::authority_tests::followthrough_delete_publication_blocks_spawn`
- `remote::workspace_api::worktrees::authority_tests::local_worktree_errors_survive_owner_wire_and_native_adapter`
- `remote::workspace_api::worktrees::authority_tests::owner_http_gate_and_eight_admissions`
- `remote::workspace_api_tests::r1_topology_after_gate_http`
- `rollout_tests::a24_rollback_waits_for_drain_and_preserves_unrelated_owner`
- `ssh::bridge::tests::ssh_bridge_dag_frame_ordering_requires_contiguity_or_resync`
- `ssh::bridge::tests::ssh_bridge_dag_subscription_drop_releases_helper_and_keeps_pty_alive`
- `ssh::bridge::tests::ssh_bridge_dag_subscription_rejects_unregistered_project`
- `ssh::bridge::tests::ssh_bridge_dag_subscription_released_after_eof_during_blocked_next`
- `ssh::bridge::tests::ssh_bridge_dag_subscription_streams_inventory_updates_and_cancels`
- `ssh::bridge::tests::ssh_bridge_handshake_verifies_host_and_epoch`
- `ssh::bridge::tests::ssh_bridge_independent_read_does_not_block_control`
- `ssh::bridge::tests::ssh_bridge_lifecycle_poison_on_timeout_cancel_and_eof_reaping`
- `ssh::bridge::tests::ssh_bridge_single_attempt_write_does_not_queue`
- `ssh::bridge::tests::ssh_bridge_spawn_retry_requires_matching_client_request_id`
- `ssh::bridge::tests::ssh_bridge_target_expired_fails_explicitly_never_spawns`
- `ssh::bridge::tests::ssh_bridge_target_mismatch_on_describe_or_read_is_rejected`
- `ssh::bridge::tests::test_ssh_bridge_dag_inventory_and_poll`
- `ssh::bridge::transfer_tests::test_bridge_drop_after_detach_does_not_kill_child`
- `ssh::bridge::transfer_tests::test_bridge_freeze_export_import_roundtrip`
- `ssh::bridge::transfer_tests::test_bridge_poisoned_connection_export_rejected`
- `ssh::bridge::transfer_tests::test_ssh_bridge_client_transfer_roundtrip`
- `ssh::direct::tests::automated_commands_disable_tty_even_when_ssh_config_requests_one`
- `ssh::direct::tests::p10_windows_bridge_preserves_native_argv`
- `ssh::direct::tests::shell_plan_with_session_forwards_agent_state_socket_and_exports_env`
- `ssh::direct::tests::ssh_bridge_plan_options_disable_tty_and_enforce_strict_host_keys`
- `ssh::direct::tests::ssh_bridge_plan_windows_uses_raw_child_stdio_forwarding`
- `ssh::direct::tests::ssh_reconnect_safety_plan_does_not_update_host_keys`
- `ssh::direct::tests::startup_plan_honors_saved_options_and_quotes_remote_root`
- `ssh::exec::tests::red_interactive_argv_flags`
- `ssh::exec::tests::red_probe_argv_shape`
- `ssh::helper_setup::tests::posix_upload_script_skips_kill_for_foreign_process`
- `ssh::projects::tests::lead_regression_local_and_remote_same_path_can_reveal_locally`
- `terminal::remote::tests::test_remote_runtime_drop_after_detach_does_not_kill_child`
- `terminal::remote::tests::test_remote_runtime_poisoned_bridge_export_rejected`
- `terminal::shell::tests::test_linux_default`
- `terminal::tests::test_lifecycle_poll_interval_is_relaxed_and_event_driven`
- `terminal::tests::test_multiple_concurrent_sessions`
- `terminal::tests::test_spawn_write_echo_and_read`
- `worktree::disk_tests::disk_scan_merges_dirty_locked_prunable_and_detached_metadata`
- `worktree::disk_tests::disk_scan_plain_folder_has_no_worktrees`
- `worktree::registry::tests::register_unique_root_disambiguates_conflicting_ids`
- `worktree::rescan::tests::sweep_baseline_is_silent_then_emits_created_and_settles`
- `worktree::tests::plain_folder_without_git_registers_and_guards_worktrees` — scoped base run FAILED

## Cross-platform: the candidate-caused set

| Test | linux | mac | windows |
| --- | --- | --- | --- |
| `ipc::tests::tauri_mock_terminal_attach_returns_base64_history_and_decimal_sequences` | FAIL | FAIL | **absent** — `#[cfg(all(test, unix))]`, `src-tauri/src/ipc/mod.rs:57-58` |
| `ipc::tests::test_p13_attach_routes_through_descriptor_and_reinstalls_proxy` | FAIL | FAIL | **absent** (same gate) |
| `native_terminal::surface_host::tests::bounds_ipc_presents_when_browser_child_is_open` | FAIL | FAIL | **FAIL** |
| `native_terminal::surface_host::tests::deferred_bounds_retry_does_not_restore_obsolete_width` | FAIL | FAIL | **FAIL** |
| `native_terminal::surface_host::tests::synchronized_output_bounds_ipc_finishes_on_detach_or_stream_end` | FAIL | FAIL | **FAIL** |
| `native_terminal::surface_host::tests::synchronized_output_bounds_ipc_waits_for_actual_presentation` | FAIL | FAIL | **FAIL** |

**The four `surface_host` presentation failures are candidate-caused on all three hosts.** The two
`ipc::tests` attach failures are candidate-caused on mac and linux and cannot exist on Windows because
the whole module is unix-gated (verified: 0 grep hits for those names in the Windows log, 4 hits each
in the mac log).

Windows shows one further candidate-caused test,
`daemon::session_service::machine_tests::interrupted_spawn_never_repeats_preallocated_intent`:
**base full-suite `... ok`**, **candidate full-suite `... FAILED`** with
`called Result::unwrap() on an Err value: Other("Timed out waiting for PTY reader shutdown")` at
`src/daemon/session_service_machine_tests.rs:344:63`.

**Determinism caveat, stated plainly.** See the dedicated evidence section at the end of this file.
## Environment-shaped families inside the pre-existing set (routing evidence)

| Family | Signature |
| --- | --- |
| remote-helper / cli binary absent | panic names a missing `ferryx-remote-helper` / `ferryx-cli` binary, or `NotFound` |
| windows long-path prefix | `git worktree add` / `\\?\` stripping rejects the path ("Invalid argument", or a truncated `left`/`right`) |
| fixture timeouts | `Elapsed(())` / `Timed out` |


## Exact scoped-A/B evidence for the `interrupted_spawn` caveat

Command (both sides, same command, host **DESKTOP-1LAPJMP** = `sook@100.126.171.58`):

```
cargo test --manifest-path src-tauri/Cargo.toml --lib \
  daemon::session_service::machine_tests::interrupted_spawn_never_repeats_preallocated_intent \
  -- --nocapture --test-threads=1
```

| Attempt | Side | Log | Bytes | `test result:` lines | `Timed out waiting for PTY reader shutdown` |
| --- | --- | --- | --- | --- | --- |
| ab2 rep1 | **base** | `windows/logs/ab2-base-1-…interrupted_spawn….log` | 27890 | **0** | **0** |
| ab3 | **base** | `windows/logs/ab3-base-…interrupted_spawn….log` | 27890 | **0** | **0** |
| ab2 rep1 | cand | `windows/logs/ab2-cand-1-…interrupted_spawn….log` | 30848 | 0 | **1** |
| ab3 | cand | `windows/logs/ab3-cand-…interrupted_spawn….log` | 31002 | 0 | **1** |

**Base side, verbatim tail (both attempts stop at the same point, 27890 bytes each):**

```
thread 'tokio-rt-worker' (11612) panicked at src\daemon\session_service_machine_tests.rs:321:17:
A09 controlled interruption at sessionSpawned
A09 interruption=sessionSpawned target=909eaab9-2441-483e-977b-c8d7b777b074 original_pid=Some(10120) replay=outcomeUnknown no-repeat=true
A09 interrupted owner cleanup: every owned PTY reaped and private root removed
```

```
thread 'tokio-rt-worker' (27292) panicked at src\daemon\session_service_machine_tests.rs:321:17:
A09 controlled interruption at sessionSpawned
A09 interruption=sessionSpawned target=a8d63170-eafd-4042-9d48-1b6feb66be7f original_pid=Some(27644) replay=outcomeUnknown no-repeat=true
A09 interrupted owner cleanup: every owned PTY reaped and private root removed
```

**Candidate side, verbatim tail (the extra panic the base never emits):**

```
thread 'daemon::session_service::machine_tests::interrupted_spawn_never_repeats_preallocated_intent' (13960) panicked at src\daemon\session_service_machine_tests.rs:344:63:
called `Result::unwrap()` on an `Err` value: Other("Timed out waiting for PTY reader shutdown")
```

**How these runs ended, stated precisely.** **No** scoped run on either side ever printed a
`test result:` line, so `native=-1` in `windows/ab2-windows.log` and `windows/ab3-windows.log` is a
**kill artifact from the verifier terminating the run**, not a test outcome. The two base attempts
stopped at an identical byte count with no verdict and no timeout panic; the two candidate attempts
stopped after emitting the timeout panic. The two `tokio-rt-worker` panics are the test's own
**intentional** `A09 controlled interruption` fixture, present on both sides.

**Verdict for this row: UNRESOLVED BY REPETITION — not a confirmed regression.**
Scoped repetition is inconclusive on **both** sides (neither side produced a verdict), so it can
neither confirm nor refute the full-suite A/B, which is the only evidence for the row:
**base full suite `ok` → candidate full suite `FAILED`** with that timeout. That single-run A/B
cannot separate three possibilities, and the record must not pretend otherwise:

1. the candidate introduced a real regression in the PTY-reader shutdown path;
2. a load-sensitive flake (the candidate's failure mode is a *timeout*, and this host was carrying
   several other sessions' cargo processes throughout);
3. the candidate **newly detects** a pre-existing condition — note the base scoped run *also* never
   completed and has no timeout detection of its own, so the underlying PTY-reader-shutdown
   condition may exist at base as well and simply be reported instead of waited on.

Routing guidance: treat this as **"needs a dedicated A/B on a quiet host"**, not as a confirmed
candidate defect. Do not cite it as the reason for a repair without that measurement.
