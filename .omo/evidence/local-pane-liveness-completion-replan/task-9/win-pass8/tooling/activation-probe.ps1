param([string]$Out='C:\Users\sook\ferryx-pane-completion\activation-probe.json')
$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
$root=Join-Path $base 'source-21dea3c0'
$bin=Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$srv=Join-Path $base 'serve-dist.mjs'
$dist=Join-Path $root 'ui\dist'
$res=[ordered]@{ session=(Get-Process -Id $PID).SessionId }
$srvProc = Start-Process -FilePath 'bun' -ArgumentList @($srv, $dist) -PassThru -WindowStyle Hidden
foreach ($i in 1..20) { Start-Sleep -Milliseconds 500; if (Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue) { break } }
$res.serverUp = [bool](Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue)
$iso=Join-Path $base 'activation-iso'
if (Test-Path $iso) { Remove-Item -Recurse -Force $iso }
foreach ($d in @('data','runtime','home','barriers')) { New-Item -ItemType Directory -Force -Path (Join-Path $iso $d) | Out-Null }
$env:FERRYX_DATA_DIR=Join-Path $iso 'data'; $env:FERRYX_RUNTIME_DIR=Join-Path $iso 'runtime'
$env:FERRYX_QA_BARRIER_DIR=Join-Path $iso 'barriers'
$env:FERRYX_QA_RUN_ID='qa-run-act'; $env:FERRYX_QA_OPERATION_ID='qa-op-act'; $env:FERRYX_QA_FIXTURE_KINDS='source'
$app=Start-Process -FilePath $bin -PassThru -WorkingDirectory $root
$res.appPid=$app.Id

function Snapshot($tag) {
  $raw = & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $base 'exp-tree.ps1') -TargetPid $app.Id -Cap 120 2>&1 | Out-String
  try {
    $j=$raw.Trim()|ConvertFrom-Json
    $named=@(); $total=0
    foreach ($w in $j.windows) { $total += $w.total; foreach ($it in $w.items) { if ($it.name -and $it.name.Length -gt 0) { $named += $it.name } } }
    return [ordered]@{ tag=$tag; totalElements=$total; namedCount=$named.Count; hasNewTerminal=($named -contains 'New Terminal'); hasSplitPaneRight=($named -contains 'Split pane right'); sample=@($named | Select-Object -First 14) }
  } catch { return [ordered]@{ tag=$tag; parseFailed=$true; raw=$raw.Substring(0,[Math]::Min(300,$raw.Length)) } }
}

Start-Sleep -Seconds 12
$res.t12 = Snapshot 't+12s (first query = the attach)'
Start-Sleep -Seconds 6
$res.t18 = Snapshot 't+18s (second query after the attach)'
Start-Sleep -Seconds 8
$res.t26 = Snapshot 't+26s (third query)'

try { taskkill /T /F /PID $app.Id 2>&1 | Out-Null } catch {}
try { taskkill /T /F /PID $srvProc.Id 2>&1 | Out-Null } catch {}
$res | ConvertTo-Json -Depth 8 | Set-Content -Path $Out -Encoding utf8
Write-Output "ACTIVATION_PROBE_DONE"
