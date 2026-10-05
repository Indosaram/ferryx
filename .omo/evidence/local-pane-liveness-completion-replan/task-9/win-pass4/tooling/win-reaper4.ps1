param([Parameter(Mandatory=$true)][string]$EvidenceDir,[Parameter(Mandatory=$true)][string]$IsolationRoot,[int]$Seconds=240)
$ErrorActionPreference='Continue'
$deadline=(Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $deadline) {
  $r = Get-ChildItem -Recurse -File $EvidenceDir -Filter result.json -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
  if ($r) {
    Start-Sleep -Seconds 2
    $mine = Get-CimInstance Win32_Process -Filter "Name='ferryx.exe'" | Where-Object { $_.ExecutablePath -like '*source-21dea3c0*' }
    foreach ($p in $mine) { Write-Output ("REAP_OWN_FERRYX PID=" + $p.ProcessId); taskkill /T /F /PID $p.ProcessId 2>&1 | Out-String | Write-Output }
    Write-Output ("REAP_AT=" + (Get-Date).ToString('o'))
    break
  }
  Start-Sleep -Milliseconds 400
}
Write-Output 'REAPER3_DONE'
