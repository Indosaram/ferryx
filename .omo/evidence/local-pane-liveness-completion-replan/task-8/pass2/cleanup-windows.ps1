$ErrorActionPreference = 'Stop'
$root='C:\Users\sook\ferryx-pane-completion\source-172baa87'
$active=@(Get-CimInstance Win32_Process | Where-Object { $_.Name -ne 'powershell.exe' -and $_.Name -ne 'cmd.exe' -and $_.Name -ne 'sshd.exe' -and $_.CommandLine -and ($_.CommandLine.Contains('source-172baa87') -or $_.CommandLine.Contains('base-pass2-d82b35e4') -or $_.CommandLine.Contains('task8-172baa87\runner.mjs') -or $_.CommandLine.Contains('ab-runner.mjs')) })
if ($active.Count) { $active | Format-List; throw 'Owned executable remains' }
$junction="$root\src-tauri\vendor\ghostty"
if (Test-Path $junction) { cmd.exe /d /c "rmdir $junction"; $native=$LASTEXITCODE; if($native -ne 0){exit $native} }
Remove-Item -LiteralPath $root -Recurse -Force
Remove-Item -LiteralPath 'C:\Users\sook\ferryx-pane-completion\pass2-source.tar' -Force
Remove-Item -LiteralPath 'C:\Users\sook\ferryx-pane-completion\base-pass2-d82b35e4' -Recurse -Force
Remove-Item -LiteralPath 'C:\Users\sook\ferryx-pane-completion\base-pass2.tar' -Force
Write-Output "CLEANUP_OK sourceAbsent=$(-not(Test-Path $root)) ownedProcesses=0 ghosttyPreserved=$(Test-Path 'C:\Users\sook\task2-ghostty-6a508fd5')"

