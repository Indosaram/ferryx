param(
  [string]$Scenario = 'split-happy',
  [string]$Out = 'C:\Users\sook\ferryx-pane-completion\pane-delta.json',
  [int]$TimeoutSec = 300
)
$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$bin  = Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$node = 'C:\Program Files\nodejs\node.exe'
$iso  = Join-Path $base ('pd-iso\' + $Scenario)
$ev   = Join-Path $base ('pd-ev\' + $Scenario)
$log  = Join-Path $base ('pd-logs\' + $Scenario)
foreach ($d in @($log, (Split-Path $iso -Parent), (Split-Path $ev -Parent))) { New-Item -ItemType Directory -Force -Path $d | Out-Null }
if (Test-Path $iso) { Remove-Item -Recurse -Force $iso -ErrorAction SilentlyContinue }
if (Test-Path $ev) { Remove-Item -Recurse -Force $ev -ErrorAction SilentlyContinue }

$res = [ordered]@{ scenario=$Scenario; session=(Get-Process -Id $PID).SessionId; interactive=[bool][System.Environment]::UserInteractive; startedAt=(Get-Date).ToString('o') }
$portFile = Join-Path $iso 'runtime\daemon.port'
$tokenFile = Join-Path $iso 'runtime\daemon.token'
$barrierDir = Join-Path $iso 'barriers'

function ListSessions([string]$rt) {
  try {
    $pf = Join-Path $rt 'daemon.port'; $tf = Join-Path $rt 'daemon.token'
    if (-not (Test-Path $pf)) { return $null }
    $port = [int]((Get-Content $pf -Raw).Trim()); $token = (Get-Content $tf -Raw).Trim()
    $c = New-Object System.Net.Sockets.TcpClient
    $c.Connect('127.0.0.1', $port)
    $s = $c.GetStream()
    $w = New-Object System.IO.StreamWriter($s); $w.NewLine = "`n"; $w.AutoFlush = $true
    $r = New-Object System.IO.StreamReader($s)
    $w.WriteLine('{"type":"handshake","version":5,"token":"' + $token + '"}')
    $null = $r.ReadLine()
    $w.WriteLine('{"type":"listSessions"}')
    $line = $r.ReadLine()
    $c.Close()
    $j = $line | ConvertFrom-Json
    return @($j.sessions)
  } catch { return $null }
}

$samples = New-Object System.Collections.ArrayList
$runLog = Join-Path $log 'runner.out'
$runErr = Join-Path $log 'runner.err'
$runner = Start-Process -FilePath $node -ArgumentList @('scripts\qa\pane-liveness.mjs','--scenario',$Scenario,'--binary',$bin,'--evidence-dir',$ev,'--isolation-root',$iso) -PassThru -WindowStyle Hidden -WorkingDirectory $root -RedirectStandardOutput $runLog -RedirectStandardError $runErr
$res.runnerPid = $runner.Id
$sw = [System.Diagnostics.Stopwatch]::StartNew()

$preCaptured = $false
while ($sw.Elapsed.TotalSeconds -lt $TimeoutSec) {
  $rt = Join-Path $iso 'runtime'
  $sessions = ListSessions $rt
  if ($null -ne $sessions) {
    $samples.Add([ordered]@{ atMs=$sw.ElapsedMilliseconds; count=$sessions.Count; sessions=@($sessions) }) | Out-Null
    if (-not $preCaptured) { $preCaptured = $true }
  }
  if ($runner.HasExited) { break }
  Start-Sleep -Milliseconds 700
}
$res.runnerExited = $runner.HasExited
$res.elapsedMs = $sw.ElapsedMilliseconds
if (-not $runner.HasExited) { try { taskkill /T /F /PID $runner.Id 2>&1 | Out-Null } catch {} }

# One final sample after the runner settles.
$final = ListSessions (Join-Path $iso 'runtime')
$res.samples = @($samples)
$res.finalSessions = @($final)
$res.maxCount = ($samples | Measure-Object -Property count -Maximum).Maximum
$res.firstCount = if ($samples.Count -gt 0) { $samples[0].count } else { $null }

$rj = Get-ChildItem -Recurse -File $ev -Filter result.json -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
if ($rj) { $j = Get-Content $rj.FullName -Raw | ConvertFrom-Json; $res.verdict = $j.verdict; $res.code = $j.error.code; $res.errorMessage = $j.error.message }
$a = Get-ChildItem -Recurse -File $ev -Filter actions.jsonl -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
if ($a) {
  $objs = Get-Content $a.FullName | ForEach-Object { $_ | ConvertFrom-Json }
  $res.actions = @($objs | ForEach-Object { $_.action })
  $launch = $objs | Where-Object { $_.action -eq 'launch.binary' } | Select-Object -First 1
  if ($launch) {
    $t0 = [datetime]$launch.at
    $res.timeline = @($objs | Where-Object { $_.action -in @('launch.binary','fixture-setup','owned-windows-enumerated','click-pane-affordance','pane-session-bound') } | ForEach-Object { [ordered]@{ atMs=[int](([datetime]$_.at - $t0).TotalMilliseconds); action=$_.action; code=$_.code } })
  }
  $click = $objs | Where-Object { $_.action -eq 'click-pane-affordance' } | Select-Object -Last 1
  if ($click) { $res.paneClick = [ordered]@{ code=$click.code; candidateCount=$click.candidateCount; actionableCount=$click.actionableCount; candidates=$click.candidates; chosen=$click.chosen } }
}
$res.receipts = @(Get-ChildItem $barrierDir -File -ErrorAction SilentlyContinue | ForEach-Object { $_.Name })
$res.allReceipts = @{}
foreach ($f in (Get-ChildItem $barrierDir -File -Filter '*.jsonl' -ErrorAction SilentlyContinue)) { $res.allReceipts[$f.Name] = (Get-Content $f.FullName -Raw) }
$res.workspaceFile = if (Test-Path (Join-Path $iso 'data\remote\machine-workspaces.v1.json')) { (Get-Content (Join-Path $iso 'data\remote\machine-workspaces.v1.json') -Raw) } else { 'ABSENT' }
$res.daemonLogTail = ((Get-Content (Join-Path $iso 'data\logs\daemon.log') -Tail 60) -join "`n")
$res.finishedAt = (Get-Date).ToString('o')
$res | ConvertTo-Json -Depth 10 | Set-Content $Out -Encoding utf8
Set-Content -Path ($Out + '.done') -Value 'done' -Encoding ascii
Write-Output "PANE_DELTA_DONE"
