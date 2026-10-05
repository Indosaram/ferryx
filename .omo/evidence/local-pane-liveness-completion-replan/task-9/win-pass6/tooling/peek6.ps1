$ErrorActionPreference='Continue'
$t9='C:\Users\sook\ferryx-pane-completion\task9-120bc965'
Write-Output ("NOW=" + (Get-Date).ToString('o'))
Get-ChildItem (Join-Path $t9 'logs') -ErrorAction SilentlyContinue | Select-Object Name,Length,LastWriteTime | Format-Table -AutoSize | Out-String
Write-Output "=== evidence ==="
Get-ChildItem -Recurse -File (Join-Path $t9 'evidence') -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  " + $_.FullName.Replace($t9,'') + " " + $_.Length) }
Write-Output "=== own processes ==="
Get-CimInstance Win32_Process | Where-Object { ($_.Name -in @('ferryx.exe','node.exe','cmd.exe')) -and (($_.ExecutablePath -like '*source-21dea3c0*') -or ($_.CommandLine -like '*task9-120bc965*') -or ($_.CommandLine -like '*run-scenario6*')) } | Select-Object ProcessId,Name,SessionId | Format-Table -AutoSize | Out-String
Write-Output "PEEK6_DONE"
