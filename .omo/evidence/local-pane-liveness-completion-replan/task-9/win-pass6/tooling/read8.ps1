$ErrorActionPreference='Continue'
$t9='C:\Users\sook\ferryx-pane-completion\task9-120bc965'
$a = Get-ChildItem -Recurse -File (Join-Path $t9 'evidence\split-happy') -Filter actions.jsonl | Sort-Object LastWriteTime | Select-Object -Last 1
Write-Output "=== the two powershell actions + owned-window (VERBATIM, raw lines) ==="
Get-Content $a.FullName | Where-Object { $_ -match '"action":"(powershell|owned-window)"' }
Write-Output "=== advance script state ==="
Get-ChildItem (Join-Path $t9 'logs') -Filter 'scenario10-*' | Select-Object Name,Length,LastWriteTime | Format-Table -AutoSize | Out-String
Get-ChildItem (Join-Path $t9 'evidence') -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  " + $_.Name) }
Write-Output "READ8_DONE"
