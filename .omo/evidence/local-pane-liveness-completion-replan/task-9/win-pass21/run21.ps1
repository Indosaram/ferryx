param([string]$Tag = 'r1', [int]$TimeoutSec = 1500)
$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$bin  = Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$node = 'C:\Program Files\nodejs\node.exe'
$iso  = Join-Path $base ('iso21\' + $Tag)
$ev   = Join-Path $base ('ev21\' + $Tag)
$log  = Join-Path $base ('log21\' + $Tag)
$outPath = Join-Path $base ('run21-' + $Tag + '.json')
$prog = Join-Path $base ('run21-' + $Tag + '.progress.log')
Remove-Item $outPath, $prog -Force -ErrorAction SilentlyContinue
function Log([string]$m) { $l = (Get-Date).ToString('HH:mm:ss.fff') + '  ' + $m; Add-Content -Path $prog -Value $l -Encoding UTF8; Write-Output $l }
foreach ($d in @($iso, $ev, $log)) { if (Test-Path $d) { Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue } }
foreach ($d in @((Split-Path $iso -Parent), (Split-Path $ev -Parent), (Split-Path $log -Parent))) { New-Item -ItemType Directory -Force -Path $d | Out-Null }
New-Item -ItemType Directory -Force -Path $log | Out-Null

$res = [ordered]@{ tag=$Tag; revision='14744c8a (staged) + verifier probe'; startedAt=(Get-Date).ToString('o'); iso=$iso; ev=$ev }
$res | ConvertTo-Json -Depth 10 | Set-Content $outPath -Encoding UTF8

$runLog = Join-Path $log 'runner.out'; $runErr = Join-Path $log 'runner.err'
$launcher = Start-Process -FilePath $node -ArgumentList @('scripts\qa\pane-liveness.mjs','--scenario','split-happy','--binary',$bin,'--evidence-dir',$ev,'--isolation-root',$iso) -PassThru -WindowStyle Hidden -WorkingDirectory $root -RedirectStandardOutput $runLog -RedirectStandardError $runErr
$res.launcherPid = $launcher.Id
Log ('launcher pid=' + $launcher.Id + ' (the runner self-delegates; the in-tree retry runs inside it)')

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$doneDir = $null
$attemptMarkers = New-Object System.Collections.ArrayList
while ($sw.Elapsed.TotalSeconds -lt $TimeoutSec) {
  # watch the retry ledger / attempt dirs appear, so a long wait is observable
  $attemptRoot = Join-Path $ev 'delegation'
  if (Test-Path $attemptRoot) {
    foreach ($d in @(Get-ChildItem $attemptRoot -Directory -ErrorAction SilentlyContinue)) {
      $tag2 = $d.Name
      if (-not ($attemptMarkers -contains $tag2)) { $attemptMarkers.Add($tag2) | Out-Null; Log ('  attempt dir appeared: ' + $tag2) }
    }
  }
  $evRoot = Join-Path $ev 'task-3-harness\split-happy'
  if (Test-Path $evRoot) {
    $d = @(Get-ChildItem $evRoot -Directory -Filter 'run-*' -ErrorAction SilentlyContinue | Where-Object { Test-Path (Join-Path $_.FullName 'result.json') } | Sort-Object LastWriteTime -Descending)
    if ($d.Count -gt 0) { $doneDir = $d[0].FullName; break }
  }
  Start-Sleep -Milliseconds 1500
}
$res.elapsedMs = $sw.ElapsedMilliseconds
$res.completedRunDir = $doneDir
$res.attemptDirs = @($attemptMarkers)
Log ('elapsedMs=' + $sw.ElapsedMilliseconds + ' doneDir=' + $doneDir)

# collect everything, wherever it landed
$res.evidenceFiles = @(Get-ChildItem $ev -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
if ($doneDir) {
  Copy-Item (Join-Path $doneDir 'result.json') (Join-Path $ev 'runner-result.json') -Force -ErrorAction SilentlyContinue
  Copy-Item (Join-Path $doneDir 'actions.jsonl') (Join-Path $ev 'runner-actions.jsonl') -Force -ErrorAction SilentlyContinue
  $j = Get-Content (Join-Path $doneDir 'result.json') -Raw | ConvertFrom-Json
  $res.verdict = $j.verdict; $res.code = $j.error.code; $res.message = $j.error.message
  Log ('VERDICT=' + $j.verdict + ' code=' + $j.error.code)
  Log ('MESSAGE=' + $j.error.message)
  Write-Output "=== app.stderr.log (always-on sink) ==="
  Get-ChildItem $ev -Recurse -Filter 'app.stderr.log' -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("--- " + $_.FullName); Get-Content $_.FullName -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  SINK: " + $_) } }
  Write-Output "=== verifier-probe.jsonl ==="
  Get-ChildItem $ev -Recurse -Filter 'verifier-probe.jsonl' -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("--- " + $_.FullName + " [" + $_.Length + "]"); Get-Content $_.FullName | ForEach-Object { Write-Output ("  PROBE: " + $_) } }
  Write-Output "=== actions timeline ==="
  foreach ($l in (Get-Content (Join-Path $doneDir 'actions.jsonl') -ErrorAction SilentlyContinue)) { try { $a = $l | ConvertFrom-Json; Write-Output ("  " + $a.action) } catch { } }
  Write-Output "=== delegation ledger (attemptsUsed / stopReason) ==="
  $raw = Get-Content (Join-Path $doneDir 'actions.jsonl') -Raw -ErrorAction SilentlyContinue
  foreach ($m in [regex]::Matches($raw, '"attemptsUsed":\s*\d+')) { Write-Output ("  " + $m.Value) }
  foreach ($m in [regex]::Matches($raw, '"stopReason":\s*"[^"]*"')) { Write-Output ("  " + $m.Value) }
  Write-Output "=== settledBy ==="
  foreach ($m in [regex]::Matches($raw, '"settledBy":\s*"[^"]*"')) { Write-Output ("  " + $m.Value) }
} else {
  Log 'NO RESULT DIR - the sequence did not complete within the timeout'
  Write-Output "=== outer runner.err tail ==="
  Get-Content $runErr -Tail 20 -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  " + $_) }
  Write-Output "=== outer runner.out tail ==="
  Get-Content $runLog -Tail 30 -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  " + $_) }
}
$res | ConvertTo-Json -Depth 10 | Set-Content $outPath -Encoding UTF8
Write-Output ('RUN20_DONE verdict=' + $res.verdict + ' code=' + $res.code + ' elapsedMs=' + $res.elapsedMs)
