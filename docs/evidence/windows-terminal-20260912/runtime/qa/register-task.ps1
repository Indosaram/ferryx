# Registers the interactive scheduled task for st_01a0958a QA launch.
# Principal: interactive console user. Root command inside launch.cmd: bun tauri dev (debug).
$ErrorActionPreference = 'Stop'
$qaRoot = 'C:\Users\sook\ferryx-qa-rt-st01a0958a'
$action = New-ScheduledTaskAction -Execute 'C:\Windows\System32\cmd.exe' -Argument ('/c "' + $qaRoot + '\qa\launch.cmd"')
$principal = New-ScheduledTaskPrincipal -UserId 'desktop-1lapjmp\sook' -LogonType Interactive
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Hours 4)
Register-ScheduledTask -TaskName 'FerryxQA_st01a0958a' -Action $action -Principal $principal -Settings $settings -Force | Format-List TaskName,State | Out-String
Write-Host 'REGISTERED FerryxQA_st01a0958a'
