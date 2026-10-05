# Full `--lib` delta — linux: `abd9e890` → `39e722ce`

Command (both): `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`

| | Result line |
| --- | --- |
| `abd9e890` | `test result: FAILED. 2629 passed; 37 failed; 6 ignored; 0 measured; 0 filtered out; finished in 451.09s` |
| `39e722ce` | `test result: FAILED. 2638 passed; 31 failed; 6 ignored; 0 measured; 0 filtered out; finished in 489.97s` |

| Count | Value |
| --- | --- |
| failing at `abd9e890` | 37 |
| failing at `39e722ce` | 31 |
| **left the failing set** | **7** |
| **joined the failing set** | **1** |
| still failing | 30 |

## Left the failing set (7)

- `ipc::tests::tauri_mock_terminal_attach_returns_base64_history_and_decimal_sequences`
- `ipc::tests::test_p13_attach_routes_through_descriptor_and_reinstalls_proxy`
- `native_terminal::surface_host::tests::bounds_ipc_presents_when_browser_child_is_open`
- `native_terminal::surface_host::tests::deferred_bounds_retry_does_not_restore_obsolete_width`
- `native_terminal::surface_host::tests::synchronized_output_bounds_ipc_finishes_on_detach_or_stream_end`
- `native_terminal::surface_host::tests::synchronized_output_bounds_ipc_waits_for_actual_presentation`
- `remote::server::machine_input_cancellation_tests::input_is_cancelled_when_grant_is_revoked`

## Joined the failing set (1) — each is a NEW finding

- `terminal::tests::a_wedged_pty_does_not_block_the_async_runtime_thread`

## Still failing (30)

- `daemon::handover_wire::tests::truncated_control_is_rejected`
- `daemon::server::a03_owner_cli_fixture::a03_private_owner_cli_surface`
- `daemon::server::remote_ssh_tests::direct_ssh_real_transport_registration_and_pty`
- `daemon::server::remote_ssh_tests::remote_ssh_first_spawn_provisions_qualified_helper_automatically`
- `ipc::worktree::deletion_repair_tests::deletion_repair_missing_record_preview_and_targeted_cleanup`
- `ipc::worktree::deletion_repair_tests::deletion_repair_preview_reports_current_dirty_and_unmerged_loss`
- `ipc::worktree::deletion_repair_tests::deletion_repair_success_removes_cached_row_and_blocks_stale_worker`
- `native_terminal::renderer::font_manager::tests::test_font_manager_derives_nonzero_metrics_and_rasterizes`
- `native_terminal::renderer::font_manager::tests::test_glyph_orientation_regression`
- `remote::relay_server::tests::a_symlinked_artifact_is_not_served`
- `remote::server::machine_input_cancellation_tests::input_is_cancelled_when_socket_disconnects`
- `remote::tests::security::sockets::a10_pending_machine_input_is_dropped_on_disconnect_and_revoke`
- `rollout_tests::a24_rollback_waits_for_drain_and_preserves_unrelated_owner`
- `ssh::bridge::tests::ssh_bridge_dag_frame_ordering_requires_contiguity_or_resync`
- `ssh::bridge::tests::ssh_bridge_dag_subscription_drop_releases_helper_and_keeps_pty_alive`
- `ssh::bridge::tests::ssh_bridge_dag_subscription_rejects_unregistered_project`
- `ssh::bridge::tests::ssh_bridge_dag_subscription_released_after_eof_during_blocked_next`
- `ssh::bridge::tests::ssh_bridge_dag_subscription_streams_inventory_updates_and_cancels`
- `ssh::bridge::tests::ssh_bridge_handshake_verifies_host_and_epoch`
- `ssh::bridge::tests::ssh_bridge_independent_read_does_not_block_control`
- `ssh::bridge::tests::ssh_bridge_lifecycle_poison_on_timeout_cancel_and_eof_reaping`
- `ssh::bridge::tests::ssh_bridge_live_loopback_openssh_connection`
- `ssh::bridge::tests::ssh_bridge_ownership_ordering_detach_rollback_commit`
- `ssh::bridge::tests::ssh_bridge_real_pty_large_session_writes_do_not_block_other_session`
- `ssh::bridge::tests::ssh_bridge_single_attempt_write_does_not_queue`
- `ssh::bridge::tests::ssh_bridge_spawn_retry_requires_matching_client_request_id`
- `ssh::bridge::tests::ssh_bridge_target_expired_fails_explicitly_never_spawns`
- `ssh::bridge::tests::ssh_bridge_target_mismatch_on_describe_or_read_is_rejected`
- `ssh::bridge::tests::test_ssh_bridge_dag_inventory_and_poll`
- `ssh::helper_setup::tests::posix_upload_script_never_kills_live_daemon_and_defers_upgrade`
