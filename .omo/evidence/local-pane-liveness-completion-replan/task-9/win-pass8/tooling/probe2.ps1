$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
$root=Join-Path $base 'source-21dea3c0'
$bin=Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$iso=Join-Path $base 'probe2-iso'
if (Test-Path $iso) { Remove-Item -Recurse -Force $iso }
foreach ($d in @('data','runtime','home','barriers')) { New-Item -ItemType Directory -Force -Path (Join-Path $iso $d) | Out-Null }
$env:FERRYX_DATA_DIR = Join-Path $iso 'data'
$env:FERRYX_RUNTIME_DIR = Join-Path $iso 'runtime'
$env:FERRYX_QA_BARRIER_DIR = Join-Path $iso 'barriers'
$env:FERRYX_QA_RUN_ID = 'qa-run-probe2'
$env:FERRYX_QA_OPERATION_ID = 'qa-op-probe2'
$env:FERRYX_QA_FIXTURE_KINDS = 'source'
$p = Start-Process -FilePath $bin -PassThru -WorkingDirectory $root
Write-Output ("APP_PID=" + $p.Id)
Start-Sleep -Seconds 12
$tree = & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $base 'exp-tree.ps1') -TargetPid $p.Id -Cap 60 2>&1 | Out-String
try {
  $j = $tree.Trim() | ConvertFrom-Json
  foreach ($w in $j.windows) {
    $named = @($w.items | Where-Object { $_.name -and $_.name.Length -gt 0 })
    Write-Output ("  hwnd=" + $w.hwnd + " total=" + $w.total + " named=" + $named.Count)
    foreach ($it in $named) { Write-Output ("     " + $it.name.Substring(0,[Math]::Min(90,$it.name.Length)) + "  [" + $it.controlType + "]") }
  }
} catch { Write-Output "TREE_PARSE_FAILED"; Write-Output $tree.Substring(0,[Math]::Min(800,$tree.Length)) }
taskkill /T /F /PID $p.Id 2>&1 | Out-Null
Write-Output "PROBE2_DONE"
