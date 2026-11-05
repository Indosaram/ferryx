$root='C:\Users\sook\ferryx-wt\rex-verify-win-d8313422b'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win\d8313422b"
$env:PATH="$root\src-tauri\resources\ffmpeg\sdk\bin;$env:PATH"
& C:\Users\sook\ferryx-wt\.target\debug\deps\ferryx_lib-343f9c021bb9f724.exe --exact daemon::supervisor_peer::tests::verify_win_peer_11476_exited_ssh_ancestor_932_owner_and_pane_cases --nocapture *> "$logs\owner-regression-focused.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\owner-regression-focused.log"
