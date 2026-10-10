param([string]$Tag = 'r1', [int]$TimeoutSec = 200)
$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$bin  = Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$node = 'C:\Program Files\nodejs\node.exe'
$iso  = Join-Path $base ('iso17\' + $Tag)
$ev   = Join-Path $base ('ev17\' + $Tag)
$log  = Join-Path $base ('log17\' + $Tag)
$outPath = Join-Path $base ('run17-' + $Tag + '.json')
foreach ($d in @($iso, $ev, $log)) { if (Test-Path $d) { Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue } }
foreach ($d in @((Split-Path $iso -Parent), (Split-Path $ev -Parent), (Split-Path $log -Parent))) { New-Item -ItemType Directory -Force -Path $d | Out-Null }
New-Item -ItemType Directory -Force -Path $log | Out-Null

function Snapshot {
  $con = Get-CimInstance Win32_Process -Filter "Name='conhost.exe'"
  $bySession = @{}
  $deadParents = 0
  foreach ($c in $con) {
    $s = [string]$c.SessionId
    if (-not $bySession.ContainsKey($s)) { $bySession[$s] = 0 }
    $bySession[$s] = $bySession[$s] + 1
    $p = Get-CimInstance Win32_Process -Filter ("ProcessId=" + $c.ParentProcessId) -ErrorAction SilentlyContinue
    if (-not $p) { $deadParents = $deadParents + 1 }
  }
  $sessions = @{}
  Get-CimInstance Win32_Process | Group-Object SessionId | ForEach-Object { $sessions[[string]$_.Name] = $_.Count }
  return [ordered]@{
    at = (Get-Date).ToString('o')
    conhostTotal = @($con).Count
    conhostBySession = $bySession
    conhostDeadParent = $deadParents
    processesBySession = $sessions
    totalProcesses = @(Get-CimInstance Win32_Process).Count
    scheduleService = (Get-CimInstance Win32_Service -Filter "Name='Schedule'").State
  }
}

$res = [ordered]@{ tag=$Tag; revision='bc078f70 (staged from the clean candidate tree + verifier instruments)'; startedAt=(Get-Date).ToString('o') }
$res.preState = Snapshot
$res | ConvertTo-Json -Depth 10 | Set-Content $outPath -Encoding UTF8

$runLog = Join-Path $log 'runner.out'; $runErr = Join-Path $log 'runner.err'
$launcher = Start-Process -FilePath $node -ArgumentList @('scripts\qa\pane-liveness.mjs','--scenario','split-happy','--binary',$bin,'--evidence-dir',$ev,'--isolation-root',$iso) -PassThru -WindowStyle Hidden -WorkingDirectory $root -RedirectStandardOutput $runLog -RedirectStandardError $runErr
$res.launcherPid = $launcher.Id
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$done = $null
while ($sw.Elapsed.TotalSeconds -lt $TimeoutSec) {
  $evRoot = Join-Path $ev 'task-3-harness\split-happy'
  if (Test-Path $evRoot) {
    $d = @(Get-ChildItem $evRoot -Directory -Filter 'run-*' -ErrorAction SilentlyContinue | Where-Object { Test-Path (Join-Path $_.FullName 'result.json') } | Sort-Object LastWriteTime -Descending)
    if ($d.Count -gt 0) { $done = $d[0].FullName; break }
  }
  Start-Sleep -Milliseconds 800
}
$res.elapsedMs = $sw.ElapsedMilliseconds
$res.completedRunDir = $done
$res.postState = Snapshot

Write-Output ("=== markers log ===")
$mk = Join-Path $ev 'relaunch.markers.log'
$res.markerPath = $mk
$res.markerExists = Test-Path $mk
if (Test-Path $mk) {
  $res.markers = @(Get-Content $mk -ErrorAction SilentlyContinue)
  foreach ($l in $res.markers) { Write-Output ("  M " + $l) }
} else { Write-Output "  (no marker log)" }

Write-Output ("=== evidence dir files ===")
Get-ChildItem $ev -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  " + $_.FullName.Substring($ev.Length+1) + " [" + $_.Length + "]") }

Write-Output ("=== relaunch record ===")
$rr = Join-Path $ev 'windows-interactive-relaunch.json'
if (Test-Path $rr) { $j = Get-Content $rr -Raw | ConvertFrom-Json; Write-Output ("  taskName=" + $j.taskName + " batPath=" + $j.batPath + " markerPath=" + $j.markerPath) }

Write-Output ("=== inner relaunch files (out/err/exit) ===")
foreach ($f in @('relaunch.out','relaunch.err','relaunch.exit')) {
  $p = Join-Path $ev $f
  if (Test-Path $p) { Write-Output ("--- " + $f + " [" + (Get-Item $p).Length + "]"); Get-Content $p -Tail 25 -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("    " + $_) } }
}
Write-Output ("=== outer runner.err tail ===")
Get-Content $runErr -Tail 15 -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  " + $_) }
Write-Output ("=== outer runner.out tail ===")
Get-Content $runLog -Tail 20 -ErrorAction SilentlyContinue | ForEach-Object { Write-Output ("  " + $_) }

if ($done -and (Test-Path (Join-Path $done 'result.json'))) {
  $rj = Get-Content (Join-Path $done 'result.json') -Raw | ConvertFrom-Json
  $res.verdict = $rj.verdict; $res.code = $rj.error.code
  Copy-Item (Join-Path $done 'result.json') (Join-Path $ev 'runner-result.json') -Force -ErrorAction SilentlyContinue
  Copy-Item (Join-Path $done 'actions.jsonl') (Join-Path $ev 'runner-actions.jsonl') -Force -ErrorAction SilentlyContinue
}
$res | ConvertTo-Json -Depth 12 | Set-Content $outPath -Encoding UTF8
Write-Output ("RUN17_DONE verdict=" + $res.verdict + " code=" + $res.code + " markers=" + @($res.markers).Count)
