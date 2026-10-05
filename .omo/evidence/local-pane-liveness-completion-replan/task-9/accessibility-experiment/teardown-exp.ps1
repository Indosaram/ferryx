$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
Write-Output "=== experiment processes to clear (own) ==="
$mine = Get-CimInstance Win32_Process | Where-Object {
  ($_.CommandLine -like '*exp-accessibility-probe*') -or ($_.CommandLine -like '*exp-session1*') -or
  ($_.CommandLine -like '*serve-dist*') -or ($_.CommandLine -like '*dev-frontend*') -or
  ($_.CommandLine -like '*source-21dea3c0*' -and $_.Name -in @('bun.exe','node.exe')) -or
  ($_.Name -eq 'ferryx.exe' -and $_.ExecutablePath -like '*source-21dea3c0*')
}
foreach ($p in $mine) { Write-Output ("  KILL PID=" + $p.ProcessId + " S" + $p.SessionId + " " + $p.Name); taskkill /T /F /PID $p.ProcessId 2>&1 | Out-String | Write-Output }
Start-Sleep -Seconds 2
Write-Output "=== 5173 after ==="
$c = Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue
if ($c) { Write-Output ("STILL_LISTENING pid=" + ($c|Select-Object -First 1).OwningProcess) } else { Write-Output "5173_FREE" }
Write-Output "=== experiment scheduled tasks ==="
$t = (& schtasks /query /fo CSV /nh 2>&1) | Select-String -Pattern 'ferryx-exp-'
foreach ($x in $t) { $nm = ($x -split ',')[0].Trim('"','\'); Write-Output ("  DELETE " + $nm); & schtasks /delete /tn $nm /f 2>&1 | Out-String | Write-Output }
Write-Output ("OWN_EXP_TASKS_LEFT=" + @($t).Count)
Write-Output "=== experiment isolation roots ==="
Get-ChildItem $base -Directory -Filter 'exp-*' -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  ROOT " + $_.Name); Remove-Item -Recurse -Force $_.FullName -ErrorAction SilentlyContinue }
Write-Output "=== remaining own processes ==="
$after = Get-CimInstance Win32_Process | Where-Object { ($_.CommandLine -like '*exp-*') -or ($_.CommandLine -like '*source-21dea3c0*' -and $_.Name -in @('bun.exe','node.exe','ferryx.exe')) }
foreach ($p in $after) { Write-Output ("  ALIVE PID=" + $p.ProcessId + " " + $p.Name) }
Write-Output ("TASK_OWNED_ALIVE_COUNT=" + @($after).Count)
Write-Output ("FREE_GB=" + [math]::Round((Get-PSDrive C).Free/1GB,2))
Write-Output "TEARDOWN_EXP_DONE"
