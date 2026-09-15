# stop-qa.ps1 - owned cleanup for st_01a0958a. Kills ONLY processes whose command
# line or executable path references the QA root C:\Users\sook\ferryx-qa-rt-st01a0958a,
# then unregisters the owned scheduled task. Prints exact handles killed/skipped.
$ErrorActionPreference = 'Continue'
$qaRoot = 'C:\Users\sook\ferryx-qa-rt-st01a0958a'
$taskName = 'FerryxQA_st01a0958a'

$owned = Get-CimInstance Win32_Process | Where-Object {
  ($_.CommandLine -like "*$qaRoot*") -or
  ($_.ExecutablePath -like "*$qaRoot*")
}
Write-Host "OWNED_PROCESSES_BEFORE"
$owned | Select-Object ProcessId, ParentProcessId, Name | Format-Table -AutoSize | Out-String | Write-Host

foreach ($p in $owned) {
  if ($p.ProcessId -eq $PID) { Write-Host "SKIP self $($p.ProcessId)"; continue }
  Write-Host "KILL $($p.ProcessId) $($p.Name)"
  taskkill /T /F /PID $p.ProcessId 2>&1 | Out-Null
}
Start-Sleep -Seconds 2
$remaining = Get-CimInstance Win32_Process | Where-Object {
  ($_.CommandLine -like "*$qaRoot*") -or ($_.ExecutablePath -like "*$qaRoot*")
}
if ($remaining) { $remaining | Select-Object ProcessId, Name | Format-Table -AutoSize | Out-String | Write-Host }
Write-Host ("REMAINING_OWNED " + [bool]$remaining)

Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction Continue
Write-Host ('TASK_AFTER ' + (Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue).State)
Write-Host 'USER_DAEMON_UNTOUCHED_CHECK: ferryx installed app processes still present:'
Get-Process -Name ferryx -ErrorAction SilentlyContinue | Where-Object { $_.Path -notlike "$qaRoot*" } | Select-Object Id, Path | Out-String | Write-Host
