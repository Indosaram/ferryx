param([int]$MaxAttempts = 6, [int]$MarkerWindowSec = 10, [int]$RunTimeoutSec = 90)
$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$bin  = Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$node = 'C:\Program Files\nodejs\node.exe'
$outPath = Join-Path $base 'pass19.json'
$prog = Join-Path $base 'pass19.progress.log'
Remove-Item $outPath, $prog -Force -ErrorAction SilentlyContinue
function Log([string]$m) { $l = (Get-Date).ToString('HH:mm:ss.fff') + '  ' + $m; Add-Content -Path $prog -Value $l -Encoding UTF8; Write-Output $l }
$res = [ordered]@{ revision='bc078f70 (frozen) + verifier instruments (ctx fixed, first-line marker, stamped probe)'; attempts=@() }
$res | ConvertTo-Json -Depth 10 | Set-Content $outPath -Encoding UTF8

for ($n = 1; $n -le $MaxAttempts; $n++) {
  $iso = Join-Path $base ('iso19\a' + $n)
  $ev  = Join-Path $base ('ev19\a' + $n)
  $log = Join-Path $base ('log19\a' + $n)
  foreach ($d in @($iso, $ev, $log)) { if (Test-Path $d) { Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue } }
  foreach ($d in @((Split-Path $iso -Parent), (Split-Path $ev -Parent), (Split-Path $log -Parent))) { New-Item -ItemType Directory -Force -Path $d | Out-Null }
  New-Item -ItemType Directory -Force -Path $log | Out-Null
  $launcher = Start-Process -FilePath $node -ArgumentList @('scripts\qa\pane-liveness.mjs','--scenario','split-happy','--binary',$bin,'--evidence-dir',$ev,'--isolation-root',$iso) -PassThru -WindowStyle Hidden -WorkingDirectory $root -RedirectStandardOutput (Join-Path $log 'runner.out') -RedirectStandardError (Join-Path $log 'runner.err')
  $att = [ordered]@{ attempt=$n; launcherPid=$launcher.Id; ev=$ev }
  $marker = Join-Path $ev 'relaunch.markers.log'
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  $markerSeen = $false; $verdict = 'STALLED'; $code = $null; $msg = $null; $probeTaken = $false
  while ($sw.Elapsed.TotalSeconds -lt $RunTimeoutSec) {
    if ((Test-Path $marker) -and -not $markerSeen) { $markerSeen = $true; $att.markerSeenMs = $sw.ElapsedMilliseconds }
    $rj = Join-Path $ev 'runner-result.json'
    if (Test-Path $rj) {
      $j = Get-Content $rj -Raw | ConvertFrom-Json
      $verdict = $j.verdict; $code = $j.error.code; $msg = $j.error.message
      break
    }
    # a stalled attempt: record appears but no marker within the window
    if ((Test-Path (Join-Path $ev 'windows-interactive-relaunch.json')) -and -not $markerSeen -and $sw.Elapsed.TotalSeconds -ge $MarkerWindowSec) {
      $att.stalledByMarkerWindow = $true
      break
    }
    Start-Sleep -Milliseconds 400
  }
  $att.elapsedMs = $sw.ElapsedMilliseconds; $att.markerSeen = $markerSeen
  $att.verdict = $verdict; $att.code = $code; $att.message = $msg
  $att.markers = @(Get-Content $marker -ErrorAction SilentlyContinue)
  $vp = Join-Path $ev 'verifier-probe.jsonl'
  if (Test-Path $vp) { $att.probeTaken = $true; $att.probeLines = @(Get-Content $vp) }
  # teardown by exact recorded PIDs
  $killed = New-Object System.Collections.ArrayList
  foreach ($p in @(Get-CimInstance Win32_Process -Filter "Name='ferryx.exe'" -ErrorAction SilentlyContinue)) { if ($p.ExecutablePath -eq $bin) { $killed.Add($p.ProcessId) | Out-Null; taskkill /T /F /PID $p.ProcessId 2>&1 | Out-Null } }
  foreach ($p in @(Get-CimInstance Win32_Process -Filter "Name='cmd.exe'" -ErrorAction SilentlyContinue)) { $c = ($p.CommandLine -replace '\s+',' '); if ($c -like '*ferryx-qa-relaunch*') { $killed.Add($p.ProcessId) | Out-Null; taskkill /T /F /PID $p.ProcessId 2>&1 | Out-Null } }
  foreach ($p in @(Get-CimInstance Win32_Process -Filter "Name='node.exe'" -ErrorAction SilentlyContinue)) { $c = ($p.CommandLine -replace '\s+',' '); if ($c -like '*pane-liveness*') { $killed.Add($p.ProcessId) | Out-Null; taskkill /T /F /PID $p.ProcessId 2>&1 | Out-Null } }
  $live = Get-CimInstance Win32_Process -Filter ("ProcessId=" + $launcher.Id) -ErrorAction SilentlyContinue
  if ($live) { $killed.Add($launcher.Id) | Out-Null; taskkill /T /F /PID $launcher.Id 2>&1 | Out-Null }
  $att.killedPids = @($killed)
  $t = (& schtasks /query /fo CSV /nh 2>&1) | Select-String -Pattern 'ferryx-qa-split-happy'
  foreach ($x in $t) { $nm = ($x -split ',')[0].Trim('"','\'); & schtasks /end /tn $nm 2>&1 | Out-Null; & schtasks /delete /tn $nm /f 2>&1 | Out-Null }
  $res.attempts = @($res.attempts) + @($att)
  $res | ConvertTo-Json -Depth 10 | Set-Content $outPath -Encoding UTF8
  Log ('attempt ' + $n + ' verdict=' + $verdict + ' code=' + $code + ' markerSeen=' + $markerSeen + ' atMs=' + $att.markerSeenMs + ' probeTaken=' + $att.probeTaken + ' elapsedMs=' + $att.elapsedMs)
  if ($verdict -ne 'STALLED') { Log ('  attempt ' + $n + ' produced a verdict - stopping'); break }
}
$res.completed = @($res.attempts | Where-Object { $_.verdict -ne 'STALLED' }).Count
$res.stalled = @($res.attempts | Where-Object { -not $_.markerSeen }).Count
$res | ConvertTo-Json -Depth 10 | Set-Content $outPath -Encoding UTF8
Log ('DONE attempts=' + @($res.attempts).Count + ' completed=' + $res.completed + ' stalled=' + $res.stalled)
Write-Output 'PASS18C_DONE'
