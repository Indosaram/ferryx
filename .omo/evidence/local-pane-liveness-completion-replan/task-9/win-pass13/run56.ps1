$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
$root=Join-Path $base 'source-21dea3c0'
$bin=Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$node='C:\Program Files\nodejs\node.exe'
$iso=Join-Path $base 'iso56\r1'
$ev=Join-Path $base 'ev56\r1'
$log=Join-Path $base 'log56\r1'
foreach ($d in @($iso,$ev,$log)) { if (Test-Path $d) { Remove-Item -Recurse -Force $d -ErrorAction SilentlyContinue } }
foreach ($d in @((Split-Path $iso -Parent),(Split-Path $ev -Parent),(Split-Path $log -Parent))) { New-Item -ItemType Directory -Force -Path $d | Out-Null }
$runLog=Join-Path $log 'runner.out'; $runErr=Join-Path $log 'runner.err'
$launcher = Start-Process -FilePath $node -ArgumentList @('scripts\qa\pane-liveness.mjs','--scenario','split-happy','--binary',$bin,'--evidence-dir',$ev,'--isolation-root',$iso) -PassThru -WindowStyle Hidden -WorkingDirectory $root -RedirectStandardOutput $runLog -RedirectStandardError $runErr
Write-Output ("launcherPid=" + $launcher.Id)
# The runner self-delegates to session 1; completion is its own result.json.
$sw=[System.Diagnostics.Stopwatch]::StartNew(); $done=$null
while ($sw.Elapsed.TotalSeconds -lt 240) {
  $evRoot=Join-Path $ev 'task-3-harness\split-happy'
  if (Test-Path $evRoot) {
    $d=@(Get-ChildItem $evRoot -Directory -Filter 'run-*' -ErrorAction SilentlyContinue | Where-Object { Test-Path (Join-Path $_.FullName 'result.json') } | Sort-Object LastWriteTime -Descending)
    if ($d.Count -gt 0) { $done=$d[0].FullName; break }
  }
  Start-Sleep -Milliseconds 700
}
Write-Output ("elapsedMs=" + $sw.ElapsedMilliseconds + " done=" + $done)
if ($done) {
  $rj = Get-Content (Join-Path $done 'result.json') -Raw | ConvertFrom-Json
  Write-Output ("verdict=" + $rj.verdict + " code=" + $rj.error.code)
  Write-Output ("message=" + $rj.error.message)
  Copy-Item (Join-Path $done 'result.json') (Join-Path $ev 'runner-result.json') -Force -ErrorAction SilentlyContinue
  Copy-Item (Join-Path $done 'actions.jsonl') (Join-Path $ev 'runner-actions.jsonl') -Force -ErrorAction SilentlyContinue
}
Write-Output "=== evidence dir files ==="
Get-ChildItem $ev -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  " + $_.FullName.Substring($ev.Length+1) + " [" + $_.Length + "]") }
Write-Output "=== the app's own stdio (always-on sink) ==="
foreach ($f in @('app.stdout.log','app.stderr.log')) {
  Get-ChildItem $ev -Recurse -Filter $f -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Output ("--- " + $_.FullName)
    Get-Content $_.FullName -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("    " + $_) }
  }
}
Write-Output "=== verifier probe ==="
$vp = Join-Path $ev 'verifier-probe.jsonl'
if (Test-Path $vp) { Get-Content $vp | ForEach-Object { Write-Output ("  " + $_) } } else { Write-Output "  absent" }
Write-Output "RUN56_DONE"
