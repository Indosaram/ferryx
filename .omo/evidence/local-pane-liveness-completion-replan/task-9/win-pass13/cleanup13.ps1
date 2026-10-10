$ErrorActionPreference='Continue'
Write-Output "=== kill my stuck processes by exact recorded PID ==="
foreach ($pid2 in @(65204, 67708, 20748, 36640, 21524, 44448)) {
  $p = Get-CimInstance Win32_Process -Filter ("ProcessId=" + $pid2) -ErrorAction SilentlyContinue
  if (-not $p) { Write-Output ("GONE " + $pid2); continue }
  $c = ($p.CommandLine -replace '\s+',' ')
  $mine = ($c -like '*run56*') -or ($c -like '*pane-liveness*') -or ($c -like '*run5c*') -or ($c -like '*run7*')
  if ($mine) { Write-Output ("IDENTITY_OK PID=" + $pid2 + " " + $p.Name); taskkill /T /F /PID $pid2 2>&1 | Out-Null }
  else { Write-Output ("MISMATCH PID=" + $pid2 + " NOT KILLED :: " + $c.Substring(0,[Math]::Min(80,$c.Length))) }
}
Write-Output "=== end + delete the stuck tasks (mine: the runner tasks my launch created) ==="
foreach ($t in @('ferryx-qa-split-happy-36640-uwdnx5','ferryx-qa-split-happy-65204-cpzf1h','ferryx-p13-cap','ferryx-p13-bat','ferryx-p13-probe','ferryx-p13-wedge')) {
  & schtasks /end /tn $t 2>&1 | Out-Null
  & schtasks /delete /tn $t /f 2>&1 | Out-Null
}
$left = (& schtasks /query /fo CSV /nh 2>&1) | Select-String -Pattern 'ferryx-qa|ferryx-p13|ferryx-p11|ferryx-pd|ferryx-pf'
Write-Output ("  ownTasksLeft=" + @($left).Count)
foreach ($l in $left) { Write-Output ("     " + $l) }
Start-Sleep -Seconds 2
Write-Output "=== remaining of mine ==="
$still = Get-CimInstance Win32_Process | ForEach-Object { $c=($_.CommandLine -replace '\s+',' '); if ($c -like '*run56*' -or $c -like '*run5c*' -or $c -like '*run7*' -or $c -like '*pane-liveness*' -or $c -like '*capture13*' -or $c -like '*capture1*' -or $c -like '*serve-dist*' -or ($_.Name -eq 'ferryx.exe' -and $c -like '*source-21dea3c0*')) { "  STILL PID=" + $_.ProcessId + " S" + $_.SessionId + " " + $_.Name } }
Write-Output ("REMAINING_MINE=" + @($still).Count)
foreach ($s in $still) { Write-Output $s }
Write-Output ("PORT_5173=" + $(if (Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue) { 'LISTENING' } else { 'FREE' }))
$h = Join-Path $env:APPDATA 'com.ferryx.app\dev\session_state.json'
Write-Output ("PROFILE_SHA=" + (Get-FileHash $h -Algorithm SHA256).Hash)
Write-Output ("PROFILE_MTIME=" + (Get-Item $h).LastWriteTime.ToString('o'))
Write-Output ("FREE_GB=" + [math]::Round((Get-PSDrive C).Free/1GB,2))
Write-Output "CLEANUP13_DONE"
