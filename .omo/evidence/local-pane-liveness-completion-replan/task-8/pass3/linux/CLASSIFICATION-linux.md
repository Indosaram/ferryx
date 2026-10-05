# Full `--lib` failure classification — linux

Candidate: `abd9e890`  Base: `d82b35e4`
Command (both sides): `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`

- Candidate result: test result: FAILED. 2629 passed; 37 failed; 6 ignored; 0 measured; 0 filtered out; finished in 451.09s
- Base result: test result: FAILED. 2554 passed; 31 failed; 6 ignored; 0 measured; 1 filtered out; finished in 340.18s
- Base tests observed: 2582; candidate tests observed: 2663

| Classification | Count |
| --- | --- |
| candidate-caused | 6 |
| candidate-authored-test | 0 |
| pre-existing | 30 |
| flaky (A/B nondeterministic) | 1 |
| unclassified | 0 |
| **total** | **37** |

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

### `daemon::handover_wire::tests::truncated_control_is_rejected`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/daemon/handover_wire.rs:514:10` — must detect truncation: ()

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

### `ipc::worktree::deletion_repair_tests::deletion_repair_missing_record_preview_and_targeted_cleanup`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ipc/worktree.rs:332:10` — called `Result::unwrap()` on an `Err` value: GitError { command: "git clone --no-hardlinks --no-checkout /home/indo/ferryx-pane-completion/source-21dea3c0 repo", stderr: "fatal: repository '/home/indo/ferryx-pane-completion/source-21dea3c0' does not exist", stdout: "", code: Some(128) }

### `ipc::worktree::deletion_repair_tests::deletion_repair_preview_reports_current_dirty_and_unmerged_loss`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ipc/worktree.rs:332:10` — called `Result::unwrap()` on an `Err` value: GitError { command: "git clone --no-hardlinks --no-checkout /home/indo/ferryx-pane-completion/source-21dea3c0 repo", stderr: "fatal: repository '/home/indo/ferryx-pane-completion/source-21dea3c0' does not exist", stdout: "", code: Some(128) }

### `ipc::worktree::deletion_repair_tests::deletion_repair_success_removes_cached_row_and_blocks_stale_worker`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ipc/worktree.rs:332:10` — called `Result::unwrap()` on an `Err` value: GitError { command: "git clone --no-hardlinks --no-checkout /home/indo/ferryx-pane-completion/source-21dea3c0 repo", stderr: "fatal: repository '/home/indo/ferryx-pane-completion/source-21dea3c0' does not exist", stdout: "", code: Some(128) }

### `native_terminal::renderer::font_manager::tests::test_font_manager_derives_nonzero_metrics_and_rasterizes`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/native_terminal/renderer/font_manager.rs:451:9` — assertion `left == right` failed

### `native_terminal::renderer::font_manager::tests::test_glyph_orientation_regression`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/native_terminal/renderer/font_manager.rs:655:9` — 'P' top-half ink (1996) must be strictly greater than bottom-half ink (2221)

### `remote::relay_server::tests::a_symlinked_artifact_is_not_served`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/remote/relay_server.rs:6800:9` — assertion `left != right` failed: symlink artifact must not return 200 OK

### `remote::server::machine_input_cancellation_tests::input_is_cancelled_when_socket_disconnects`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/remote/../../tests/support/machine_input_cancellation.rs:31:14` — real input future must become pending: Elapsed(())

### `remote::tests::security::sockets::a10_pending_machine_input_is_dropped_on_disconnect_and_revoke`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/remote/../../tests/support/machine_input_cancellation.rs:31:14` — real input future must become pending: Elapsed(())

### `rollout_tests::a24_rollback_waits_for_drain_and_preserves_unrelated_owner`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/rollout_tests.rs:12:64` — isolated TMPDIR

### `ssh::bridge::tests::ssh_bridge_dag_frame_ordering_requires_contiguity_or_resync`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /home/indo/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_dag_subscription_drop_releases_helper_and_keeps_pty_alive`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /home/indo/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_dag_subscription_rejects_unregistered_project`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /home/indo/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_dag_subscription_released_after_eof_during_blocked_next`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /home/indo/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_dag_subscription_streams_inventory_updates_and_cancels`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /home/indo/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_handshake_verifies_host_and_epoch`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /home/indo/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_independent_read_does_not_block_control`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /home/indo/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_lifecycle_poison_on_timeout_cancel_and_eof_reaping`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /home/indo/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_live_loopback_openssh_connection`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /home/indo/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_ownership_ordering_detach_rollback_commit`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:1583:9` — spawner cleanup must end the child and close its stdout pipe

### `ssh::bridge::tests::ssh_bridge_real_pty_large_session_writes_do_not_block_other_session`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /home/indo/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_single_attempt_write_does_not_queue`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /home/indo/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_spawn_retry_requires_matching_client_request_id`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /home/indo/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_target_expired_fails_explicitly_never_spawns`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /home/indo/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::ssh_bridge_target_mismatch_on_describe_or_read_is_rejected`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /home/indo/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::bridge::tests::test_ssh_bridge_dag_inventory_and_poll`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/bridge_tests.rs:29:5` — ferryx-remote-helper debug binary must exist at /home/indo/ferryx-pane-completion/source-21dea3c0/src-tauri/../remote-helper/target/debug/ferryx-remote-helper

### `ssh::helper_setup::tests::posix_upload_script_never_kills_live_daemon_and_defers_upgrade`
- base d82b35e4: **FAILED**  |  candidate abd9e890: **FAILED**
- why: the same test name fails at base d82b35e4 on the same host with the same command
- panic: `src/ssh/helper_setup_tests.rs:701:6` — called `Result::unwrap()` on an `Err` value: Os { code: 2, kind: NotFound, message: "No such file or directory" }


## pre-existing (30)

## flaky (A/B nondeterministic) (1)

### `remote::server::machine_input_cancellation_tests::input_is_cancelled_when_grant_is_revoked`
- single-run comparison: base **passed**, candidate **FAILED**
- two-rep isolated A/B (same host, filter-scoped, `--test-threads=1`):
  ```
  AB side=base rep=1 native=0   test result: ok. 1 passed
  AB side=cand rep=1 native=101 test result: FAILED. 0 passed; 1 failed
  AB side=base rep=2 native=101 test result: FAILED. 0 passed; 1 failed
  AB side=cand rep=2 native=0   test result: ok. 1 passed
  ```
- panic: `tests/support/machine_input_cancellation.rs:31:14` — real input future must become pending: Elapsed(())
- **Verdict: NOT candidate-caused.** The test is nondeterministic on both sides of the A/B (it fails on
  base as well), so it cannot be attributed to the candidate. It is reported as a flaky test and is a
  separate finding for the owning lane, not a regression this candidate introduced.
- Evidence: `linux/ab-classify2.log`, `linux/logs/ab/base-2-*.log`, `linux/logs/ab/cand-1-*.log`.

## Not a candidate regression (base fails, candidate passes)

### `remote::server::machine_input_cancellation_tests::sibling_pty_is_responsive_when_first_input_is_saturated`
- base d82b35e4 (single full-lib run): **FAILED**; candidate abd9e890 (single full-lib run): **passed**
- two-rep isolated A/B: base [PASS, PASS], candidate [PASS, FAIL] — also nondeterministic, and it is
  **not** in the candidate's failure list, so it is recorded only as evidence that this family is flaky
  on this host and must not be used as an A/B oracle without repetition.

## Isolated A/B of every candidate-suspect test (deterministic results)

The full-lib A/B was repeated test-by-test with the filter scoped to one test, `--test-threads=1`, two
repetitions per side, on the same host:

| Test | base rep1 | base rep2 | cand rep1 | cand rep2 | Verdict |
| --- | --- | --- | --- | --- | --- |
| `ipc::tests::tauri_mock_terminal_attach_returns_base64_history_and_decimal_sequences` | PASS | PASS | FAIL | FAIL | candidate-caused |
| `ipc::tests::test_p13_attach_routes_through_descriptor_and_reinstalls_proxy` | PASS | PASS | FAIL | FAIL | candidate-caused |
| `native_terminal::surface_host::tests::bounds_ipc_presents_when_browser_child_is_open` | PASS | PASS | FAIL | FAIL | candidate-caused |
| `native_terminal::surface_host::tests::deferred_bounds_retry_does_not_restore_obsolete_width` | PASS | PASS | FAIL | FAIL | candidate-caused |
| `native_terminal::surface_host::tests::synchronized_output_bounds_ipc_finishes_on_detach_or_stream_end` | PASS | PASS | FAIL | FAIL | candidate-caused |
| `native_terminal::surface_host::tests::synchronized_output_bounds_ipc_waits_for_actual_presentation` | PASS | PASS | FAIL | FAIL | candidate-caused |
| `remote::server::machine_input_cancellation_tests::input_is_cancelled_when_grant_is_revoked` | PASS | FAIL | FAIL | PASS | flaky |
| `remote::server::machine_input_cancellation_tests::sibling_pty_is_responsive_when_first_input_is_saturated` | PASS | PASS | PASS | FAIL | flaky |

Evidence: `linux/ab-classify2.log` and the per-run logs under `linux/logs/ab/`.

## Method notes for this file

- The base run was executed with **one test explicitly skipped**, because it hung indefinitely at base
  and had to be SIGTERM'd: `ipc::tests::test_p11_start_cleanup_reaper_schedules_background_resolution`
  (hung after 1036 tests; log kept as `full-lib-base-HUNG.log`). That test **passes at the candidate**,
  so it is not candidate-caused; it is reported as NOT_RUN at base and as a finding.
- Both sides were run with `--test-threads=1` on the same host and the same command.
- The single-run comparison above labels `input_is_cancelled_when_grant_is_revoked` candidate-caused;
  the **repeated** isolated A/B in the section below moves it to flaky, and that refined verdict is
  the one to use. Every other `candidate-caused` row was confirmed deterministic over two repetitions.
