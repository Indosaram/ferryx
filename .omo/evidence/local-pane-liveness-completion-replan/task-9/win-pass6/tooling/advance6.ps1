param([int]$TimeoutSec=600)
$ErrorActionPreference='Continue'
$t9='C:\Users\sook\ferryx-pane-completion\task9-120bc965'
$deadline=(Get-Date).AddSeconds($TimeoutSec)
$seen=@{}
while ((Get-Date) -lt $deadline) {
  $all=$true
  foreach ($s in @('split-happy','split-attach-stall','split-cancel')) {
    $rj = Get-ChildItem -Recurse -File (Join-Path $t9 ('evidence\' + $s)) -Filter result.json -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($rj) {
      if (-not $seen.ContainsKey($s)) { $seen[$s]=$true; Write-Output ("RESULT_APPEARED " + $s) }
    } else { $all=$false }
  }
  if ($all) { Write-Output "ALL_THREE_PRESENT"; break }
  # Clear each scenario's own daemon as soon as its result lands, so the serialized
  # wrapper can advance. Exact PIDs scoped to THIS task's staged tree only.
  foreach ($s in @('split-happy','split-attach-stall','split-cancel')) {
    if ($seen.ContainsKey($s) -and -not $seen.ContainsKey($s + ':cleared')) {
      $iso = Join-Path $t9 ('runtime\' + $s)
      $holders = Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like ('*' + $iso + '*') }
      foreach ($h in $holders) { Write-Output ("  CLEAR " + $s + " PID=" + $h.ProcessId + " " + $h.Name); taskkill /T /F /PID $h.ProcessId 2>&1 | Out-Null }
      $d = Get-CimInstance Win32_Process -Filter "Name='ferryx.exe'" | Where-Object { $_.ExecutablePath -like '*source-21dea3c0*' -and $_.CommandLine -like '*--daemon*' }
      foreach ($x in $d) { Write-Output ("  CLEAR_DAEMON PID=" + $x.ProcessId); taskkill /T /F /PID $x.ProcessId 2>&1 | Out-Null }
      $seen[$s + ':cleared']=$true
    }
  }
  Start-Sleep -Seconds 5
}
foreach ($s in @('split-happy','split-attach-stall','split-cancel')) {
  $rj = Get-ChildItem -Recurse -File (Join-Path $t9 ('evidence\' + $s)) -Filter result.json -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($rj) { $j = Get-Content $rj.FullName -Raw | ConvertFrom-Json; Write-Output ("RESULT " + $s + " verdict=" + $j.verdict + " code=" + $j.error.code + " gate=" + $j.cleanupGate.ok) } else { Write-Output ("RESULT " + $s + " MISSING") }
}
Write-Output "ADVANCE6_DONE"
