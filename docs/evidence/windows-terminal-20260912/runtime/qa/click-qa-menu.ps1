param([string]$Label = 'New Terminal Profile')
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class QaMenuPointer {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint dx,uint dy,uint data,UIntPtr extra);
}
"@
$qa = Get-Process -Id 21620
if ($qa.Path -ne 'C:\Users\sook\ferryx-qa-rt-st01a0958a\orca-lite\src-tauri\target\debug\ferryx.exe') { throw 'QA identity changed' }
$condition = New-Object System.Windows.Automation.PropertyCondition(
  [System.Windows.Automation.AutomationElement]::ProcessIdProperty, [int]$qa.Id)
$elements = [System.Windows.Automation.AutomationElement]::RootElement.FindAll(
  [System.Windows.Automation.TreeScope]::Descendants, $condition)
foreach ($e in $elements) {
  if ($e.Current.Name -ne $Label -or $e.Current.ControlType -ne [System.Windows.Automation.ControlType]::MenuItem -or $e.Current.IsOffscreen) { continue }
  $r = $e.Current.BoundingRectangle
  $point = New-Object QaMenuPointer+POINT
  $point.X = [int]($r.Left + 25); $point.Y = [int]($r.Top + $r.Height/2)
  $window = [QaMenuPointer]::WindowFromPoint($point)
  [uint32]$owner = 0
  [QaMenuPointer]::GetWindowThreadProcessId($window,[ref]$owner) | Out-Null
  if ($owner -ne $qa.Id) { throw "Click blocked: point belongs to $owner, not QA" }
  if (-not [QaMenuPointer]::SetCursorPos($point.X,$point.Y)) { throw 'Cursor positioning failed' }
  [QaMenuPointer]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
  [QaMenuPointer]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
  "CLICKED label=$Label pid=$owner hwnd=$window x=$($point.X) y=$($point.Y)" | Add-Content "$PSScriptRoot\..\evidence\menu-clicks.log"
  exit 0
}
throw "Visible QA menu item not found: $Label"
