# Operator-assisted extension of runtime/qa; does not launch an app or daemon.
# Run in the owned Windows interactive allocation, with PowerShell terminal panes.
[CmdletBinding()]
param(
  [Parameter(Mandatory=$true)][int]$AppProcessId,
  [Parameter(Mandatory=$true)][string]$DebugExecutable,
  [Parameter(Mandatory=$true)][string]$OutputDirectory,
  [Parameter(Mandatory=$true)][switch]$OwnedInteractiveAllocation
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (!$OwnedInteractiveAllocation) { throw 'Owned allocation acknowledgment required' }
if ($DebugExecutable -notmatch '[\\/]target[\\/]debug[\\/]ferryx.exe$') { throw 'Debug binary required' }
if (!(Test-Path -LiteralPath $OutputDirectory -PathType Container)) { throw 'Existing owned output directory required' }
$app = Get-Process -Id $AppProcessId
if ($app.Path -ne $DebugExecutable -or $app.SessionId -ne (Get-Process -Id $PID).SessionId) { throw 'Wrong app path or interactive session' }
$appStart = $app.StartTime.ToUniversalTime().ToString('o')
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class PR3Window {
 [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
}
'@
$run = [Guid]::NewGuid().ToString('N')
function Read-ShellReceipt([string]$path, [string]$marker) {
  $r = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
  if ($r.marker -ne $marker) { throw 'Wrong shell sentinel' }
  $p = Get-Process -Id $r.pid
  if ($p.StartTime.ToUniversalTime().ToString('o') -ne $r.start -or $p.SessionId -ne $app.SessionId) { throw 'Shell identity mismatch' }
  return $r
}
function Shell-Command([string]$path, [string]$marker) {
  $q = $path.Replace("'", "''")
  return "[ordered]@{marker='$marker';pid=`$PID;start=(Get-Process -Id `$PID).StartTime.ToUniversalTime().ToString('o')} | ConvertTo-Json | Set-Content -LiteralPath '$q'"
}
foreach ($kind in @('untagged','terminal')) {
 foreach ($pinned in @($false,$true)) {
  foreach ($route in @('keyboard','native-menu')) {
   foreach ($leaf in @('first','second')) {
    $case = "$kind-pinned$pinned-$route-$leaf"
    $prefix = Join-Path $OutputDirectory "$run-$case"
    Write-Host "CASE $case : create a NEW owned two-PowerShell-pane tab; focus $leaf."
    Write-Host "Use existing runtime/qa capture helpers only after rebinding their old allocation paths."
    Write-Host 'Save real before-layout JSON and HWND/action screenshots; kind must be absent for untagged, terminal otherwise.'
    $beforeLayout = Read-Host 'Path to real before-layout evidence'
    if (!(Test-Path -LiteralPath $beforeLayout -PathType Leaf)) { throw 'Missing layout evidence' }
    $tab = Read-Host 'Actual tab ID from layout'
    $selected = Read-Host 'Actual selected backend session ID from layout'
    $sibling = Read-Host 'Actual sibling backend session ID from layout'
    if (!$tab -or !$selected -or !$sibling -or $selected -eq $sibling) { throw 'Distinct actual session IDs required' }
    [long]$hwnd = Read-Host 'Actual owned top-level HWND (decimal) from capture'
    [uint32]$owner = 0
    [PR3Window]::GetWindowThreadProcessId([IntPtr]$hwnd,[ref]$owner) | Out-Null
    if (![PR3Window]::IsWindow([IntPtr]$hwnd) -or $owner -ne $AppProcessId) { throw 'Wrong HWND owner' }
    foreach ($role in @('selected','sibling')) {
      Write-Host "In the $role pane execute:"
      Write-Host (Shell-Command "$prefix-$role.json" "$run-$case-$role")
      $null = Read-Host 'Press Enter here only after shell output file exists'
    }
    $a = Read-ShellReceipt "$prefix-selected.json" "$run-$case-selected"
    $b = Read-ShellReceipt "$prefix-sibling.json" "$run-$case-sibling"
    if ($a.pid -eq $b.pid) { throw 'Not two distinct shell processes' }
    $selectedProcess = Get-Process -Id $a.pid
    # Retain the real process handle before action, so PID reuse cannot fake exit.
    $null = $selectedProcess.Handle
    Write-Host "Refocus $leaf in HWND $hwnd. Subscribe existing terminal exit recorder BEFORE action."
    Write-Host "For keyboard use real Ctrl+W; for native-menu click app File > Close Tab (not synthetic JS callback)."
    Write-Host 'Capture input/action and selected backend exit. No confirmation bypass: record any dialog separately.'
    $null = Read-Host 'Press Enter after the close action and selected exit event'
    if (!$selectedProcess.HasExited) { throw 'Selected actual shell has not exited' }
    Write-Host 'In surviving pane execute the following NEW command (not replay/history):'
    Write-Host (Shell-Command "$prefix-after.json" "$run-$case-after")
    $null = Read-Host 'Press Enter after new output file exists'
    $after = Read-ShellReceipt "$prefix-after.json" "$run-$case-after"
    if ($after.pid -ne $b.pid -or $after.start -ne $b.start) { throw 'Sibling shell replaced' }
    $afterSession = Read-Host 'Actual surviving backend session ID from after-layout'
    if ($afterSession -ne $sibling) { throw 'Sibling backend replaced' }
    $afterLayout = Read-Host 'Path to actual after-layout evidence (same tab, pin value, sibling leaf)'
    $exitEvidence = Read-Host 'Path to actual selected backend exit event evidence'
    $actionEvidence = Read-Host 'Path to real HWND keyboard/menu action evidence'
    foreach ($path in @($afterLayout,$exitEvidence,$actionEvidence)) {
      if (!(Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing evidence $path" }
    }
    $currentApp = Get-Process -Id $AppProcessId
    if ($currentApp.StartTime.ToUniversalTime().ToString('o') -ne $appStart) { throw 'App replaced' }
    [ordered]@{
      case=$case; run=$run; appPid=$AppProcessId; appStart=$appStart; hwnd=$hwnd
      tab=$tab; selectedBackend=$selected; siblingBackend=$sibling
      selectedShell=$a; siblingBefore=$b; siblingAfter=$after
      beforeLayout=$beforeLayout; afterLayout=$afterLayout; exitEvidence=$exitEvidence; actionEvidence=$actionEvidence
      verdict='PROCESS_SENTINELS_VERIFIED; layout/event/action contents require runtime-owner review'
    } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath "$prefix-receipt.json"
    $selectedProcess.Dispose()
    Write-Host 'Close only this owned survivor via GUI; retain cleanup receipt before next case. Never stop user daemons.'
    $null = Read-Host 'Press Enter after owned-case cleanup is recorded'
   }
  }
 }
}
Write-Host "Matrix receipts saved for $run. Not an automatic acceptance verdict."
