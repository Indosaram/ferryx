param(
  [Parameter(Mandatory=$true)][string]$Route,
  [Parameter(Mandatory=$true)][string]$Out,
  [int]$WaitWindowSec = 40
)
$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$bin  = Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$probe = Join-Path $root 'scripts\qa\exp-accessibility-probe.mjs'
$treeP = Join-Path $base 'exp-tree.ps1'
$oleP  = Join-Path $base 'exp-oleacc.ps1'

$result = [ordered]@{
  route = $Route
  startedAt = (Get-Date).ToString('o')
  mySession = (Get-Process -Id $PID).SessionId
  interactive = [bool][System.Environment]::UserInteractive
  binSha = (Get-FileHash -Algorithm SHA256 $bin).Hash.ToLower()
  binBytes = (Get-Item $bin).Length
  hostLoad = (Get-CimInstance Win32_Processor | Select-Object -ExpandProperty LoadPercentage)
  freeGb = [math]::Round((Get-PSDrive C).Free/1GB, 2)
}

$iso = Join-Path $base ('exp-' + $Route)
if (Test-Path $iso) { Remove-Item -Recurse -Force $iso -ErrorAction SilentlyContinue }
foreach ($d in @('data','runtime','home','barriers')) { New-Item -ItemType Directory -Force -Path (Join-Path $iso $d) | Out-Null }
$result.isolationRoot = $iso

# Mirror buildIsolatedEnv exactly.
$env:PATH = $env:PATH
$env:HOME = Join-Path $iso 'home'
$env:FERRYX_DATA_DIR = Join-Path $iso 'data'
$env:FERRYX_RUNTIME_DIR = Join-Path $iso 'runtime'
$env:FERRYX_QA_BARRIER_DIR = Join-Path $iso 'barriers'
$env:FERRYX_QA_RUN_ID = 'qa-run-exp-' + $Route
$env:FERRYX_QA_OPERATION_ID = 'qa-op-exp-' + $Route
$env:FERRYX_QA_FIXTURE_KINDS = 'source'

# Route-specific launch env.
$result.extraEnv = [ordered]@{}
if ($Route -eq 'env') {
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--force-renderer-accessibility'
  $result.extraEnv['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
}

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$proc = Start-Process -FilePath $bin -PassThru -WorkingDirectory $root
$result.appPid = $proc.Id
$result.launchedAt = (Get-Date).ToString('o')

# Wait for a visible owned window (same acceptance as the runner's gate).
$deadline = (Get-Date).AddSeconds($WaitWindowSec)
$visible = 0
$firstVisibleMs = $null
while ((Get-Date) -lt $deadline) {
  $p = Get-CimInstance Win32_Process -Filter ("ProcessId=" + $proc.Id) -ErrorAction SilentlyContinue
  if (-not $p) { break }
  $t = Join-Path $base 'exp-tree.ps1'
  $raw = & powershell -NoProfile -ExecutionPolicy Bypass -File $t -TargetPid $proc.Id -Cap 1 2>&1 | Out-String
  $m = [regex]::Match($raw, '"visibleWindows":(\d+)')
  if ($m.Success -and [int]$m.Groups[1].Value -gt 0) { $visible = [int]$m.Groups[1].Value; $firstVisibleMs = $sw.ElapsedMilliseconds; break }
  Start-Sleep -Milliseconds 500
}
$result.visibleWindowCountAtLaunch = $visible
$result.firstVisibleWindowMs = $firstVisibleMs
$result.waitedMs = $sw.ElapsedMilliseconds

if ($visible -lt 1) {
  $result.outcome = 'NO_VISIBLE_WINDOW'
  try { taskkill /T /F /PID $proc.Id 2>&1 | Out-Null } catch { }
  $result | ConvertTo-Json -Depth 8 | Set-Content -Path $Out -Encoding utf8
  return
}

# Route 2: attach a legacy MSAA client BEFORE measuring.
if ($Route -eq 'oleacc') {
  $result.oleaccRaw = (& powershell -NoProfile -ExecutionPolicy Bypass -File $oleP -TargetPid $proc.Id 2>&1 | Out-String).Trim()
  Start-Sleep -Seconds 3
}

# 1) the driver's own probe (enumeration + split inventory)
$probeOut = & node $probe $proc.Id 2>&1 | Out-String
$result.driverProbeRaw = $probeOut.Trim()
try { $result.driverProbe = ($probeOut.Trim() | ConvertFrom-Json) } catch { $result.driverProbeParseError = $true }

# 2) the unfiltered UIA tree dump (what IS in the tree)
$treeRaw = & powershell -NoProfile -ExecutionPolicy Bypass -File $treeP -TargetPid $proc.Id -Cap 120 2>&1 | Out-String
$result.treeDumpRaw = $treeRaw.Trim()
try { $result.treeDump = ($treeRaw.Trim() | ConvertFrom-Json) } catch { $result.treeDumpParseError = $true }

# Acceptance question: is the literal accessible name findable anywhere?
$needle = 'Split pane right'
$result.nameFoundAnywhere = $false
$result.nameFoundWhere = @()
if ($result.treeDump -and $result.treeDump.windows) {
  foreach ($w in $result.treeDump.windows) {
    foreach ($it in @($w.items)) {
      if ($it -and $it.name -eq $needle) { $result.nameFoundAnywhere = $true; $result.nameFoundWhere += ("exact:" + $w.hwnd + ":" + $it.controlType) }
      elseif ($it -and $it.name -and $it.name -like '*Split*') { $result.nameFoundWhere += ("contains:" + $w.hwnd + ":" + $it.name) }
    }
  }
}
$result.outcome = 'MEASURED'

# Teardown: exact PID only.
try { taskkill /T /F /PID $proc.Id 2>&1 | Out-Null } catch { }
$result | ConvertTo-Json -Depth 12 | Set-Content -Path $Out -Encoding utf8
