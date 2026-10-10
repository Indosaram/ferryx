param(
  [string]$Scenario = 'split-happy',
  [int]$ControlRepeats = 2,
  [int]$IsoRepeats = 2,
  [int]$TimeoutSec = 240
)
$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$bin  = Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$node = 'C:\Program Files\nodejs\node.exe'
$classifier = Join-Path $root 'scripts\lib\qa-scenarios\diagnostic-classifier.mjs'
$classifierOrig = Join-Path $base 'diagnostic-classifier.orig.mjs'
$classifierIso  = Join-Path $base 'diagnostic-classifier.session.mjs'
$outPath = Join-Path $base 'pane-delta12.json'
$progPath = Join-Path $base 'pane-delta12.progress.log'
$hostState = Join-Path $env:APPDATA 'com.ferryx.app\dev\session_state.json'

function Log([string]$m) { $line = (Get-Date).ToString('HH:mm:ss') + "  " + $m; Add-Content -Path $progPath -Value $line -Encoding UTF8; Write-Output $line }
function StateOf([string]$p) { if (Test-Path $p) { $f=Get-Item $p; return @{ exists=$true; size=$f.Length; mtime=$f.LastWriteTime.ToString('o') } } else { return @{ exists=$false; size=0; mtime=$null } } }

function ListSessions([string]$iso) {
  try {
    $rt = Join-Path $iso 'runtime'
    $pf = Join-Path $rt 'daemon.port'; $tf = Join-Path $rt 'daemon.token'
    if (-not (Test-Path $pf)) { return $null }
    $port = [int]((Get-Content $pf -Raw).Trim()); $token = (Get-Content $tf -Raw).Trim()
    $c = New-Object System.Net.Sockets.TcpClient
    $c.ReceiveTimeout = 1500; $c.SendTimeout = 1500
    $iar = $c.BeginConnect('127.0.0.1', $port, $null, $null)
    if (-not $iar.AsyncWaitHandle.WaitOne(1200)) { $c.Close(); return $null }
    $c.EndConnect($iar)
    $st = $c.GetStream(); $st.ReadTimeout = 1500; $st.WriteTimeout = 1500
    $w = New-Object System.IO.StreamWriter($st); $w.NewLine = [char]10; $w.AutoFlush = $true
    $rd = New-Object System.IO.StreamReader($st)
    $w.WriteLine('{"type":"handshake","version":5,"token":"' + $token + '"}')
    $null = $rd.ReadLine()
    $w.WriteLine('{"type":"listSessions"}')
    $line = $rd.ReadLine()
    $c.Close()
    if (-not $line) { return $null }
    $j = $line | ConvertFrom-Json
    return @($j.sessions)
  } catch { return $null }
}

# The runner DELEGATES to a scheduled task in the interactive session when started
# from SSH session 0, so the launcher process stays alive after the work is done.
# Completion is therefore the runner's own artifact, never the launcher's exit.
function CompletedRunDir([string]$ev) {
  $evRoot = Join-Path $ev 'task-3-harness\split-happy'
  if (-not (Test-Path $evRoot)) { return $null }
  $d = @(Get-ChildItem $evRoot -Directory -Filter 'run-*' -ErrorAction SilentlyContinue | Where-Object { Test-Path (Join-Path $_.FullName 'result.json') } | Sort-Object LastWriteTime -Descending)
  if ($d.Count -eq 0) { return $null }
  return $d[0].FullName
}

Remove-Item $progPath -Force -ErrorAction SilentlyContinue
$out = [ordered]@{ scenario=$Scenario; startedAt=(Get-Date).ToString('o'); scriptSession=(Get-Process -Id $PID).SessionId; scriptInteractive=[bool][System.Environment]::UserInteractive; runs=@() }
$out | ConvertTo-Json -Depth 14 | Set-Content $outPath -Encoding UTF8
$n = 0
foreach ($plan in @(@{tag='control'; iso=$false; repeats=$ControlRepeats}, @{tag='sessioniso'; iso=$true; repeats=$IsoRepeats})) {
  if ($plan.repeats -le 0) { continue }
  $src = if ($plan.iso) { $classifierIso } else { $classifierOrig }
  for ($r = 1; $r -le $plan.repeats; $r++) {
    $n = $n + 1
    Copy-Item $src $classifier -Force
    $sha = (Get-FileHash $classifier -Algorithm SHA256).Hash.Substring(0,16)
    Log ("RUN " + $n + " tag=" + $plan.tag + " classifierSha=" + $sha)
    $iso = Join-Path $base ('pd12-iso\' + $Scenario + '\' + $plan.tag + '-run' + $r)
    $ev  = Join-Path $base ('pd12-ev\' + $Scenario + '\' + $plan.tag + '-run' + $r)
    $log = Join-Path $base ('pd12-logs\' + $Scenario + '\' + $plan.tag + '-run' + $r)
    foreach ($d in @($log, (Split-Path $iso -Parent), (Split-Path $ev -Parent), (Split-Path $log -Parent))) { New-Item -ItemType Directory -Force -Path $d | Out-Null }
    foreach ($d in @($iso, $ev, $log)) { if (Test-Path $d) { Remove-Item -Recurse -Force $d -ErrorAction SilentlyContinue } }
    foreach ($f in @('pane-dump-new-pane.json','pane-dump-split-right.json')) { $p = Join-Path $base $f; if (Test-Path $p) { Remove-Item -Force $p -ErrorAction SilentlyContinue } }
    New-Item -ItemType Directory -Force -Path $log | Out-Null

    $entry = [ordered]@{ index=$n; variant=[ordered]@{ tag=$plan.tag; sessionIsolation=$plan.iso; classifierSha=$sha }; iso=$iso; ev=$ev; startedAt=(Get-Date).ToString('o') }
    $entry.hostSessionStateBefore = StateOf $hostState
    $samples = New-Object System.Collections.ArrayList
    $appPids = New-Object System.Collections.ArrayList
    $runLog = Join-Path $log 'runner.out'; $runErr = Join-Path $log 'runner.err'
    $launcher = Start-Process -FilePath $node -ArgumentList @('scripts\qa\pane-liveness.mjs','--scenario',$Scenario,'--binary',$bin,'--evidence-dir',$ev,'--isolation-root',$iso) -PassThru -WindowStyle Hidden -WorkingDirectory $root -RedirectStandardOutput $runLog -RedirectStandardError $runErr
    $entry.launcherPid = $launcher.Id
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $doneDir = $null
    while ($sw.Elapsed.TotalSeconds -lt $TimeoutSec) {
      $sessions = ListSessions $iso
      if ($null -ne $sessions) { $samples.Add([ordered]@{ atMs=$sw.ElapsedMilliseconds; count=$sessions.Count; sessions=@($sessions) }) | Out-Null }
      foreach ($proc in @(Get-CimInstance Win32_Process -Filter "Name='ferryx.exe'" -ErrorAction SilentlyContinue)) {
        if ($proc.ExecutablePath -eq $bin) {
          $known = $false; foreach ($a in $appPids) { if ($a.pid -eq $proc.ProcessId) { $known = $true } }
          if (-not $known) { $appPids.Add([ordered]@{ pid=$proc.ProcessId; ppid=$proc.ParentProcessId; cmd=($proc.CommandLine -replace '\s+',' '); startedAt=$proc.CreationDate.ToString('o'); exe=$proc.ExecutablePath }) | Out-Null }
        }
      }
      $doneDir = CompletedRunDir $ev
      if ($doneDir) { break }
      Start-Sleep -Milliseconds 600
    }
    $entry.completedRunDir = $doneDir
    $entry.elapsedMs = $sw.ElapsedMilliseconds
    Start-Sleep -Milliseconds 800
    $entry.hostSessionStateAfter = StateOf $hostState
    $entry.isoSessionState = StateOf (Join-Path $iso 'session\session_state.json')
    $entry.appProcesses = @($appPids)
    $entry.samples = @($samples)
    $mx = 0; foreach ($s in $samples) { if ($s.count -gt $mx) { $mx = $s.count } }
    $entry.maxCountFromSamples = $mx
    $entry.sampleCount = @($samples).Count
    if ($doneDir -and (Test-Path (Join-Path $doneDir 'result.json'))) {
      $rj = Get-Content (Join-Path $doneDir 'result.json') -Raw -ErrorAction SilentlyContinue | ConvertFrom-Json
      $entry.runnerVerdict = $rj.verdict
      $entry.runnerCode = $rj.error.code
      $entry.runnerMessage = $rj.error.message
      Copy-Item (Join-Path $doneDir 'result.json') (Join-Path $ev 'runner-result.json') -Force -ErrorAction SilentlyContinue
      Copy-Item (Join-Path $doneDir 'actions.jsonl') (Join-Path $ev 'runner-actions.jsonl') -Force -ErrorAction SilentlyContinue
    }
    $dumps = New-Object System.Collections.ArrayList
    foreach ($probe in @('new-pane','split-right')) {
      $sp = Join-Path $base ('pane-dump-' + $probe + '.json')
      if (Test-Path $sp) { Copy-Item $sp (Join-Path $ev ('pane-dump-' + $probe + '.json')) -Force -ErrorAction SilentlyContinue; $dumps.Add($probe) | Out-Null }
    }
    $entry.dumpsCaptured = @($dumps)
    # The launcher is my own recorded child; kill by exact PID.
    $lp = Get-CimInstance Win32_Process -Filter ("ProcessId=" + $launcher.Id) -ErrorAction SilentlyContinue
    if ($lp) { $entry.launcherKilled = $true; taskkill /T /F /PID $launcher.Id 2>&1 | Out-Null } else { $entry.launcherKilled = $false }
    $out.runs = @($out.runs) + @($entry)
    $out | ConvertTo-Json -Depth 14 | Set-Content $outPath -Encoding UTF8
    Log ("  -> verdict=" + $entry.runnerVerdict + " code=" + $entry.runnerCode + " elapsedMs=" + $entry.elapsedMs + " maxCount=" + $mx + " dumps=" + (@($dumps) -join ',') + " hostBefore=" + $entry.hostSessionStateBefore.mtime + " hostAfter=" + $entry.hostSessionStateAfter.mtime + " isoStateExists=" + $entry.isoSessionState.exists)
  }
}
Copy-Item $classifierOrig $classifier -Force
Log "RESTORED classifier to original"
Set-Content -Path ($outPath + '.done') -Value 'DONE' -Encoding UTF8
Write-Output 'PANE_DELTA12_DONE'
