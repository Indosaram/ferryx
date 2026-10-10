$ErrorActionPreference='Continue'
$base='C:\Users\sook\ferryx-pane-completion'
$root=Join-Path $base 'source-21dea3c0'
$rt=Join-Path $base 'pf-iso\runtime'
$barrierDir=Join-Path $base 'pf-iso\barriers'
$node='C:\Program Files\nodejs\node.exe'
$dmon=Join-Path $root 'scripts\qa\pf-daemon.mjs'
$gen=Join-Path $root 'scripts\qa\pf-gen.mjs'
$tree=Join-Path $base 'exp-tree.ps1'
$out=Join-Path $base 'manual-delta.json'
$res=[ordered]@{ at=(Get-Date).ToString('o') }

# the app pid: the live ferryx.exe with a window (not the --daemon child)
$apps = Get-CimInstance Win32_Process -Filter "Name='ferryx.exe'" | Where-Object { $_.ExecutablePath -like '*source-21dea3c0*' -and $_.CommandLine -notlike '*--daemon*' }
$res.appPids = @($apps | ForEach-Object { $_.ProcessId })
$res.daemonPids = @((Get-CimInstance Win32_Process -Filter "Name='ferryx.exe'" | Where-Object { $_.CommandLine -like '*--daemon*' }).ProcessId)
Write-Output ("APP_PIDS=" + ($res.appPids -join ','))
Write-Output ("DAEMON_PIDS=" + ($res.daemonPids -join ','))
if (-not $res.appPids) { Write-Output 'NO_APP'; $res | ConvertTo-Json -Depth 8 | Set-Content $out -Encoding utf8; return }
$appPid = $res.appPids[0]

function Query {
  $f=Join-Path $base 'manual-inv.json'
  Remove-Item -Force $f -ErrorAction SilentlyContinue
  $p=Start-Process -FilePath $node -ArgumentList @($dmon,$rt,'list',$f) -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $base 'manual-inv.out') -RedirectStandardError (Join-Path $base 'manual-inv.err')
  if (-not $p.WaitForExit(20000)) { taskkill /T /F /PID $p.Id 2>&1 | Out-Null; return 'TIMEOUT' }
  return (Get-Content $f -Raw -ErrorAction SilentlyContinue)
}

$res.preInventory = Query
Write-Output "=== PRE inventory ==="; Write-Output $res.preInventory
$res.receiptsBefore = @(Get-ChildItem $barrierDir -File -ErrorAction SilentlyContinue | ForEach-Object { $_.Name })
Write-Output ("RECEIPTS_BEFORE=" + ($res.receiptsBefore -join ', '))

# generate + run the driver's own click script
$clickPs1=Join-Path $base 'manual-click.ps1'
$genInfo=Join-Path $base 'manual-gen.json'
Remove-Item -Force $clickPs1,$genInfo -ErrorAction SilentlyContinue
$gp=Start-Process -FilePath $node -ArgumentList @($gen,$appPid,$clickPs1,$genInfo) -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $base 'manual-gen.out') -RedirectStandardError (Join-Path $base 'manual-gen.err')
if (-not $gp.WaitForExit(60000)) { taskkill /T /F /PID $gp.Id 2>&1 | Out-Null }
$res.genInfo = (Get-Content $genInfo -Raw -ErrorAction SilentlyContinue)
Write-Output "=== genInfo ==="; Write-Output $res.genInfo

$clickLog=Join-Path $base 'manual-click.out'
Remove-Item -Force $clickLog -ErrorAction SilentlyContinue
$cp=Start-Process -FilePath 'powershell' -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',$clickPs1) -PassThru -WindowStyle Hidden -RedirectStandardOutput $clickLog -RedirectStandardError ($clickLog+'.err')
if (-not $cp.WaitForExit(120000)) { taskkill /T /F /PID $cp.Id 2>&1 | Out-Null }
$res.clickStdout = (Get-Content $clickLog -Raw -ErrorAction SilentlyContinue)
Write-Output "=== CLICK STDOUT ==="; Write-Output $res.clickStdout

Start-Sleep -Seconds 8
$res.postInventory = Query
Write-Output "=== POST inventory ==="; Write-Output $res.postInventory
$res.receiptsAfter = @(Get-ChildItem $barrierDir -File -ErrorAction SilentlyContinue | ForEach-Object { $_.Name })
Write-Output ("RECEIPTS_AFTER=" + ($res.receiptsAfter -join ', '))
$res.allReceipts = @{}
foreach ($f in (Get-ChildItem $barrierDir -File -Filter '*.jsonl' -ErrorAction SilentlyContinue)) { $res.allReceipts[$f.Name] = (Get-Content $f.FullName -Raw) }
$res.workspaceFile = if (Test-Path (Join-Path $base 'pf-iso\data\remote\machine-workspaces.v1.json')) { (Get-Content (Join-Path $base 'pf-iso\data\remote\machine-workspaces.v1.json') -Raw) } else { 'ABSENT' }
$res.daemonLogTail = ((Get-Content (Join-Path $base 'pf-iso\data\logs\daemon.log') -Tail 50) -join "`n")
$treeOut=Join-Path $base 'manual-tree.json'
Remove-Item -Force $treeOut -ErrorAction SilentlyContinue
$tp=Start-Process -FilePath 'powershell' -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',$tree,'-TargetPid',$appPid,'-Cap','250') -PassThru -WindowStyle Hidden -RedirectStandardOutput $treeOut -RedirectStandardError ($treeOut+'.err')
if (-not $tp.WaitForExit(120000)) { taskkill /T /F /PID $tp.Id 2>&1 | Out-Null }
$res.treeAfter = (Get-Content $treeOut -Raw -ErrorAction SilentlyContinue)
$res | ConvertTo-Json -Depth 10 | Set-Content $out -Encoding utf8
Write-Output "MANUAL_DELTA_DONE"
