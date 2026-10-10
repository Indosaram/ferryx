param(
  [int]$ProbeCount = 12,
  [int]$MarkerWindowSec = 10,
  [int]$MaxAttempts = 6,
  [int]$RunTimeoutSec = 150
)
$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$bin  = Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$node = 'C:\Program Files\nodejs\node.exe'
$outPath = Join-Path $base 'pass18.json'
$prog = Join-Path $base 'pass18.progress.log'
Remove-Item $outPath, $prog -Force -ErrorAction SilentlyContinue
function Log([string]$m) { $l = (Get-Date).ToString('HH:mm:ss.fff') + '  ' + $m; Add-Content -Path $prog -Value $l -Encoding UTF8; Write-Output $l }
function Save { $res | ConvertTo-Json -Depth 12 | Set-Content $outPath -Encoding UTF8 }

$res = [ordered]@{ revision='bc078f70 (candidate tree + verifier instruments)'; startedAt=(Get-Date).ToString('o'); probes=@(); attempts=@() }
$res | ConvertTo-Json -Depth 12 | Set-Content $outPath -Encoding UTF8

# ---------------------------------------------------------------- PART 1: rate sample
Log ('PART1 probes=' + $ProbeCount + ' markerWindowSec=' + $MarkerWindowSec)
$pdir = Join-Path $base 'p18'
New-Item -ItemType Directory -Force -Path $pdir | Out-Null
for ($i = 1; $i -le $ProbeCount; $i++) {
  $tag = 'p' + $i
  $marker = Join-Path $pdir ($tag + '.markers.log')
  $out = Join-Path $pdir ($tag + '.out.txt')
  Remove-Item $marker, $out -Force -ErrorAction SilentlyContinue
  $bat = Join-Path $pdir ($tag + '.bat')
  $body = '@echo off' + [char]13 + [char]10 +
          'echo [' + '%TIME%' + '] 00_FIRST_LINE >> "' + $marker + '"' + [char]13 + [char]10 +
          'echo PROBE_OK > "' + $out + '"' + [char]13 + [char]10 +
          'echo [' + '%TIME%' + '] 01_DONE >> "' + $marker + '"' + [char]13 + [char]10
  Set-Content -Path $bat -Value $body -Encoding ASCII
  $task = 'ferryx-p18-' + $tag
  & schtasks /delete /tn $task /f 2>&1 | Out-Null
  & schtasks /create /tn $task /tr $bat /sc once /st 00:00 /f /it 2>&1 | Out-Null
  $t0 = Get-Date
  & schtasks /run /tn $task 2>&1 | Out-Null
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  while ($sw.Elapsed.TotalSeconds -lt $MarkerWindowSec -and -not (Test-Path $marker)) { Start-Sleep -Milliseconds 250 }
  $markerMs = $sw.ElapsedMilliseconds
  $gotMarker = Test-Path $marker
  # let a started one finish
  if ($gotMarker) {
    while ($sw.Elapsed.TotalSeconds -lt ($MarkerWindowSec + 15) -and -not (Test-Path $out)) { Start-Sleep -Milliseconds 250 }
  }
  $ok = Test-Path $out
  # the task's own cmd.exe PID, recorded for exact-PID teardown
  $cmdPid = $null
  Get-CimInstance Win32_Process -Filter "Name='cmd.exe'" | ForEach-Object { $c = ($_.CommandLine -replace '\s+',' '); if ($c -like ('*' + $tag + '.bat*')) { $cmdPid = $_.ProcessId } }
  $res.probes = @($res.probes) + @([ordered]@{
    tag=$tag; markerAppearedMs=($(if ($gotMarker) { $markerMs } else { $null })); markerAppeared=$gotMarker;
    completed=$ok; taskCmdPid=$cmdPid; at=(Get-Date).ToString('o')
  })
  Save
  Log ('  probe ' + $tag + ' marker=' + $gotMarker + ' atMs=' + $markerMs + ' completed=' + $ok + ' cmdPid=' + $cmdPid)
  & schtasks /end /tn $task 2>&1 | Out-Null
  & schtasks /delete /tn $task /f 2>&1 | Out-Null
  if ($cmdPid) { taskkill /T /F /PID $cmdPid 2>&1 | Out-Null }
}
$stalled = @($res.probes | Where-Object { -not $_.markerAppeared }).Count
$res.probeSummary = [ordered]@{ total=$ProbeCount; stalled=$stalled; started=($ProbeCount - $stalled); stallRate=([math]::Round($stalled / $ProbeCount, 3)) }
Save
Log ('PART1 summary: stalled=' + $stalled + '/' + $ProbeCount + ' rate=' + $res.probeSummary.stallRate)

# ------------------------------------------------- PART 2: retrying full runner attempts
Log ('PART2 maxAttempts=' + $MaxAttempts)
for ($n = 1; $n -le $MaxAttempts; $n++) {
  $iso = Join-Path $base ('iso18\a' + $n)
  $ev  = Join-Path $base ('ev18\a' + $n)
  $log = Join-Path $base ('log18\a' + $n)
  foreach ($d in @($iso, $ev, $log)) { if (Test-Path $d) { Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue } }
  foreach ($d in @((Split-Path $iso -Parent), (Split-Path $ev -Parent), (Split-Path $log -Parent))) { New-Item -ItemType Directory -Force -Path $d | Out-Null }
  New-Item -ItemType Directory -Force -Path $log | Out-Null
  $runLog = Join-Path $log 'runner.out'; $runErr = Join-Path $log 'runner.err'
  $launcher = Start-Process -FilePath $node -ArgumentList @('scripts\qa\pane-liveness.mjs','--scenario','split-happy','--binary',$bin,'--evidence-dir',$ev,'--isolation-root',$iso) -PassThru -WindowStyle Hidden -WorkingDirectory $root -RedirectStandardOutput $runLog -RedirectStandardError $runErr
  $att = [ordered]@{ attempt=$n; launcherPid=$launcher.Id; iso=$iso; ev=$ev; startedAt=(Get-Date).ToString('o') }
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  $marker = Join-Path $ev 'relaunch.markers.log'
  $relaunchRecord = Join-Path $ev 'windows-interactive-relaunch.json'
  $doneDir = $null; $verdict = 'STALLED'
  $sawRecord = $false; $markerSeen = $false
  while ($sw.Elapsed.TotalSeconds -lt $RunTimeoutSec) {
    if ((Test-Path $relaunchRecord) -and -not $sawRecord) { $sawRecord = $true; $att.relaunchRecordMs = $sw.ElapsedMilliseconds }
    if ((Test-Path $marker) -and -not $markerSeen) { $markerSeen = $true; $att.markerSeenMs = $sw.ElapsedMilliseconds }
    $evRoot = Join-Path $ev 'task-3-harness\split-happy'
    if (Test-Path $evRoot) {
      $d = @(Get-ChildItem $evRoot -Directory -Filter 'run-*' -ErrorAction SilentlyContinue | Where-Object { Test-Path (Join-Path $_.FullName 'result.json') } | Sort-Object LastWriteTime -Descending)
      if ($d.Count -gt 0) { $doneDir = $d[0].FullName; break }
    }
    Start-Sleep -Milliseconds 500
  }
  $att.elapsedMs = $sw.ElapsedMilliseconds
  $att.markerSeen = $markerSeen
  $att.relaunchRecordSeen = $sawRecord
  $att.markers = @(Get-Content $marker -ErrorAction SilentlyContinue)
  if ($doneDir) {
    $rj = Get-Content (Join-Path $doneDir 'result.json') -Raw | ConvertFrom-Json
    $att.verdict = $rj.verdict; $att.code = $rj.error.code; $att.message = $rj.error.message
    Copy-Item (Join-Path $doneDir 'result.json') (Join-Path $ev 'runner-result.json') -Force -ErrorAction SilentlyContinue
    Copy-Item (Join-Path $doneDir 'actions.jsonl') (Join-Path $ev 'runner-actions.jsonl') -Force -ErrorAction SilentlyContinue
    Copy-Item (Join-Path $doneDir 'app.stderr.log') (Join-Path $ev 'app.stderr.log') -Force -ErrorAction SilentlyContinue
    Copy-Item (Join-Path $doneDir 'app.stdout.log') (Join-Path $ev 'app.stdout.log') -Force -ErrorAction SilentlyContinue
    $vp = Join-Path $doneDir 'verifier-probe.jsonl'
    if (Test-Path $vp) { Copy-Item $vp (Join-Path $ev 'verifier-probe.jsonl') -Force -ErrorAction SilentlyContinue; $att.probeTaken = $true } else { $att.probeTaken = $false }
  }
  # teardown this attempt by exact recorded PIDs
  $killed = New-Object System.Collections.ArrayList
  foreach ($p in @(Get-CimInstance Win32_Process -Filter "Name='ferryx.exe'" -ErrorAction SilentlyContinue)) { if ($p.ExecutablePath -eq $bin) { $killed.Add($p.ProcessId) | Out-Null; taskkill /T /F /PID $p.ProcessId 2>&1 | Out-Null } }
  foreach ($p in @(Get-CimInstance Win32_Process -Filter "Name='cmd.exe'" -ErrorAction SilentlyContinue)) { $c = ($p.CommandLine -replace '\s+',' '); if ($c -like '*ferryx-qa-relaunch*') { $killed.Add($p.ProcessId) | Out-Null; taskkill /T /F /PID $p.ProcessId 2>&1 | Out-Null } }
  foreach ($p in @(Get-CimInstance Win32_Process -Filter "Name='node.exe'" -ErrorAction SilentlyContinue)) { $c = ($p.CommandLine -replace '\s+',' '); if ($c -like '*pane-liveness*') { $killed.Add($p.ProcessId) | Out-Null; taskkill /T /F /PID $p.ProcessId 2>&1 | Out-Null } }
  $live = Get-CimInstance Win32_Process -Filter ("ProcessId=" + $launcher.Id) -ErrorAction SilentlyContinue
  if ($live) { $killed.Add($launcher.Id) | Out-Null; taskkill /T /F /PID $launcher.Id 2>&1 | Out-Null }
  $att.killedPids = @($killed)
  # stop+delete any task this attempt created
  $t = (& schtasks /query /fo CSV /nh 2>&1) | Select-String -Pattern 'ferryx-qa-split-happy'
  foreach ($x in $t) { $nm = ($x -split ',')[0].Trim('"','\'); & schtasks /end /tn $nm 2>&1 | Out-Null; & schtasks /delete /tn $nm /f 2>&1 | Out-Null }
  $res.attempts = @($res.attempts) + @($att)
  Save
  Log ('  attempt ' + $n + ' verdict=' + $att.verdict + ' code=' + $att.code + ' markerSeen=' + $markerSeen + ' atMs=' + $att.markerSeenMs + ' probeTaken=' + $att.probeTaken + ' elapsedMs=' + $att.elapsedMs)
  if ($doneDir) { Log ('  ATTEMPT ' + $n + ' COMPLETED - stopping retries'); break }
  if (-not $sawRecord) { Log ('  attempt ' + $n + ' produced no relaunch record at all') }
}
$res.completedAttempts = @($res.attempts | Where-Object { $_.verdict -and $_.verdict -ne 'STALLED' }).Count
$res.stalledAttempts = @($res.attempts | Where-Object { -not $_.markerSeen }).Count
Save
Log ('PART2 done: attempts=' + @($res.attempts).Count + ' stalled=' + $res.stalledAttempts + ' completed=' + $res.completedAttempts)
Write-Output 'PASS18_DONE'
