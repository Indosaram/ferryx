param(
  [string]$Scenario = 'split-happy',
  [string]$Tag = 'r1',
  [int]$TimeoutSec = 240
)
$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$bin  = Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$node = 'C:\Program Files\nodejs\node.exe'
$sink = Join-Path $base ('app-stderr-' + $Tag + '.log')
$hostProfile = Join-Path $env:APPDATA 'com.ferryx.app\dev\session_state.json'
$outPath = Join-Path $base ('run5c-' + $Tag + '.json')
$progPath = Join-Path $base ('run5c-' + $Tag + '.progress.log')
Remove-Item $outPath, $progPath, $sink -Force -ErrorAction SilentlyContinue

function Log([string]$m) { $l = (Get-Date).ToString('HH:mm:ss') + '  ' + $m; Add-Content -Path $progPath -Value $l -Encoding UTF8; Write-Output $l }
function HashOf([string]$p) { if (Test-Path $p) { (Get-FileHash $p -Algorithm SHA256).Hash } else { 'ABSENT' } }

$iso = Join-Path $base ('iso5c\' + $Tag)
$ev  = Join-Path $base ('ev5c\' + $Tag)
$log = Join-Path $base ('log5c\' + $Tag)
foreach ($d in @($iso, $ev, $log)) { if (Test-Path $d) { Remove-Item -Recurse -Force $d -ErrorAction SilentlyContinue } }
foreach ($d in @((Split-Path $iso -Parent), (Split-Path $ev -Parent), (Split-Path $log -Parent))) { New-Item -ItemType Directory -Force -Path $d | Out-Null }

$res = [ordered]@{
  tag = $Tag; scenario = $Scenario
  revisionStaged = '5c423880 (staged by tar; the staged copy has no .git)'
  startedAt = (Get-Date).ToString('o')
  hostProfileShaBefore = HashOf $hostProfile
  hostProfileMtimeBefore = if (Test-Path $hostProfile) { (Get-Item $hostProfile).LastWriteTime.ToString('o') } else { $null }
}
$res | ConvertTo-Json -Depth 10 | Set-Content $outPath -Encoding UTF8
Log ('RUN ' + $Tag + ' staged=' + $res.revisionStaged)

# The sink the instrumented spawnOwned drains the app's stdio into.
$env:FERRYX_VERIFIER_APP_STDERR = $sink
$res.sink = $sink

$runLog = Join-Path $log 'runner.out'; $runErr = Join-Path $log 'runner.err'
$launcher = Start-Process -FilePath $node -ArgumentList @('scripts\qa\pane-liveness.mjs','--scenario',$Scenario,'--binary',$bin,'--evidence-dir',$ev,'--isolation-root',$iso) -PassThru -WindowStyle Hidden -WorkingDirectory $root -RedirectStandardOutput $runLog -RedirectStandardError $runErr
$res.launcherPid = $launcher.Id
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$invSamples = New-Object System.Collections.ArrayList
$doneDir = $null
while ($sw.Elapsed.TotalSeconds -lt $TimeoutSec) {
  $evRoot = Join-Path $ev 'task-3-harness\split-happy'
  if (Test-Path $evRoot) {
    $d = @(Get-ChildItem $evRoot -Directory -Filter 'run-*' -ErrorAction SilentlyContinue | Where-Object { Test-Path (Join-Path $_.FullName 'result.json') } | Sort-Object LastWriteTime -Descending)
    if ($d.Count -gt 0) { $doneDir = $d[0].FullName; break }
  }
  $sessions = $null
  try {
    $pf = Join-Path $iso 'runtime\daemon.port'; $tf = Join-Path $iso 'runtime\daemon.token'
    if ((Test-Path $pf) -and (Test-Path $tf)) {
      $port = [int]((Get-Content $pf -Raw).Trim()); $token = (Get-Content $tf -Raw).Trim()
      $tc = New-Object System.Net.Sockets.TcpClient
      $tc.ReceiveTimeout = 1200; $tc.SendTimeout = 1200
      $iar = $tc.BeginConnect('127.0.0.1', $port, $null, $null)
      if ($iar.AsyncWaitHandle.WaitOne(1000)) {
        $tc.EndConnect($iar)
        $st = $tc.GetStream(); $st.ReadTimeout = 1200
        $w = New-Object System.IO.StreamWriter($st); $w.NewLine = [char]10; $w.AutoFlush = $true
        $rd = New-Object System.IO.StreamReader($st)
        $w.WriteLine('{"type":"handshake","version":5,"token":"' + $token + '"}')
        $null = $rd.ReadLine()
        $w.WriteLine('{"type":"listSessions"}')
        $line = $rd.ReadLine()
        $tc.Close()
        if ($line) { $sessions = @(($line | ConvertFrom-Json).sessions) }
      } else { $tc.Close() }
    }
  } catch { }
  if ($null -ne $sessions) { $invSamples.Add([ordered]@{ atMs=$sw.ElapsedMilliseconds; count=@($sessions).Count; sessions=@($sessions) }) | Out-Null }
  Start-Sleep -Milliseconds 500
}
$res.inventorySamples = @($invSamples)
$res.maxCountFromSamples = if (@($invSamples).Count -gt 0) { (@($invSamples) | Measure-Object -Property count -Maximum).Maximum } else { 0 }
$res.elapsedMs = $sw.ElapsedMilliseconds
$res.completedRunDir = $doneDir
# Post-run: enumerate every ferryx process (which daemon did the app use?)
$res.allFerryxProcesses = @(Get-CimInstance Win32_Process -Filter "Name='ferryx.exe'" -ErrorAction SilentlyContinue | ForEach-Object { [ordered]@{ pid=$_.ProcessId; ppid=$_.ParentProcessId; session=$_.SessionId; cmd=($_.CommandLine -replace '\s+',' ') } })
$res.barrierFiles = @(Get-ChildItem (Join-Path $iso 'barriers') -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
$res.isoDataLogs = @(Get-ChildItem (Join-Path $iso 'data') -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
Start-Sleep -Seconds 1
$res.hostProfileShaAfter = HashOf $hostProfile
$res.hostProfileMtimeAfter = if (Test-Path $hostProfile) { (Get-Item $hostProfile).LastWriteTime.ToString('o') } else { $null }
$res.isoSessionStateExists = Test-Path (Join-Path $iso 'session\session_state.json')
$res.isoSessionStatePath = Join-Path $iso 'session\session_state.json'
$res.isoSessionStateSha = HashOf (Join-Path $iso 'session\session_state.json')
$res.isoSessionStateMtime = if ($res.isoSessionStateExists) { (Get-Item $res.isoSessionStatePath).LastWriteTime.ToString('o') } else { $null }
$res.isoRemoteSessionsExists = Test-Path (Join-Path $iso 'session\remote_sessions.json')

if ($doneDir -and (Test-Path (Join-Path $doneDir 'result.json'))) {
  $rj = Get-Content (Join-Path $doneDir 'result.json') -Raw | ConvertFrom-Json
  $res.verdict = $rj.verdict; $res.code = $rj.error.code; $res.message = $rj.error.message
  Copy-Item (Join-Path $doneDir 'result.json') (Join-Path $ev 'runner-result.json') -Force -ErrorAction SilentlyContinue
  Copy-Item (Join-Path $doneDir 'actions.jsonl') (Join-Path $ev 'runner-actions.jsonl') -Force -ErrorAction SilentlyContinue
}
# The two launch.binary evidence records
$acts = Join-Path $ev 'runner-actions.jsonl'
$res.launchBinaryRecords = @()
if (Test-Path $acts) {
  foreach ($line in (Get-Content $acts -ErrorAction SilentlyContinue)) {
    try { $j = $line | ConvertFrom-Json; if ($j.action -eq 'launch.binary') { $res.launchBinaryRecords += ,$j } } catch { }
  }
}
$res.appStderrSinkExists = Test-Path $sink
$res.appStderrSink = @(Get-Content $sink -ErrorAction SilentlyContinue)
$res.daemonLog = @(Get-Content (Join-Path $iso 'data\logs\daemon.log') -ErrorAction SilentlyContinue)
$res.daemonLogExists = Test-Path (Join-Path $iso 'data\logs\daemon.log')
$res | ConvertTo-Json -Depth 12 | Set-Content $outPath -Encoding UTF8

$lp = Get-CimInstance Win32_Process -Filter ('ProcessId=' + $launcher.Id) -ErrorAction SilentlyContinue
if ($lp) { taskkill /T /F /PID $launcher.Id 2>&1 | Out-Null }
Log ('  -> verdict=' + $res.verdict + ' code=' + $res.code + ' hostShaBefore=' + $res.hostProfileShaBefore.Substring(0,12) + ' hostShaAfter=' + $res.hostProfileShaAfter.Substring(0,12) + ' isoState=' + $res.isoSessionStateExists + ' sinkBytes=' + (Get-Item $sink -ErrorAction SilentlyContinue).Length)
Set-Content -Path ($outPath + '.done') -Value 'DONE' -Encoding UTF8
Write-Output ('RUN5C_DONE ' + $outPath)
