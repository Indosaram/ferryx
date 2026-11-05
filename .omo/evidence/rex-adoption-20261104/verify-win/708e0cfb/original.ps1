$root='C:\Users\sook\ferryx-wt\rex-verify-win-d8313422b'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win\708e0cfb"
$env:PATH="$root\src-tauri\resources\ffmpeg\sdk\bin;$env:PATH"
& C:\Users\sook\ferryx-wt\.target\debug\deps\ferryx_lib-343f9c021bb9f724.exe --nocapture daemon::layout:: daemon::layout_migration:: daemon::handover::spawn_owner_tests:: daemon::agent_extension::p09_tests:: ipc::debug::pane_liveness_diagnostics_tests:: daemon::supervisor_peer::tests::verify_win_peer_11476_exited_ssh_ancestor_932_owner_and_pane_cases native_terminal::osc_effects::tests::osc_expectation_table *> "$logs\original-failures.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\original-failures.log"
