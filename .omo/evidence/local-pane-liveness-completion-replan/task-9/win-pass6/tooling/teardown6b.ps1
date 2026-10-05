$ErrorActionPreference='Continue'
$t9='C:\Users\sook\ferryx-pane-completion\task9-120bc965'
Write-Output "=== bat exit files (CONTENT and BYTES) ==="
foreach ($s in @('split-happy','split-attach-stall','split-cancel')) {
  $f = Join-Path $t9 ('logs\bat-' + $s + '.exit')
  if (Test-Path $f) {
    $c = (Get-Content $f) -join ''
    $b = (Get-Item $f).Length
    Write-Output ("  " + $s + " CONTENT=[" + $c + "] BYTES=" + $b)
  } else {
    Write-Output ("  " + $s + " NO_EXIT_FILE")
  }
}
Write-Output "=== delegated relaunch exits ==="
$dirs = Get-ChildItem (Join-Path $env:LOCALAPPDATA 'Temp') -Directory -Filter 'ferryx-qa-relaunch-*' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 3
foreach ($d in $dirs) {
  $ex = Join-Path $d.FullName 'relaunch.exit'
  $bat = Get-ChildItem $d.FullName -File -Filter '*.bat' -ErrorAction SilentlyContinue | Select-Object -First 1
  $name = 'unknown'
  if ($bat) { $name = $bat.BaseName }
  $c = '<none>'
  $b = -1
  if (Test-Path $ex) { $c = (Get-Content $ex) -join ''; $b = (Get-Item $ex).Length }
  Write-Output ("  " + $name + " CONTENT=[" + $c + "] BYTES=" + $b)
}
Write-Output "=== ISOLATION-ROOT HOLDER (lead's ask, final) ==="
$iso = Join-Path $t9 'runtime'
$holders = Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like ('*' + $iso + '*') }
foreach ($h in $holders) { Write-Output ("  HOLDER PID=" + $h.ProcessId + " PPID=" + $h.ParentProcessId + " S" + $h.SessionId + " " + $h.Name) }
Get-ChildItem $iso -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  ROOT_STILL_PRESENT " + $_.Name) }
Write-Output "=== TEARDOWN: exact-PID, own tree only ==="
$mine = Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like '*task9-120bc965*' -or $_.CommandLine -like '*run-scenario6*' -or ($_.Name -eq 'ferryx.exe' -and $_.ExecutablePath -like '*source-21dea3c0*') }
foreach ($p in $mine) { Write-Output ("  KILL " + $p.ProcessId + " " + $p.Name); taskkill /T /F /PID $p.ProcessId 2>&1 | Out-String | Write-Output }
Start-Sleep -Seconds 3
$after = Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like '*task9-120bc965*' -or $_.CommandLine -like '*run-scenario6*' -or ($_.Name -eq 'ferryx.exe' -and $_.ExecutablePath -like '*source-21dea3c0*') }
Write-Output ("TASK_OWNED_ALIVE_COUNT=" + @($after).Count)
Write-Output "=== own scheduled tasks ==="
$tasks = (& schtasks /query /fo CSV /nh 2>&1) | Select-String -Pattern 'ferryx-qa-'
foreach ($t in $tasks) { $nm = ($t -split ',')[0].Trim('"','\'); Write-Output ("  DELETE " + $nm); & schtasks /delete /tn $nm /f 2>&1 | Out-String | Write-Output }
Write-Output ("FREE_GB=" + [math]::Round((Get-PSDrive C).Free/1GB,2))
Write-Output "TEARDOWN6B_DONE"
