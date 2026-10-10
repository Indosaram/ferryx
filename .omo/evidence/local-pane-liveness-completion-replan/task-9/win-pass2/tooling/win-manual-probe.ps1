$ErrorActionPreference='Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$bin = Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$probe = Join-Path $base 'task9-314251e0\manual-probe'
if (Test-Path $probe) { Remove-Item -Recurse -Force $probe }
$bd = Join-Path $probe 'barriers'
$data = Join-Path $probe 'data'
$rt = Join-Path $probe 'runtime'
$home = Join-Path $probe 'home'
foreach ($d in @($bd,$data,$rt,$home)) { New-Item -ItemType Directory -Force -Path $d | Out-Null }
$env:FERRYX_DATA_DIR = $data
$env:FERRYX_RUNTIME_DIR = $rt
$env:HOME = $home
$env:FERRYX_QA_BARRIER_DIR = $bd
$env:FERRYX_QA_RUN_ID = 'qa-run-manualprobe'
$env:FERRYX_QA_OPERATION_ID = 'qa-op-manualprobe'
Write-Output "PROBE_DIR=$probe"
$p = Start-Process -FilePath $bin -PassThru -WorkingDirectory $root
Write-Output "PROBE_PID=$($p.Id) at $((Get-Date).ToString('o'))"
foreach ($t in 5,10,20,40,60) {
  Start-Sleep -Seconds ($t - [int]$last)
  $last = $t
  Write-Output "=== T+$t s ==="
  Get-ChildItem -Recurse -File $bd -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  B " + $_.Name + " " + $_.Length) }
  if (Test-Path (Join-Path $bd 'fixture-setup.receipt.jsonl')) { Write-Output "  RECEIPT:"; Get-Content (Join-Path $bd 'fixture-setup.receipt.jsonl') }
}
Write-Output "=== data dir tree ==="
Get-ChildItem -Recurse $data -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  D " + $_.FullName.Replace($probe,'') + " " + $_.Length) }
Write-Output "=== runtime dir tree ==="
Get-ChildItem -Recurse $rt -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  R " + $_.FullName.Replace($probe,'') + " " + $_.Length) }
Write-Output "=== probe process still alive? ==="
$alive = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
Write-Output ("ALIVE=" + ($alive -ne $null))
taskkill /T /F /PID $p.Id 2>&1 | Out-String | Write-Output
Write-Output "MANUAL_PROBE_DONE"
