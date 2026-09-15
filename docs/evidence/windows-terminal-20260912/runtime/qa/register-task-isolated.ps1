# Re-registers the QA launch task to the isolated launcher (v2), preserving the pinned
# task name so no duplicate task is created. Same principal (Interactive) and settings
# as the pinned register-task.ps1; only the launcher script changes.
$ErrorActionPreference = 'Stop'
$qaRoot = 'C:\Users\sook\ferryx-qa-rt-st01a0958a'
$action = New-ScheduledTaskAction -Execute 'C:\Windows\System32\cmd.exe' -Argument ('/c "' + $qaRoot + '\qa\launch-isolated.cmd"')
$principal = New-ScheduledTaskPrincipal -UserId 'desktop-1lapjmp\sook' -LogonType Interactive
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Hours 4)
Register-ScheduledTask -TaskName 'FerryxQA_st01a0958a' -Action $action -Principal $principal -Settings $settings -Force | Format-List TaskName,State | Out-String
Write-Host 'REGISTERED FerryxQA_st01a0958a -> launch-isolated.cmd'
