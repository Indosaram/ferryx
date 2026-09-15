# Registers Session-1 helper scheduled tasks for st_01a0958a QA evidence capture.
# FerryxShot_st01a0958a -> capture-screen.ps1 (full-screen PNG + top-level HWND dump)
# FerryxKey_st01a0958a  -> send-echo.ps1    (gated echo input into the QA window only)
# Helpers run capture/input ONLY against the QA app; the user's installed app is never
# targeted. Unregistered by the cleanup step at the end of the run.
$ErrorActionPreference = 'Stop'
$qaRoot = 'C:\Users\sook\ferryx-qa-rt-st01a0958a'
$principal = New-ScheduledTaskPrincipal -UserId 'desktop-1lapjmp\sook' -LogonType Interactive
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Minutes 10)
$pwsh = 'c:\program files\powershell\7\pwsh.exe'
$shotAction = New-ScheduledTaskAction -Execute $pwsh -Argument ('-NoProfile -ExecutionPolicy Bypass -File ' + $qaRoot + '\qa\capture-screen.ps1 -Tag shot')
$keyAction = New-ScheduledTaskAction -Execute $pwsh -Argument ('-NoProfile -ExecutionPolicy Bypass -File ' + $qaRoot + '\qa\send-echo.ps1')
Register-ScheduledTask -TaskName 'FerryxShot_st01a0958a' -Action $shotAction -Principal $principal -Settings $settings -Force | Out-Null
Register-ScheduledTask -TaskName 'FerryxKey_st01a0958a' -Action $keyAction -Principal $principal -Settings $settings -Force | Out-Null
Write-Host 'REGISTERED FerryxShot_st01a0958a FerryxKey_st01a0958a'
