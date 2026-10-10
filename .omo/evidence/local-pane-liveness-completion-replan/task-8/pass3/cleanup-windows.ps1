$ErrorActionPreference = 'Stop'
$root='C:\Users\sook\ferryx-pane-completion\source-21dea3c0'
$active=@(Get-CimInstance Win32_Process | Where-Object { $_.Name -notin @('powershell.exe','cmd.exe','sshd.exe') -and $_.CommandLine -and ($_.CommandLine.Contains('source-21dea3c0') -or $_.CommandLine.Contains('task8-21dea3c0\runner3.mjs')) })
if ($active.Count) { $active | Format-List; throw 'Owned executable remains' }
$junction="$root\src-tauri\vendor\ghostty"
if (Test-Path $junction) { cmd.exe /d /c "rmdir $junction"; $native=$LASTEXITCODE; if($native -ne 0){exit $native} }
Remove-Item -LiteralPath $root -Recurse -Force
Remove-Item -LiteralPath 'C:\Users\sook\ferryx-pane-completion\pass3-source.tar.gz' -Force
Write-Output "CLEANUP_OK sourceAbsent=$(-not(Test-Path $root)) ownedProcesses=0 ghosttyPreserved=$(Test-Path 'C:\Users\sook\task2-ghostty-6a508fd5')"
