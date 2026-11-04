$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-20261104'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win"
$env:PATH="$root\src-tauri\resources\ffmpeg\sdk\bin;$env:PATH"
$test='C:\Users\sook\ferryx-wt\.target\debug\deps\ferryx_lib-a3aa23b3b5f5f1b3.exe'
& $test --nocapture ipc::debug::pane_liveness_diagnostics_tests::pane_liveness_diagnostics_entry_size_bound daemon::session_service::machine_tests::desktop_inventory_isolated_parent *> "$logs\baseline-last-two.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\baseline-last-two.log"
