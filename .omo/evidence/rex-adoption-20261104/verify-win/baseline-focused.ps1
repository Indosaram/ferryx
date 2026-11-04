$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-20261104'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win"
$env:PATH="$root\src-tauri\resources\ffmpeg\sdk\bin;$env:PATH"
# The full baseline stopped producing output at paired_host for >15 minutes.
# Stop only the recorded baseline runner, not any daemon.
Get-Process -Id 5996 -ErrorAction SilentlyContinue | Stop-Process
'BASELINE_RUNNER_PID=5996 STOPPED_AFTER_NO_PROGRESS; full baseline incomplete' | Out-File "$logs\baseline-timeout.log"
$failed=Get-Content "$logs\lib-test-corrected.log" | Where-Object {$_ -match '^test (.+) \.\.\. FAILED$'} | ForEach-Object { [regex]::Match($_,'^test (.+) \.\.\. FAILED$').Groups[1].Value }
$test='C:\Users\sook\ferryx-wt\.target\debug\deps\ferryx_lib-a3aa23b3b5f5f1b3.exe'
& $test --nocapture @failed *> "$logs\baseline-matching-failures.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\baseline-matching-failures.log"
