$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
Write-Output ("NOW=" + (Get-Date).ToString('HH:mm:ss'))
Write-Output "=== the runner's relaunch record (did it delegate?) ==="
$rr = Join-Path $base 'ev56\r1\windows-interactive-relaunch.json'
if (Test-Path $rr) { Get-Content $rr -Raw | ForEach-Object { Write-Output ("  " + $_.Substring(0,[Math]::Min(500,$_.Length))) } }
Write-Output "=== runner tasks now present ==="
(& schtasks /query /fo CSV /nh 2>&1) | Select-String -Pattern 'ferryx-qa|ferryx-p13|ferryx-p11' | ForEach-Object { Write-Output ("  " + $_) }
Write-Output "=== any app alive? ==="
Get-CimInstance Win32_Process -Filter "Name='ferryx.exe'" | ForEach-Object { Write-Output ("  PID=" + $_.ProcessId + " S" + $_.SessionId) }
Write-Output "=== my node/powershell for run56 ==="
Get-CimInstance Win32_Process | ForEach-Object { $c=($_.CommandLine -replace '\s+',' '); if ($c -like '*run56*' -or $c -like '*pane-liveness*') { Write-Output ("  PID=" + $_.ProcessId + " PPID=" + $_.ParentProcessId + " S" + $_.SessionId + " " + $_.Name) } }
Write-Output "=== wedge confirmation: a trivial task ==="
$task='ferryx-p13-wedge'
$o = Join-Path $base 'wedge-out.txt'
Remove-Item $o -Force -ErrorAction SilentlyContinue
$bat = Join-Path $base 'wedge.bat'
Set-Content -Path $bat -Value ('@echo off' + [char]13 + [char]10 + 'echo HELLO > "' + $o + '"' + [char]13 + [char]10) -Encoding ASCII
& schtasks /delete /tn $task /f 2>&1 | Out-Null
& schtasks /create /tn $task /tr $bat /sc once /st 00:00 /f /it 2>&1 | Out-Null
& schtasks /run /tn $task 2>&1 | Out-Null
Start-Sleep -Seconds 12
Write-Output ("  trivialTaskProducedOutput=" + (Test-Path $o))
& schtasks /query /tn $task /fo LIST /v 2>&1 | Select-String -Pattern 'Status|Last Result' | ForEach-Object { Write-Output ("  " + $_) }
& schtasks /delete /tn $task /f 2>&1 | Out-Null
Write-Output "WEDGE2_DONE"
