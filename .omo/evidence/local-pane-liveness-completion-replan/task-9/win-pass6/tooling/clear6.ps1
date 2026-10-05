$ErrorActionPreference='Continue'
# Evidence first: who holds split-happy's isolation root, before any kill.
$iso='C:\Users\sook\ferryx-pane-completion\task9-120bc965\runtime\split-happy'
Write-Output "=== HOLDER EVIDENCE (before kill) ==="
Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like ('*'+$iso+'*') -or $_.ProcessId -eq 22456 -or $_.ProcessId -eq 27816 -or $_.ProcessId -eq 476 } | ForEach-Object { Write-Output ("  PID=" + $_.ProcessId + " PPID=" + $_.ParentProcessId + " S" + $_.SessionId + " " + $_.Name) }
Write-Output "=== does the daemon's own data live under the isolation root? ==="
Get-ChildItem -Recurse (Join-Path $iso 'data') -ErrorAction SilentlyContinue | Select-Object -First 12 | ForEach-Object { Write-Output ("  " + $_.FullName.Replace($iso,'')) }
Write-Output "=== exact-PID kill of split-happy's OWN tree only ==="
foreach ($pid2 in @(476,10448,27816,7716,22456)) {
  $p = Get-CimInstance Win32_Process -Filter ("ProcessId=" + $pid2) -ErrorAction SilentlyContinue
  if ($p) { Write-Output ("  KILL " + $pid2 + " " + $p.Name + " S" + $p.SessionId); taskkill /T /F /PID $pid2 2>&1 | Out-String | Write-Output }
}
Start-Sleep -Seconds 3
Write-Output "=== after ==="
Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like '*task9-120bc965*' -or ($_.Name -eq 'ferryx.exe' -and $_.ExecutablePath -like '*source-21dea3c0*') } | Select-Object ProcessId,ParentProcessId,Name,SessionId | Format-Table -AutoSize | Out-String
Write-Output "=== can the isolation root now be removed? ==="
try { Remove-Item -Recurse -Force $iso -ErrorAction Stop; Write-Output ("ISO_REMOVED_AFTER_KILL=" + -not (Test-Path $iso)) } catch { Write-Output ("ISO_REMOVE_FAILED: " + $_.Exception.Message) }
Write-Output "CLEAR6_DONE"
