# Full `--lib` failure classification — mac

Candidate: `abd9e890`  Base: `d82b35e4`
Command (both sides): `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`

- Candidate result: test result: FAILED. 2635 passed; 49 failed; 6 ignored; 0 measured; 0 filtered out; finished in 761.06s
- Base result: test result: FAILED. 2557 passed; 44 failed; 6 ignored; 0 measured; 2 filtered out; finished in 665.66s
- Base tests observed: 2607; candidate tests observed: 2690

| Classification | Count |
| --- | --- |
| candidate-caused | 6 |
| candidate-authored-test | 0 |
| pre-existing | 43 |
| unclassified | 0 |
| **total** | **49** |

Every entry is decided by the base A/B on the same host with the same command — no entry is inferred.

## candidate-caused (6)

### `ipc::tests::tauri_mock_terminal_attach_returns_base64_history_and_decimal_sequences`
- base d82b35e4: **passed**  |  candidate abd9e890: **FAILED**
- why: the test exists at base d82b35e4 and passes there; the candidate makes it fail
- panic: `src/ipc/tests.rs:313:6` — attach: IpcError { code: UnsupportedCapability, message: "Attach requires the persisted seven-field pane binding; pass attachTuple", details: None }

### `ipc::tests::test_p13_attach_routes_through_descriptor_and_reinstalls_proxy`
- base d82b35e4: **passed**  |  candidate abd9e890: **FAILED**
- why: the test exists at base d82b35e4 and passes there; the candidate makes it fail
- panic: `src/ipc/tests.rs:3426:6` — attach of paired session must succeed through reinstalled proxy: IpcError { code: UnsupportedCapability, message: "Attach requires the persisted seven-field pane binding; pass attachTuple", details: None }

### `native_terminal::surface_host::tests::bounds_ipc_presents_when_browser_child_is_open`
- base d82b35e4: **passed**  |  candidate abd9e890: **FAILED**
- why: the test exists at base d82b35e4 and passes there; the candidate makes it fail
- panic: `src/native_terminal/surface_host.rs:5961:9` — assertion failed: receipt.presented

### `native_terminal::surface_host::tests::deferred_bounds_retry_does_not_restore_obsolete_width`
- base d82b35e4: **passed**  |  candidate abd9e890: **FAILED**
- why: the test exists at base d82b35e4 and passes there; the candidate makes it fail
- panic: `src/native_terminal/surface_host.rs:6183:13` — assertion failed: latest.presented

### `native_terminal::surface_host::tests::synchronized_output_bounds_ipc_finishes_on_detach_or_stream_end`
- base d82b35e4: **passed**  |  candidate abd9e890: **FAILED**
- why: the test exists at base d82b35e4 and passes there; the candidate makes it fail
- panic: `src/native_terminal/surface_host.rs:6269:17` — assertion failed: result.unwrap().presented

### `native_terminal::surface_host::tests::synchronized_output_bounds_ipc_waits_for_actual_presentation`
- base d82b35e4: **passed**  |  candidate abd9e890: **FAILED**
- why: the test exists at base d82b35e4 and passes there; the candidate makes it fail
- panic: `src/native_terminal/surface_host.rs:6072:9` — assertion failed: receipt.presented

## pre-existing (43)

### `daemon::client::tests::test_client_dedicated_attach_stream_does_not_monopolize_control_connection`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/daemon/client.rs:5500:14` — spawn terminal: IpcError { code: IoError, message: "Timed out waiting for daemon response (15s)", details: None }

### `daemon::server::a03_owner_cli_fixture::a03_private_owner_cli_surface`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `(none)` — (no panic) stdout: Error: build ferryx-cli before the owner fixture

### `daemon::server::remote_ssh_tests::direct_ssh_real_transport_registration_and_pty`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/daemon/remote_ssh_tests.rs:301:5` — child stdout:

### `daemon::server::remote_ssh_tests::remote_ssh_first_spawn_provisions_qualified_helper_automatically`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/daemon/remote_ssh_tests.rs:784:5` — Explicit test prerequisite: build remote-helper before SSH daemon tests

### `daemon::session_service::machine_tests::crash::actual_owner_crash_reconciles_without_respawn`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/daemon/session_service_crash_tests.rs:87:102` — called `Result::unwrap()` on an `Err` value: Elapsed(())

### `ipc::file_link::file_link_tests::session_id_resolves_the_live_local_terminal_cwd`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ipc/file_link_tests.rs:599:6` — cd completion signal: Elapsed(())

### `ipc::tests::terminal_global_events_preserve_raw_bytes_and_lifecycle`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ipc/tests.rs:513:6` — raw output timeout: Elapsed(())

### `ipc::tests::terminal_output_batching_coalesces_rapid_bursts`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ipc/tests.rs:652:6` — burst output timeout: Elapsed(())

### `ipc::worktree::deletion_repair_tests::deletion_repair_missing_record_preview_and_targeted_cleanup`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ipc/worktree.rs:332:10` — called `Result::unwrap()` on an `Err` value: GitError { command: "git clone --no-hardlinks --no-checkout /Users/I552267/ferryx-pane-completion/source-21dea3c0 repo", stderr: "fatal: repository '/Users/I552267/ferryx-pane-completion/source-21dea3c0' does not exist", stdout: "", code: Some(128) }

### `ipc::worktree::deletion_repair_tests::deletion_repair_preview_reports_current_dirty_and_unmerged_loss`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ipc/worktree.rs:332:10` — called `Result::unwrap()` on an `Err` value: GitError { command: "git clone --no-hardlinks --no-checkout /Users/I552267/ferryx-pane-completion/source-21dea3c0 repo", stderr: "fatal: repository '/Users/I552267/ferryx-pane-completion/source-21dea3c0' does not exist", stdout: "", code: Some(128) }

### `ipc::worktree::deletion_repair_tests::deletion_repair_success_removes_cached_row_and_blocks_stale_worker`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ipc/worktree.rs:332:10` — called `Result::unwrap()` on an `Err` value: GitError { command: "git clone --no-hardlinks --no-checkout /Users/I552267/ferryx-pane-completion/source-21dea3c0 repo", stderr: "fatal: repository '/Users/I552267/ferryx-pane-completion/source-21dea3c0' does not exist", stdout: "", code: Some(128) }

### `native_terminal::renderer::font_manager::tests::test_variable_font_korean_weight_instantiation`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/native_terminal/renderer/font_manager.rs:568:9` — Korean '실' ink density (0.1361) must be in the Regular band (> 0.15)

### `remote::server::machine_input_cancellation_tests::input_is_cancelled_when_full_queue_precedes_close`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/remote/../../tests/support/machine_input_fixture.rs:174:14` — called `Result::unwrap()` on an `Err` value: Elapsed(())

### `remote::server::machine_input_cancellation_tests::input_is_cancelled_when_generation_is_replaced`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/remote/../../tests/support/machine_input_fixture.rs:174:14` — called `Result::unwrap()` on an `Err` value: Elapsed(())

### `remote::server::machine_input_cancellation_tests::input_is_cancelled_when_grant_is_revoked`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/remote/../../tests/support/machine_input_fixture.rs:174:14` — called `Result::unwrap()` on an `Err` value: Elapsed(())

### `remote::server::machine_input_cancellation_tests::input_is_cancelled_when_socket_disconnects`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/remote/../../tests/support/machine_input_fixture.rs:174:14` — called `Result::unwrap()` on an `Err` value: Elapsed(())

### `remote::server::machine_input_cancellation_tests::sibling_pty_is_responsive_when_first_input_is_saturated`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/remote/../../tests/support/machine_input_fixture.rs:174:14` — called `Result::unwrap()` on an `Err` value: Elapsed(())

### `remote::server::tests::test_machine_terminal_input_throttled_under_saturation_preserves_order`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/remote/server.rs:7234:14` — called `Result::unwrap()` on an `Err` value: Elapsed(())

### `remote::tests::security::sockets::a10_pending_machine_input_is_dropped_on_disconnect_and_revoke`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/remote/../../tests/support/machine_input_fixture.rs:174:14` — called `Result::unwrap()` on an `Err` value: Elapsed(())

### `remote::workspace_api::worktrees::authority_tests::typed_owner_repair_private_uds_and_native_adapter`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/remote/workspace_api/worktree_authority_tests.rs:635:18` — called `Result::unwrap()` on an `Err` value: Elapsed(())

### `rollout_tests::a24_rollback_waits_for_drain_and_preserves_unrelated_owner`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/rollout_tests.rs:12:64` — isolated TMPDIR

### `ssh::bridge::tests::ssh_bridge_child_exit_on_cancellation_with_event_barrier`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:1478:5` — child process must terminate on cancelled close and drop

### `ssh::bridge::tests::ssh_bridge_child_exit_on_drop_with_event_barrier`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:1438:5` — child process must terminate and close pipe on drop

### `ssh::bridge::tests::ssh_bridge_dag_frame_ordering_requires_contiguity_or_resync`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /Users/I552267/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_dag_subscription_drop_releases_helper_and_keeps_pty_alive`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /Users/I552267/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_dag_subscription_rejects_unregistered_project`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /Users/I552267/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_dag_subscription_released_after_eof_during_blocked_next`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /Users/I552267/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_dag_subscription_streams_inventory_updates_and_cancels`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /Users/I552267/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_handshake_verifies_host_and_epoch`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /Users/I552267/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_imported_connection_has_no_kill_rights`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:1523:5` — spawner cleanup must end the child and close its stdout pipe

### `ssh::bridge::tests::ssh_bridge_independent_read_does_not_block_control`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /Users/I552267/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_lifecycle_poison_on_timeout_cancel_and_eof_reaping`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /Users/I552267/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_live_loopback_openssh_connection`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:759:5` — Loopback SSH must be available for live OpenSSH bridge test

### `ssh::bridge::tests::ssh_bridge_ownership_ordering_detach_rollback_commit`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:1583:9` — spawner cleanup must end the child and close its stdout pipe

### `ssh::bridge::tests::ssh_bridge_real_pty_large_session_writes_do_not_block_other_session`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /Users/I552267/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_setup_failure_ends_the_child`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:1758:5` — setup failure must end the child and close its stdout pipe

### `ssh::bridge::tests::ssh_bridge_single_attempt_write_does_not_queue`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /Users/I552267/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_spawn_retry_requires_matching_client_request_id`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /Users/I552267/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_supervision_attach_failure_leaves_no_supervisor`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:1875:37` — spawn supervisor: Os { code: 2, kind: NotFound, message: "No such file or directory" }

### `ssh::bridge::tests::ssh_bridge_target_expired_fails_explicitly_never_spawns`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /Users/I552267/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_target_mismatch_on_describe_or_read_is_rejected`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /Users/I552267/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::test_ssh_bridge_dag_inventory_and_poll`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /Users/I552267/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::helper_setup::tests::posix_upload_script_never_kills_live_daemon_and_defers_upgrade`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/helper_setup_tests.rs:701:6` — called `Result::unwrap()` on an `Err` value: Os { code: 2, kind: NotFound, message: "No such file or directory" }

## Method notes for this file

- The base run was executed with **two tests explicitly skipped**, because both hung indefinitely at
  base and had to be SIGTERM'd (they are NOT excluded from the candidate run):
  - `ipc::tests::test_p11_start_cleanup_reaper_schedules_background_resolution` (hung after 1035 tests;
    log kept as `full-lib-base-HUNG.log`)
  - `remote::machine_operation_journal::contention_tests::journal_mutation_deadline_retains_admission_until_worker_drains`
    (hung after 1707 tests; log kept as `full-lib-base-HUNG2.log`)
  The base result line therefore reads `2 filtered out`. Both tests **pass at the candidate** on mac
  and linux, so they are not candidate-caused; they are reported as NOT_RUN at base and as findings.
- Both sides were run with `--test-threads=1` on the same host and the same command, so each
  `pre-existing` row above is backed by the base run failing the identical test name.
- `candidate-caused` means: the test name is present in the base run, base **passed** it, and the
  candidate **fails** it.
## Determinism caveat for the mac rows

The mac comparison above is a **single run per side**. The identical six `candidate-caused` tests were
re-verified deterministically on linux with two repetitions per side
(`linux/ab-classify2.log`, `linux/logs/ab/`), and the mac panic sites and messages match linux
one-for-one. The mac rows are therefore reported as candidate-caused with the linux repetition as the
determinism evidence; mac itself was not repeated, and the same caveat applies to the mac
`pre-existing` rows — a flaky test could in principle be mislabelled in a single run, which is exactly
what happened on linux for `input_is_cancelled_when_grant_is_revoked`.
