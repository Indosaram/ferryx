$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-20261104'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win"
$env:PATH="$root\src-tauri\resources\ffmpeg\sdk\bin;$env:PATH"
$test='C:\Users\sook\ferryx-wt\.target\debug\deps\ferryx_lib-ea620c3a1cf0c11a.exe'
& $test --nocapture daemon::layout:: daemon::layout_migration:: daemon::handover::spawn_owner_tests:: *> "$logs\layout-focused.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\layout-focused.log"
& $test --nocapture ipc::updater:: *> "$logs\updater-focused.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\updater-focused.log"
& $test --nocapture daemon::windows_pipe daemon::server::control_server::control_wire_tests::windows_pipe *> "$logs\pipe-focused.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\pipe-focused.log"
