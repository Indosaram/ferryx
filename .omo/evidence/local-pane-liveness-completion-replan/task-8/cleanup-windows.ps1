$ErrorActionPreference = 'Stop'
$root = 'C:\Users\sook\ferryx-pane-completion\source-5464da0d'
$matches = @(Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -and ($_.CommandLine.Contains('source-5464da0d') -or $_.CommandLine.Contains('task8-5464da0d\runner.mjs')) -and $_.ProcessId -ne $PID -and $_.Name -ne 'sshd.exe' -and $_.Name -ne 'cmd.exe' -and $_.Name -ne 'powershell.exe' })
if ($matches.Count -gt 0) { $matches | Format-List ProcessId,Name,CommandLine; throw 'Owned executable still active; refusing cleanup' }
$junction = "$root\src-tauri\vendor\ghostty"
if (Test-Path $junction) { cmd.exe /d /c "rmdir $junction"; $native=$LASTEXITCODE; if ($native -ne 0) { exit $native } }
if (Test-Path $root) { Remove-Item -LiteralPath $root -Recurse -Force }
Remove-Item -LiteralPath 'C:\Users\sook\ferryx-pane-completion\task8-source.tar' -Force
Write-Output "CLEANUP_OK sourceAbsent=$(-not (Test-Path $root)) ghosttyPreserved=$(Test-Path 'C:\Users\sook\task2-ghostty-6a508fd5')"
