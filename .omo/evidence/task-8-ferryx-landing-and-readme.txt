=== SITE BUILD ===
$ tsc && vite build
vite v6.4.3 building for production...
transforming...
✓ 1602 modules transformed.
rendering chunks...
computing gzip size...
dist/index.html                   1.53 kB │ gzip:  0.74 kB
dist/assets/index-BXNmhpb7.css   26.64 kB │ gzip:  5.17 kB
dist/assets/index-DU6ANgtQ.js   207.93 kB │ gzip: 63.76 kB │ map: 578.98 kB
✓ built in 1.12s
=== UI TEST ===
$ vitest run --maxWorkers=1

 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/components/SettingsDialog.test.tsx (14 tests) 514ms
 ✓ src/components/Sidebar.test.tsx (16 tests) 280ms
 ✓ src/App.test.tsx (17 tests) 295ms
 ✓ src/components/TabBar.test.tsx (10 tests) 187ms
 ✓ src/state/workspaceRuntime.test.tsx (3 tests) 169ms
 ✓ src/components/TerminalSplitView.test.tsx (10 tests) 112ms
 ✓ src/components/ProjectDialogs.test.tsx (10 tests) 103ms
 ✓ src/components/WorktreeDeleteDialog.test.tsx (4 tests) 87ms
 ✓ src/lib/terminalHostManager.test.ts (4 tests) 81ms
 ✓ src/remote/RemoteUI.test.tsx (7 tests) 58ms
 ✓ src/components/Sidebar.activity.test.tsx (2 tests) 72ms
 ✓ src/components/TerminalSplitView.pointerDrag.test.tsx (4 tests) 80ms
 ✓ src/components/WorktreeList.test.tsx (7 tests) 75ms
 ✓ src/components/CommandPalette.test.tsx (2 tests) 73ms
 ✓ src/state/projectWorkspaceScope.test.tsx (2 tests) 63ms
 ✓ src/components/TerminalSearchOverlay.test.tsx (4 tests) 56ms
 ✓ src/components/WorkspaceHeader.test.tsx (4 tests) 62ms
 ✓ src/components/BrowserToolbar.test.tsx (6 tests) 50ms
 ✓ src/lib/browserTauri.test.ts (1 test) 52ms
 ✓ src/lib/shortcuts.test.tsx (27 tests) 37ms
 ✓ src/components/NewTabPopover.test.tsx (4 tests) 32ms
 ✓ src/state/workspaceStore.test.tsx (18 tests) 31ms
 ✓ src/components/TerminalPane.test.tsx (1 test) 27ms
 ✓ src/components/TerminalSplitView.tabDropFallback.test.tsx (1 test) 24ms
 ✓ src/lib/terminalRenderer.test.ts (3 tests) 18ms
 ✓ src/components/BrowserPane.test.tsx (2 tests) 15ms
 ✓ src/state/workspaceActivity.test.tsx (3 tests) 12ms
 ✓ src/state/workspaceStore.tabDrop.test.tsx (2 tests) 11ms
 ✓ src/state/workspaceStore.browserLifecycle.test.tsx (1 test) 8ms
 ✓ src/lib/terminalSettings.test.tsx (3 tests) 9ms
 ✓ src/index-html.test.ts (1 test) 2ms
 ✓ src/state/paneTree.test.ts (48 tests) 5ms
 ✓ src/lib/agentTitle.test.ts (15 tests) 5ms
 ✓ src/state/layout.test.ts (11 tests) 4ms
 ✓ src/lib/tauri.test.ts (10 tests) 4ms
 ✓ src/lib/sessionPersistence.test.ts (5 tests) 4ms
 ✓ src/manifest.test.ts (3 tests) 3ms
 ✓ src/lib/terminalEvents.test.ts (4 tests) 2ms
 ✓ src/lib/terminalTransport/terminalTransport.test.ts (3 tests) 2ms
 ✓ src/lib/storageKeys.test.ts (4 tests) 2ms
 ✓ src/lib/terminalEvents.bus.test.ts (2 tests) 2ms
 ✓ src/lib/activity.test.ts (3 tests) 2ms
 ✓ src/assets/ferryx-icon.test.ts (1 test) 2ms
 ✓ src/lib/terminalOutput.test.ts (2 tests) 2ms
 ✓ src/lib/ipcErrors.test.ts (2 tests) 1ms
 ✓ src/test/viteHmrConfig.test.ts (1 test) 67ms

 Test Files  46 passed (46)
      Tests  307 passed (307)
   Start at  10:12:03
   Duration  25.22s (transform 468ms, setup 2.78s, collect 6.61s, tests 2.80s, environment 7.58s, prepare 1.27s)

=== RUST TEST ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.84s
     Running unittests src/lib.rs (/Users/indo/code/project/orca-lite/src-tauri/target/debug/deps/ferryx_lib-d3b00f8cfc2e62f0)

running 119 tests
test ipc::notifications::tests::picker_filters_only_offer_decodable_formats ... ok
test ipc::notifications::tests::permission_status_round_trips_across_the_ipc_boundary ... ok
test ipc::notifications::tests::probe_result_round_trips_across_the_ipc_boundary ... ok
test browser::tests::test_validate_url ... ok
test ipc::notifications::tests::dispatch_result_round_trips_across_the_ipc_boundary ... ok
test ipc::notifications::tests::permission_request_result_round_trips_across_the_ipc_boundary ... ok
test notification::audio::tests::picked_file_exposes_only_path_and_display_name ... ok
test notification::audio::tests::missing_file_reports_not_found_without_panicking ... ok
test ipc::notifications::tests::picked_audio_result_round_trips_including_cancellation ... ok
test ipc::notifications::tests::dispatch_command_payload_matches_the_frontend_contract ... ok
test browser::tests::test_browser_manager_lifecycle ... ok
test notification::service::tests::authorized_status_submits_exactly_one_notification ... ok
test notification::service::tests::backend_failure_is_reported_as_a_structured_reason ... ok
test notification::audio::tests::unsupported_extension_is_rejected_before_decoding ... ok
test notification::service::tests::denied_macos_status_blocks_before_submitting ... ok
test notification::audio::tests::rapid_automatic_sounds_are_deduped_but_force_still_plays ... ok
test notification::audio::tests::sound_outside_the_dedupe_window_plays_again ... ok
test notification::audio::tests::zero_volume_short_circuits_before_touching_the_audio_device ... ok
test notification::service::tests::dispatched_content_is_sanitized_before_reaching_the_backend ... ok
test notification::audio::tests::negative_and_oversized_volumes_clamp_into_range ... ok
test notification::service::tests::non_authoritative_denied_does_not_block ... ok
test notification::service::tests::non_authoritative_unknown_status_still_dispatches ... ok
test notification::service::tests::permission_status_passes_through_the_provider_value ... ok
test notification::service::tests::not_determined_returns_permission_required_without_implicit_prompt ... ok
test notification::service::tests::preflight_decisions_cover_every_authorization_state ... ok
test notification::service::tests::explicit_permission_request_reaches_the_provider_once ... ok
test notification::service::tests::probe_reports_blocked_without_sending_a_test ... ok
test notification::service::tests::probe_failure_reports_failed_not_submitted ... ok
test notification::service::tests::probe_reports_permission_required_without_prompting ... ok
test notification::service::tests::probe_with_test_submits_exactly_one_test_notification ... ok
test notification::service::tests::probe_without_test_reports_ready_and_sends_nothing ... ok
test notification::service::tests::unsupported_platform_rejects_dispatch ... ok
test notification::service::tests::provisional_authorization_is_allowed_to_dispatch ... ok
test notification::tests::agent_completion_falls_back_to_workspace_label ... ok
test notification::tests::agent_completion_falls_back_when_labels_are_missing ... ok
test ipc::blocking_contract_tests::blocking_ipc_helper_runs_operation_off_async_caller_thread ... ok
test notification::tests::agent_completion_uses_agent_name_and_worktree_label ... ok
test notification::tests::current_platform_is_detected ... ok
test ipc::notifications::tests::open_system_settings_command_returns_a_structured_result ... ok
test notification::tests::dispatch_reasons_serialize_as_kebab_case ... ok
test notification::tests::dispatch_request_deserializes_from_camel_case_payload ... ok
test notification::tests::dispatch_request_tolerates_a_minimal_payload ... ok
test notification::tests::display_name_uses_the_file_name ... ok
test notification::tests::formatted_content_is_sanitized_against_injected_control_characters ... ok
test notification::tests::formatted_content_never_exceeds_length_bounds ... ok
test notification::tests::nan_volume_is_silent_rather_than_undefined ... ok
test notification::tests::notification_source_uses_camel_case_wire_values ... ok
test notification::tests::only_authoritative_denied_status_counts_as_blocked ... ok
test notification::tests::only_authoritative_not_determined_requires_a_permission_request ... ok
test notification::tests::open_system_settings_result_serializes_for_the_frontend ... ok
test notification::tests::out_of_range_volume_is_clamped ... ok
test notification::tests::permission_status_dto_matches_the_documented_wire_shape ... ok
test notification::tests::permission_request_dto_serializes_with_nested_status ... ok
test notification::tests::picked_audio_file_serializes_path_and_display_name ... ok
test notification::tests::play_sound_result_serializes_kebab_case_reasons ... ok
test notification::tests::probe_result_serializes_outcome_and_submission_flag ... ok
test notification::tests::rejected_dispatch_result_carries_its_reason ... ok
test notification::tests::sanitize_collapses_newlines_and_pathological_whitespace ... ok
test notification::tests::sanitize_of_only_control_characters_is_empty ... ok
test notification::tests::sanitize_preserves_ordinary_unicode_text ... ok
test notification::tests::sanitize_removes_unicode_line_separators_and_bom ... ok
test notification::tests::successful_dispatch_result_omits_the_reason_field ... ok
test notification::tests::supported_extensions_are_recognized_case_insensitively ... ok
test notification::tests::terminal_bell_joins_location_and_terminal_title ... ok
test notification::tests::terminal_bell_without_any_label_still_has_a_body ... ok
test notification::tests::sanitize_strips_control_characters_and_escape_sequences ... ok
test notification::tests::test_notification_has_fixed_content ... ok
test notification::tests::truncate_appends_ellipsis_and_respects_the_limit ... ok
test notification::tests::truncate_leaves_short_text_untouched ... ok
test notification::tests::truncate_splits_on_character_boundaries_not_bytes ... ok
test notification::tests::unsupported_and_extensionless_paths_are_rejected ... ok
test notification::tests::unsupported_status_reports_no_capabilities ... ok
test notification::tests::validate_rejects_a_missing_file ... ok
test notification::audio::tests::corrupt_audio_reports_decode_failure_not_a_crash ... ok
test notification::tests::validate_rejects_unsupported_format_before_touching_the_filesystem ... ok
test notification::tests::volume_percentage_maps_to_linear_gain ... ok
test notification::tests::whitespace_only_labels_are_treated_as_absent ... ok
test remote::tests::test_tailscale_status_parsing ... ok
test notification::tests::validate_rejects_files_over_the_size_bound ... ok
test notification::tests::validate_accepts_a_file_at_the_size_bound_and_returns_a_canonical_path ... ok
test notification::tests::validate_rejects_a_directory_masquerading_as_audio ... ok
test terminal::output_hub::tests::test_bounded_buffer_overflow ... ok
test terminal::output_hub::tests::test_output_hub_replay_and_broadcast ... ok
test terminal::preferences::tests::test_empty_font_family_resets_list ... ok
test notification::tests::validate_resolves_indirect_paths_to_a_canonical_location ... ok
test remote::tests::test_auth_manager_pairing_and_revocation ... ok
test session::tests::test_corrupted_session_recovery ... ok
test terminal::preferences::tests::test_palette_hex_and_256_colors ... ok
test ipc::notifications::tests::play_sound_command_reports_missing_file_without_erroring ... ok
test ipc::notifications::tests::play_sound_command_tolerates_omitted_optional_arguments ... ok
test terminal::tests::close_session_is_idempotent ... ok
test terminal::tests::explicit_close_kills_reaps_and_removes_session ... ok
test terminal::tests::test_kill ... ok
test terminal::tests::test_multiple_concurrent_sessions ... ok
test remote::tests::test_remote_server_health_and_lifecycle ... ok
test remote::tests::test_remote_server_serves_spa_index_html ... ok
test session::tests::test_session_save_load_clear_cycle ... ok
test worktree::tests::branch_namespace_formatting_and_validation ... ok
test ipc::notifications::tests::play_sound_command_rejects_unsupported_formats ... ok
test worktree::tests::parse_status_porcelain_v1_and_v2 ... ok
test worktree::tests::parse_worktree_list_porcelain ... ok
test terminal::tests::test_resize ... ok
test terminal::tests::test_session_lifecycle_and_errors ... ok
test terminal::tests::test_spawn_write_echo_and_read ... ok
test terminal::tests::dropped_output_receiver_still_cleans_reader_and_session ... ok
test terminal::tests::interrupt_signal_targets_foreground_pty_process_group ... ok
test terminal::tests::fast_spawn_close_race_is_safe ... ok
test terminal::tests::natural_child_exit_auto_removes_session_and_records_exit_code ... ok
test ipc::tests::tauri_mock_terminal_events_use_registered_workspace ... ok
test worktree::tests::existing_target_and_missing_status_are_rejected ... ok
test worktree::tests::worktree_manager_resolves_nested_git_directory_to_canonical_root ... ok
test worktree::tests::registry_resolves_only_registered_workspace_identity ... ok
test worktree::tests::create_find_and_list_worktree ... ok
test worktree::tests::dirty_worktree_is_never_deleted_even_with_force_remove ... ok
test worktree::tests::safe_delete_clean_worktree ... ok
test ipc::tests::tauri_mock_worktree_commands_use_identity_contract ... ok
test worktree::tests::safe_branch_delete_removes_merged_branch ... ok
test worktree::tests::multiple_worktrees_remain_isolated ... ok
test ipc::tests::terminal_global_events_preserve_raw_bytes_and_lifecycle ... ok

test result: ok. 119 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.72s

     Running unittests src/main.rs (/Users/indo/code/project/orca-lite/src-tauri/target/debug/deps/ferryx-49b119df1258e901)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/backend_hardening.rs (/Users/indo/code/project/orca-lite/src-tauri/target/debug/deps/backend_hardening-7f23cc452ae4c83e)

running 8 tests
test leading_dash_namespace_component_is_rejected ... ok
test serialized_dirty_state_uses_explicit_camel_case_fields ... ok
test ipc_request_contract_rejects_raw_paths_and_shell_commands ... ok
test csp_is_active_and_restrictive ... ok
test git_error_command_log_escapes_control_characters ... ok
test worktree_manager_validates_repo_root_during_construction ... ok
test worktree_creation_rejects_parent_traversal_outside_registered_root ... ok
test worktree_creation_rejects_symlink_escape ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s

     Running tests/daemon_persistence_contract.rs (/Users/indo/code/project/orca-lite/src-tauri/target/debug/deps/daemon_persistence_contract-11f52542ae6a2690)

running 2 tests
test test_durable_fsync_session_persistence_lifecycle ... ok
test test_daemon_uds_handshake_and_ping ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s

     Running tests/e2e_agent_workflow.rs (/Users/indo/code/project/orca-lite/src-tauri/target/debug/deps/e2e_agent_workflow-bc58f58595adb896)

running 1 test
test test_e2e_agent_worktree_and_terminal_lifecycle ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.38s

     Running tests/ipc_hardening_contract.rs (/Users/indo/code/project/orca-lite/src-tauri/target/debug/deps/ipc_hardening_contract-2a9e1807900c5ad6)

running 6 tests
test terminal_lifecycle_state_serializes_stably ... ok
test dirty_delete_returns_structured_error_code ... ok
test worktree_status_emits_dirty_changed_on_clean_to_dirty_transition ... ok
test worktree_status_emits_dirty_changed_on_dirty_to_clean_transition ... ok
test identity_based_ipc_resolves_registered_worktree_and_emits_mutation_events ... ok
test worktree_status_does_not_emit_when_dirty_state_is_unchanged ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.52s

     Running tests/rorca_native_contract.rs (/Users/indo/code/project/orca-lite/src-tauri/target/debug/deps/rorca_native_contract-d13c14077571610e)

running 9 tests
test tauri_metadata_uses_rorca_identity_and_generated_icons ... ok
test main_window_has_explicit_titlebar_drag_permission ... ok
test ghostty_parser_handles_quotes_and_macos_option_keywords ... ok
test ghostty_parser_combines_font_families_and_reads_macos_option_as_alt ... ok
test terminal_preferences_use_safe_absent_and_malformed_defaults ... ok
test loads_real_ghostty_config_from_system ... ok
test project_registration_rejects_non_git_roots_and_unregistered_branch_queries ... ok
test project_registration_is_idempotent_for_the_same_workspace_and_root ... ok
test project_registration_returns_canonical_root_and_lists_local_branches ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.22s

     Running tests/session_persistence_integration.rs (/Users/indo/code/project/orca-lite/src-tauri/target/debug/deps/session_persistence_integration-2a840d9685889b01)

running 1 test
test test_session_lifecycle_integration ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running tests/worktree_safety.rs (/Users/indo/code/project/orca-lite/src-tauri/target/debug/deps/worktree_safety-8a795f30e07647fd)

running 7 tests
test second_writer_is_rejected_and_release_allows_reacquire ... ok
test active_writer_blocks_delete ... ok
test pty_writer_lease_releases_on_close_and_natural_exit ... ok
test default_delete_rejects_clean_unmerged_branch ... ok
test explicit_destructive_delete_removes_clean_unmerged_branch ... ok
test pty_spawn_failure_rolls_back_writer_lease ... ok
test five_concurrent_worktree_terminal_lifecycles_release_all_leases ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.14s

   Doc-tests ferryx_lib

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

