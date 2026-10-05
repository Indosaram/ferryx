$ErrorActionPreference='Continue'
$t9='C:\Users\sook\ferryx-pane-completion\task9-48b4ed93'
Write-Output ("NOW=" + (Get-Date).ToString('o'))
Get-ChildItem (Join-Path $t9 'logs') -ErrorAction SilentlyContinue | Select-Object Name,Length,LastWriteTime | Format-Table -AutoSize | Out-String
Get-ChildItem (Join-Path $t9 'logs') -Filter 'scenario12-*' -ErrorAction SilentlyContinue | ForEach-Object {
  Write-Output ("--- " + $_.Name)
  Get-Content $_.FullName | Select-String -Pattern 'VERDICT=|CODE=|GATE_OK=|NODE_HANG|RAW_EXIT|PORT_5173|ISO_EXISTS_AFTER|SCENARIO12_DONE|ERROR_MESSAGE' | ForEach-Object { Write-Output ("  " + $_.Line) }
}
Write-Output ("PORT_5173_NOW=" + $(if (Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue) { 'OCCUPIED' } else { 'FREE' }))
Write-Output "PEEK8_DONE"
