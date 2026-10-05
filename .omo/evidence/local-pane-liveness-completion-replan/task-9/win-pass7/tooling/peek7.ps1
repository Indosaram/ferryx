$ErrorActionPreference='Continue'
$t9='C:\Users\sook\ferryx-pane-completion\task9-42fba06f'
Write-Output ("NOW=" + (Get-Date).ToString('o'))
Get-ChildItem (Join-Path $t9 'logs') -ErrorAction SilentlyContinue | Select-Object Name,Length,LastWriteTime | Format-Table -AutoSize | Out-String
Write-Output "=== key lines from any scenario11 log ==="
Get-ChildItem (Join-Path $t9 'logs') -Filter 'scenario11-*' -ErrorAction SilentlyContinue | ForEach-Object {
  Write-Output ("--- " + $_.Name)
  Get-Content $_.FullName | Select-String -Pattern 'VERDICT=|CODE=|GATE_OK=|HOLDERS=|NODE_HANG_OBSERVED|RAW_EXIT_CONTENT|ISO_EXISTS_AFTER|SCENARIO11_DONE' | ForEach-Object { Write-Output ("  " + $_.Line) }
}
Write-Output "PEEK7_DONE"
