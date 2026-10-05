param([string]$Out = 'C:\Users\sook\ferryx-pane-completion\pf-result.json')
$ErrorActionPreference = 'Continue'
$base  = 'C:\Users\sook\ferryx-pane-completion'
$root  = Join-Path $base 'source-21dea3c0'
$bin   = Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$dist  = Join-Path $root 'ui\dist'
$gen   = Join-Path $root 'scripts\qa\pf-gen.mjs'
$tree  = Join-Path $base 'exp-tree.ps1'
$iso   = Join-Path $base 'pf-iso'
$bun   = 'C:\Users\sook\.bun\bin\bun.exe'
$node  = 'C:\Program Files\nodejs\node.exe'
$res = [ordered]@{ session=(Get-Process -Id $PID).SessionId; startedAt=(Get-Date).ToString('o'); stage='init'; errors=@() }
function Flush { try { $res | ConvertTo-Json -Depth 12 | Set-Content -Path $Out -Encoding utf8 } catch {} }

# Native daemon query over TCP — no node, no pipeline capture.
function DaemonQuery([string]$rt, [string]$cmd) {
  try {
    $port = [int]((Get-Content (Join-Path $rt 'daemon.port') -Raw).Trim())
    $token = (Get-Content (Join-Path $rt 'daemon.token') -Raw).Trim()
    $c = New-Object System.Net.Sockets.TcpClient
    $c.Connect('127.0.0.1', $port)
    $s = $c.GetStream()
    $w = New-Object System.IO.StreamWriter($s)
    $w.NewLine = "`n"
    $w.AutoFlush = $true
    $r = New-Object System.IO.StreamReader($s)
    $w.WriteLine('{"type":"handshake","version":5,"token":"' + $token + '"}')
    $h = $r.ReadLine()
    $w.WriteLine('{"type":"listSessions"}')
    $l = $r.ReadLine()
    $c.Close()
    return ('{"handshake":' + $h + ',"response":' + $l + '}')
  } catch { return ('{"error":"' + $_.Exception.Message + '"}') }
}

Flush
try {
  if (Test-Path $iso) { Remove-Item -Recurse -Force $iso -ErrorAction SilentlyContinue }
  foreach ($d in @('data','runtime','home','barriers')) { New-Item -ItemType Directory -Force -Path (Join-Path $iso $d) | Out-Null }
  $runtimeDir = Join-Path $iso 'runtime'; $barrierDir = Join-Path $iso 'barriers'
  $res.isolationRoot = $iso
  $srvLog = Join-Path $base 'pf-server.log'
  $srvProc = Start-Process -FilePath $bun -ArgumentList @((Join-Path $base 'serve-dist.mjs'), $dist) -PassThru -WindowStyle Hidden -RedirectStandardOutput $srvLog -RedirectStandardError ($srvLog + '.err')
  $res.serverPid = $srvProc.Id
  foreach ($i in 1..24) { Start-Sleep -Milliseconds 500; if (Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue) { break } }
  $res.serverListening = [bool](Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue)
  $res.stage='server-up'; Flush
  $env:FERRYX_DATA_DIR = Join-Path $iso 'data'; $env:FERRYX_RUNTIME_DIR = $runtimeDir
  $env:FERRYX_QA_BARRIER_DIR = $barrierDir; $env:FERRYX_QA_RUN_ID = 'qa-run-pf'
  $env:FERRYX_QA_OPERATION_ID = 'qa-op-pf'; $env:FERRYX_QA_FIXTURE_KINDS = 'source'
  $app = Start-Process -FilePath $bin -PassThru -WorkingDirectory $root
  $res.appPid = $app.Id
  $res.stage='app-launched'; Flush
  $sw=[System.Diagnostics.Stopwatch]::StartNew(); $visible=0
  $deadline=(Get-Date).AddSeconds(45)
  while ((Get-Date) -lt $deadline) {
    if (-not (Get-Process -Id $app.Id -ErrorAction SilentlyContinue)) { break }
    $raw = & powershell -NoProfile -ExecutionPolicy Bypass -File $tree -TargetPid $app.Id -Cap 1 2>&1 | Out-String
    $m = [regex]::Match($raw, '"visibleWindows":(\d+)')
    if ($m.Success -and [int]$m.Groups[1].Value -gt 0) { $visible=[int]$m.Groups[1].Value; break }
    Start-Sleep -Milliseconds 500
  }
  $res.visibleWindowCount=$visible; $res.firstVisibleMs=$sw.ElapsedMilliseconds
  $res.appAlive=[bool](Get-Process -Id $app.Id -ErrorAction SilentlyContinue)
  $res.stage='window-wait-done'; Flush
  if ($visible -ge 1 -and $res.appAlive) {
    Start-Sleep -Seconds 4
    $res.stage='pre-inventory'; Flush
    $res.preInventory = DaemonQuery $runtimeDir 'list'
    $wsFile = Join-Path $iso 'data\remote\machine-workspaces.v1.json'
    $res.workspaceFileBefore = if (Test-Path $wsFile) { (Get-Content $wsFile -Raw) } else { 'ABSENT' }
    $res.receiptFilesBefore = @(Get-ChildItem $barrierDir -File -ErrorAction SilentlyContinue | ForEach-Object { $_.Name })
    # enumerate windows, generate the click script with the driver's own builder
    $enum = & powershell -NoProfile -ExecutionPolicy Bypass -File $tree -TargetPid $app.Id -Cap 1 2>&1 | Out-String
    $res.stage='enumerated'; Flush
    $enumJson = Join-Path $base 'pf-enum.json'
    & powershell -NoProfile -ExecutionPolicy Bypass -Command "& { $j = Get-Content '$base\pf-ownwin.json' -Raw -ErrorAction SilentlyContinue } " | Out-Null
    # use the driver's own owned-windows builder via a tiny node codegen step (one-shot, not in the click path)
    $genOut = Join-Path $base 'pf-click.ps1'
    $genInfo = Join-Path $base 'pf-gen.json'
    Remove-Item -Force $genOut,$genInfo -ErrorAction SilentlyContinue
    $gp = Start-Process -FilePath $node -ArgumentList @($gen, $app.Id, $genOut, $genInfo) -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $base 'pf-gen.out') -RedirectStandardError (Join-Path $base 'pf-gen.err')
    if (-not $gp.WaitForExit(60000)) { try { taskkill /T /F /PID $gp.Id 2>&1 | Out-Null } catch {} }
    $res.genInfo = (Get-Content $genInfo -Raw -ErrorAction SilentlyContinue)
    $res.stage='gen-done'; Flush
    # click
    $clickLog = Join-Path $base 'pf-click.out'
    Remove-Item -Force $clickLog -ErrorAction SilentlyContinue
    $cp = Start-Process -FilePath 'powershell' -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',$genOut) -PassThru -WindowStyle Hidden -RedirectStandardOutput $clickLog -RedirectStandardError ($clickLog+'.err')
    if (-not $cp.WaitForExit(120000)) { try { taskkill /T /F /PID $cp.Id 2>&1 | Out-Null } catch {} }
    $res.uiaClickRaw = (Get-Content $clickLog -Raw -ErrorAction SilentlyContinue)
    $res.stage='click-done'; Flush
    Start-Sleep -Seconds 6
    $res.postInventory = DaemonQuery $runtimeDir 'list'
    $res.workspaceFileAfter = if (Test-Path $wsFile) { (Get-Content $wsFile -Raw) } else { 'ABSENT' }
    $res.receiptFilesAfter = @(Get-ChildItem $barrierDir -File -ErrorAction SilentlyContinue | ForEach-Object { $_.Name })
    $res.allReceiptsAfter = @{}
    foreach ($f in (Get-ChildItem $barrierDir -File -Filter '*.jsonl' -ErrorAction SilentlyContinue)) { $res.allReceiptsAfter[$f.Name] = (Get-Content $f.FullName -Raw) }
    $res.daemonLogTail = ((Get-Content (Join-Path $iso 'data\logs\daemon.log') -Tail 40) -join "`n")
    $treeLog = Join-Path $base 'pf-tree.out'
    Remove-Item -Force $treeLog -ErrorAction SilentlyContinue
    $tp = Start-Process -FilePath 'powershell' -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',$tree,'-TargetPid',$app.Id,'-Cap','250') -PassThru -WindowStyle Hidden -RedirectStandardOutput $treeLog -RedirectStandardError ($treeLog+'.err')
    if (-not $tp.WaitForExit(120000)) { try { taskkill /T /F /PID $tp.Id 2>&1 | Out-Null } catch {} }
    $res.uiaTreeAfterRaw = (Get-Content $treeLog -Raw -ErrorAction SilentlyContinue)
    $res.outcome='MEASURED'
  } else {
    $res.outcome='APP_NOT_READY'
    $res.daemonLogTail = ((Get-Content (Join-Path $iso 'data\logs\daemon.log') -Tail 25) -join "`n")
    $res.fixtureReceipt = (Get-Content (Join-Path $barrierDir 'fixture-setup.receipt.jsonl') -Raw -ErrorAction SilentlyContinue)
  }
} catch { $res.errors += ("STAGE=" + $res.stage + " :: " + $_.Exception.Message) }
$res.finishedAt=(Get-Date).ToString('o'); Flush
if ($res.appPid) { try { taskkill /T /F /PID $res.appPid 2>&1 | Out-Null } catch {} }
if ($res.serverPid) { try { taskkill /T /F /PID $res.serverPid 2>&1 | Out-Null } catch {} }
Set-Content -Path ($Out + '.done') -Value 'done' -Encoding ascii
Write-Output "PF_RUN_DONE"
