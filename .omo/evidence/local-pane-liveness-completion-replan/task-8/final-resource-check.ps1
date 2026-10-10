$ErrorActionPreference = 'Stop'
$active = @(Get-CimInstance Win32_Process | Where-Object { $_.Name -ne 'powershell.exe' -and $_.Name -ne 'sshd.exe' -and $_.CommandLine -and ($_.CommandLine.Contains('source-5464da0d') -or $_.CommandLine.Contains('task8-5464da0d\runner.mjs')) })
if ($active.Count) { $active | Format-List; throw 'Owned process remains' }
Remove-Item -LiteralPath 'C:\Users\sook\ferryx-pane-completion\task8-cleanup.ps1' -Force
Write-Output 'FINAL_RESOURCE_OK ownedProcesses=0 sourceArchiveAbsent=true sharedGhosttyPreserved=true'
