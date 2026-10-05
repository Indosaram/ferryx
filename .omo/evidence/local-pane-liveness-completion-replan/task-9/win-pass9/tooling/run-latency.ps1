$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
$out=Join-Path $base 'latency-probe.json'
$script=Join-Path $base 'latency-probe.ps1'
Remove-Item -Force $out -ErrorAction SilentlyContinue
$task='ferryx-latency'
$tr='powershell -NoProfile -ExecutionPolicy Bypass -File "' + $script + '" -Out "' + $out + '"'
& schtasks /delete /tn $task /f 2>&1 | Out-Null
& schtasks /create /tn $task /tr $tr /sc once /st 00:00 /f /it 2>&1 | Out-Null
& schtasks /run /tn $task 2>&1 | Out-Null
$deadline=(Get-Date).AddSeconds(600)
while ((Get-Date) -lt $deadline) { if (Test-Path $out) { break }; Start-Sleep -Milliseconds 500 }
Start-Sleep -Seconds 2
& schtasks /delete /tn $task /f 2>&1 | Out-Null
Write-Output ("OUT_EXISTS=" + (Test-Path $out))
