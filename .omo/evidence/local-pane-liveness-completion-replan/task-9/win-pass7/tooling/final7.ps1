$ErrorActionPreference='Continue'
$t9='C:\Users\sook\ferryx-pane-completion\task9-42fba06f'
Write-Output "=== any capture? ==="
$imgs = Get-ChildItem -Recurse -File (Join-Path $t9 'evidence') -ErrorAction SilentlyContinue | Where-Object { $_.Extension -in @('.png','.jpg','.jpeg') }
foreach ($i in $imgs) { Write-Output ("  " + $i.FullName) }
Write-Output ("IMAGE_COUNT=" + @($imgs).Count)
Write-Output "=== action sequences ==="
foreach ($s in @('split-happy','split-attach-stall','split-cancel')) {
  $a = Get-ChildItem -Recurse -File (Join-Path $t9 ('evidence\' + $s)) -Filter actions.jsonl -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($a) { Write-Output ("  " + $s + ": " + ((Get-Content $a.FullName | ForEach-Object { ($_ | ConvertFrom-Json).action }) -join ' | ')) }
}
Write-Output "=== isolation roots after (force-reap should have removed them) ==="
Get-ChildItem (Join-Path $t9 'runtime') -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  REMAINS " + $_.Name) }
Write-Output "=== own processes / tasks ==="
$p = Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like '*task9-42fba06f*' -or ($_.Name -eq 'ferryx.exe' -and $_.ExecutablePath -like '*source-21dea3c0*') }
foreach ($x in $p) { Write-Output ("  ALIVE PID=" + $x.ProcessId + " " + $x.Name) }
Write-Output ("TASK_OWNED_ALIVE_COUNT=" + @($p).Count)
$t = (& schtasks /query /fo CSV /nh 2>&1) | Select-String -Pattern 'ferryx-qa-'
foreach ($x in $t) { Write-Output ("  TASK_LEFT " + $x) }
Write-Output ("OWN_TASK_COUNT=" + @($t).Count)
Write-Output ("FREE_GB=" + [math]::Round((Get-PSDrive C).Free/1GB,2))
Write-Output "FINAL7_DONE"
