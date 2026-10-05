$ErrorActionPreference='Continue'
$t9='C:\Users\sook\ferryx-pane-completion\task9-120bc965\evidence\split-happy'
Write-Output "=== result.json ==="
$rj = Get-ChildItem -Recurse -File $t9 -Filter result.json | Sort-Object LastWriteTime | Select-Object -Last 1
Get-Content $rj.FullName
Write-Output "=== actions.jsonl (action names in order) ==="
$a = Get-ChildItem -Recurse -File $t9 -Filter actions.jsonl | Sort-Object LastWriteTime | Select-Object -Last 1
Get-Content $a.FullName | ForEach-Object { ($_ | ConvertFrom-Json).action }
Write-Output "=== the click action, VERBATIM ==="
Get-Content $a.FullName | Where-Object { $_ -match 'click-split-affordance' }
Write-Output "READ6_DONE"
