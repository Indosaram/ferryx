$root='C:\Users\sook\ferryx-wt\rex-verify-win-d8313422b'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win\d8313422b"
Get-Date -Format o | Out-File "$logs\full-lib-timeout.log"
Get-Process -Id 33368 | Format-List Id,CPU,StartTime | Out-File "$logs\full-lib-timeout.log" -Append
Get-Item "$logs\lib-test.log" | Format-List LastWriteTime,Length | Out-File "$logs\full-lib-timeout.log" -Append
'No output since 11:24:55, observed 11:52:07; >27 minutes without progress. Bounded full-suite gate TIMEOUT, not PASS. Stop only recorded candidate test runner PID=33368; do not retry full suite.' | Add-Content "$logs\full-lib-timeout.log"
Stop-Process -Id 33368
