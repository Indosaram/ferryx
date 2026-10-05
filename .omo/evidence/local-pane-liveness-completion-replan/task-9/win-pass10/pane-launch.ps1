param([string]$Scenario='split-happy')
$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
$out=Join-Path $base 'pane-delta.json'
$done=$out + '.done'
$script=Join-Path $base 'pane-delta.ps1'
Remove-Item -Force $out,$done -ErrorAction SilentlyContinue
$task='ferryx-pd'
$tr='powershell -NoProfile -ExecutionPolicy Bypass -File "' + $script + '" -Scenario ' + $Scenario + ' -Out "' + $out + '"'
& schtasks /delete /tn $task /f 2>&1 | Out-Null
& schtasks /create /tn $task /tr $tr /sc once /st 00:00 /f /it 2>&1 | Out-Null
& schtasks /run /tn $task 2>&1 | Out-Null
$deadline=(Get-Date).AddSeconds(500)
while ((Get-Date) -lt $deadline) { if (Test-Path $done) { break }; Start-Sleep -Milliseconds 500 }
Start-Sleep -Seconds 2
& schtasks /delete /tn $task /f 2>&1 | Out-Null
Write-Output ("DONE_MARKER=" + (Test-Path $done))
