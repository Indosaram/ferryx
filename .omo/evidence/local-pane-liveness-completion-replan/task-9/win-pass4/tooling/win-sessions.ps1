$ErrorActionPreference='Continue'
Write-Output "=== qwinsta (all sessions) ==="
& qwinsta 2>&1
Write-Output "=== query user ==="
& query user 2>&1
Write-Output "=== explorer.exe owners (an interactive desktop would have one) ==="
Get-CimInstance Win32_Process -Filter "Name='explorer.exe'" | Select-Object ProcessId,SessionId,CreationDate | Format-Table -AutoSize | Out-String
Write-Output "=== winlogon / dwm per session ==="
Get-CimInstance Win32_Process | Where-Object { $_.Name -in @('winlogon.exe','dwm.exe') } | Select-Object ProcessId,Name,SessionId | Format-Table -AutoSize | Out-String
Write-Output "=== my own session ==="
Write-Output ("MY_SESSION=" + (Get-Process -Id $PID).SessionId)
Write-Output "SESSIONS_DONE"
