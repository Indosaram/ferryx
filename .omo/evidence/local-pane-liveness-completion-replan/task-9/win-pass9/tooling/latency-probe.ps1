param(
  [string]$Out = 'C:\Users\sook\ferryx-pane-completion\latency-probe.json',
  [int]$PollMs = 250,
  [int]$MaxMs = 30000
)
$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$bin  = Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$srv  = Join-Path $base 'serve-dist.mjs'
$dist = Join-Path $root 'ui\dist'

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class LatWin {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
}
'@

function Measure-Tree([int]$TargetPid) {
  $pids = @([uint32]$TargetPid)
  $handles = New-Object System.Collections.ArrayList
  $cb = [LatWin+EnumProc]{
    param($h,$l)
    $op = 0
    [LatWin]::GetWindowThreadProcessId($h, [ref]$op) | Out-Null
    if (($pids -contains $op) -and [LatWin]::IsWindowVisible($h)) { $handles.Add($h) | Out-Null }
    return $true
  }
  [LatWin]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
  $total = 0; $named = 0; $hasDoc = $false; $hasNewTerminal = $false
  foreach ($h in $handles) {
    $r = [System.Windows.Automation.AutomationElement]::FromHandle($h)
    if ($r -eq $null) { continue }
    $all = $r.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    $total += $all.Count
    for ($i = 0; $i -lt $all.Count; $i++) {
      try {
        $e = $all.Item($i)
        $ct = $e.Current.ControlType.ProgrammaticName
        $n = $e.Current.Name
        if ($n -and $n.Length -gt 0) { $named++ }
        if ($ct -eq 'ControlType.Document') { $hasDoc = $true }
        if ($n -eq 'New Terminal') { $hasNewTerminal = $true }
      } catch {}
    }
  }
  return [ordered]@{ windows=$handles.Count; total=$total; named=$named; hasDocument=$hasDoc; hasNewTerminal=$hasNewTerminal }
}

$srvProc = Start-Process -FilePath 'bun' -ArgumentList @($srv, $dist) -PassThru -WindowStyle Hidden
$srvPid = $srvProc.Id
foreach ($i in 1..24) { Start-Sleep -Milliseconds 500; if (Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue) { break } }
$serverUp = [bool](Get-NetTCPConnection -LocalPort 5173 -State Listen -ErrorAction SilentlyContinue)

$results = New-Object System.Collections.ArrayList
foreach ($delay in @(1000, 3000, 6000, 12000)) {
  $iso = Join-Path $base ('lat-iso-' + $delay)
  if (Test-Path $iso) { Remove-Item -Recurse -Force $iso -ErrorAction SilentlyContinue }
  foreach ($d in @('data','runtime','home','barriers')) { New-Item -ItemType Directory -Force -Path (Join-Path $iso $d) | Out-Null }
  $env:FERRYX_DATA_DIR = Join-Path $iso 'data'
  $env:FERRYX_RUNTIME_DIR = Join-Path $iso 'runtime'
  $env:FERRYX_QA_BARRIER_DIR = Join-Path $iso 'barriers'
  $env:FERRYX_QA_RUN_ID = 'qa-run-lat-' + $delay
  $env:FERRYX_QA_OPERATION_ID = 'qa-op-lat-' + $delay
  $env:FERRYX_QA_FIXTURE_KINDS = 'source'

  $app = Start-Process -FilePath $bin -PassThru -WorkingDirectory $root
  $appPid = $app.Id
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  Start-Sleep -Milliseconds $delay
  $first = Measure-Tree $appPid
  $attachAt = $sw.ElapsedMilliseconds
  $polls = New-Object System.Collections.ArrayList
  $polls.Add([ordered]@{ atMs=$attachAt; total=$first.total; named=$first.named; hasDocument=$first.hasDocument; hasNewTerminal=$first.hasNewTerminal }) | Out-Null
  $domAt = $null
  if ($first.hasDocument) { $domAt = $attachAt } else {
    $deadline = $attachAt + $MaxMs
    while ($sw.ElapsedMilliseconds -lt $deadline) {
      Start-Sleep -Milliseconds $PollMs
      $m = Measure-Tree $appPid
      $polls.Add([ordered]@{ atMs=$sw.ElapsedMilliseconds; total=$m.total; named=$m.named; hasDocument=$m.hasDocument; hasNewTerminal=$m.hasNewTerminal }) | Out-Null
      if ($m.hasDocument) { $domAt = $sw.ElapsedMilliseconds; break }
    }
  }
  $alive = $null -ne (Get-Process -Id $appPid -ErrorAction SilentlyContinue)
  $results.Add([ordered]@{
    firstAttachDelayMs = $delay
    launchToAttachMs = $attachAt
    firstAttachTotal = $first.total
    firstAttachNamed = $first.named
    appAliveAtEnd = $alive
    domAtMsFromLaunch = $domAt
    domLatencyFromAttachMs = $(if ($domAt -ne $null) { $domAt - $attachAt } else { $null })
    pollCount = $polls.Count
    polls = @($polls)
  }) | Out-Null
  try { taskkill /T /F /PID $appPid 2>&1 | Out-Null } catch {}
  Start-Sleep -Seconds 1
}

try { taskkill /T /F /PID $srvPid 2>&1 | Out-Null } catch {}
[ordered]@{ session=(Get-Process -Id $PID).SessionId; pollMs=$PollMs; maxMs=$MaxMs; serverUp=$serverUp; results=@($results) } | ConvertTo-Json -Depth 8 | Set-Content -Path $Out -Encoding utf8
Write-Output "LATENCY_PROBE_DONE"
