# Registers a scheduled task that runs the CDP console recorder detached from any SSH
# session (SSH-launched processes are killed when the session closes). Recorder output
# goes to evidence\cdp-recorder.out/.err via cmd redirection. Stop by creating
# evidence\cdp-stop.txt (the recorder watches it with fs.watch).
$ErrorActionPreference = 'Stop'
$qaRoot = 'C:\Users\sook\ferryx-qa-rt-st01a0958a'
$cmd = '/c cd /d "' + $qaRoot + '\qa" && "C:\Program Files\nodejs\node.exe" cdp-console-capture.mjs 1800 > "' + $qaRoot + '\evidence\cdp-recorder.out" 2> "' + $qaRoot + '\evidence\cdp-recorder.err"'
$action = New-ScheduledTaskAction -Execute 'C:\Windows\System32\cmd.exe' -Argument $cmd
$principal = New-ScheduledTaskPrincipal -UserId 'desktop-1lapjmp\sook' -LogonType Interactive
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Minutes 40)
Register-ScheduledTask -TaskName 'FerryxCdp_st01a0958a' -Action $action -Principal $principal -Settings $settings -Force | Out-Null
Write-Host 'REGISTERED FerryxCdp_st01a0958a'
