$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
Write-Output "=== teardown own experiment resources ==="
$mine = Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like '*serve-dist*' -or $_.CommandLine -like '*activation-probe*' -or $_.CommandLine -like '*s1-serve-and-probe*' -or $_.CommandLine -like '*probe-with-server*' -or ($_.Name -eq 'ferryx.exe' -and $_.ExecutablePath -like '*source-21dea3c0*') }
foreach ($p in $mine) { Write-Output ("  KILL PID=" + $p.ProcessId + " " + $p.Name); taskkill /T /F /PID $p.ProcessId 2>&1 | Out-Null }
$t = (& schtasks /query /fo CSV /nh 2>&1) | Select-String -Pattern 'ferryx-actprobe|ferryx-s1probe|ferryx-exp-'
foreach ($x in $t) { $nm=($x -split ',')[0].Trim('"','\'); Write-Output ("  DELETE_TASK " + $nm); & schtasks /delete /tn $nm /f 2>&1 | Out-Null }
Get-ChildItem $base -Directory -Filter '*iso*' -ErrorAction SilentlyContinue | ForEach-Object { Remove-Item -Recurse -Force $_.FullName -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 2
$after = Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like '*serve-dist*' -or $_.CommandLine -like '*activation-probe*' -or ($_.Name -eq 'ferryx.exe' -and $_.ExecutablePath -like '*source-21dea3c0*') }
Write-Output ("TASK_OWNED_ALIVE_COUNT=" + @($after).Count)
Write-Output ("PORT_5173=" + $(if (Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue) { 'LISTENING' } else { 'FREE' }))
Write-Output ("FREE_GB=" + [math]::Round((Get-PSDrive C).Free/1GB,2))
Write-Output "FINAL8_DONE"
