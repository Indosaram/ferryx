param([string]$Out='C:\Users\sook\ferryx-pane-completion\s1-probe.json')
$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
$root=Join-Path $base 'source-21dea3c0'
$bin=Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$srv=Join-Path $base 'serve-dist.mjs'
$dist=Join-Path $root 'ui\dist'
$res=[ordered]@{ session=(Get-Process -Id $PID).SessionId; interactive=[bool][System.Environment]::UserInteractive }

# 1) serve ui/dist on 5173 FROM THIS SESSION so it outlives the app launch
$srvLog=Join-Path $base 's1-serve.log'
$srvProc = Start-Process -FilePath 'bun' -ArgumentList @($srv, $dist) -PassThru -WindowStyle Hidden -RedirectStandardOutput $srvLog -RedirectStandardError ($srvLog+'.err')
$res.serverPid=$srvProc.Id
$ok=$false
foreach ($i in 1..20) { Start-Sleep -Milliseconds 500; if (Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue) { $ok=$true; break } }
$res.serverListening=$ok
try { $r=Invoke-WebRequest -Uri 'http://127.0.0.1:5173/' -UseBasicParsing -TimeoutSec 5; $res.http=$r.StatusCode; $res.httpBytes=$r.RawContentLength; $res.hasRoot=($r.Content -match 'id="root"') } catch { $res.http='FAIL: '+$_.Exception.Message }

# 2) launch the app in THIS SESSION with the runner's isolated env
$iso=Join-Path $base 's1-probe-iso'
if (Test-Path $iso) { Remove-Item -Recurse -Force $iso }
foreach ($d in @('data','runtime','home','barriers')) { New-Item -ItemType Directory -Force -Path (Join-Path $iso $d) | Out-Null }
$env:FERRYX_DATA_DIR=Join-Path $iso 'data'; $env:FERRYX_RUNTIME_DIR=Join-Path $iso 'runtime'
$env:FERRYX_QA_BARRIER_DIR=Join-Path $iso 'barriers'
$env:FERRYX_QA_RUN_ID='qa-run-s1probe'; $env:FERRYX_QA_OPERATION_ID='qa-op-s1probe'; $env:FERRYX_QA_FIXTURE_KINDS='source'
$app=Start-Process -FilePath $bin -PassThru -WorkingDirectory $root
$res.appPid=$app.Id
Start-Sleep -Seconds 14

# 3) dump the tree (visible windows of the app)
$tree = & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $base 'exp-tree.ps1') -TargetPid $app.Id -Cap 80 2>&1 | Out-String
$res.treeRaw = $tree.Trim()
try {
  $j=$tree.Trim()|ConvertFrom-Json
  $res.windows=@()
  foreach ($w in $j.windows) {
    $named=@($w.items | Where-Object { $_.name -and $_.name.Length -gt 0 })
    $res.windows += [ordered]@{ hwnd=$w.hwnd; total=$w.total; namedCount=$named.Count; names=@($named | ForEach-Object { $_.name }) }
  }
} catch { $res.treeParseFailed=$true }

# 4) teardown both, exact PID
try { taskkill /T /F /PID $app.Id 2>&1 | Out-Null } catch {}
try { taskkill /T /F /PID $srvProc.Id 2>&1 | Out-Null } catch {}
$res | ConvertTo-Json -Depth 8 | Set-Content -Path $Out -Encoding utf8
Write-Output "S1_PROBE_DONE"
