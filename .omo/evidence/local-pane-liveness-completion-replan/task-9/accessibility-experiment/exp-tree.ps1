param([Parameter(Mandatory=$true)][int]$TargetPid, [int]$Cap = 120)
$ErrorActionPreference = 'Continue'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class ExpWin {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
}
'@
$pids = @([uint32]$TargetPid)
$handles = New-Object System.Collections.ArrayList
$cb = [ExpWin+EnumProc]{
  param($h, $l)
  $op = 0
  [ExpWin]::GetWindowThreadProcessId($h, [ref]$op) | Out-Null
  if (($pids -contains $op) -and [ExpWin]::IsWindowVisible($h)) { $handles.Add($h) | Out-Null }
  return $true
}
[ExpWin]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
$report = New-Object System.Collections.ArrayList
foreach ($h in $handles) {
  $root = [System.Windows.Automation.AutomationElement]::FromHandle($h)
  if ($root -eq $null) {
    $report.Add([ordered]@{ hwnd = $h.ToInt64(); rootNull = $true; total = 0; shown = 0; items = @() }) | Out-Null
    continue
  }
  $all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  $items = New-Object System.Collections.ArrayList
  $n = [Math]::Min($all.Count, $Cap)
  for ($i = 0; $i -lt $n; $i = $i + 1) {
    try {
      $e = $all.Item($i)
      $items.Add([ordered]@{ name = $e.Current.Name; controlType = $e.Current.ControlType.ProgrammaticName; automationId = $e.Current.AutomationId; className = $e.Current.ClassName }) | Out-Null
    } catch { }
  }
  $report.Add([ordered]@{ hwnd = $h.ToInt64(); rootNull = $false; total = $all.Count; shown = $n; items = @($items) }) | Out-Null
}
Write-Output ([ordered]@{ probe = 'tree-dump'; pid = $TargetPid; visibleWindows = $handles.Count; cap = $Cap; windows = @($report) } | ConvertTo-Json -Compress -Depth 8)
