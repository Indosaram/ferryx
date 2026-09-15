$ErrorActionPreference = 'Stop'
$root = 'C:\Users\sook\ferryx-qa-fresh-0912'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class FreshPointer {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref POINT p);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr e);
}
"@
$qa = Get-Process -Id 24236
if ($qa.Path -ne "$root\orca-lite\src-tauri\target\debug\ferryx.exe") { throw 'QA PID changed' }
$condition = New-Object System.Windows.Automation.PropertyCondition(
  [System.Windows.Automation.AutomationElement]::ProcessIdProperty,[int]$qa.Id)
function Click-Menu([string]$label) {
  $elements = [System.Windows.Automation.AutomationElement]::RootElement.FindAll(
    [System.Windows.Automation.TreeScope]::Descendants,$condition)
  foreach ($e in $elements) {
    if ($e.Current.Name -ne $label -or $e.Current.IsOffscreen) { continue }
    $r = $e.Current.BoundingRectangle
    $point = New-Object FreshPointer+POINT
    $point.X = [int]($r.Left+25); $point.Y = [int]($r.Top+$r.Height/2)
    [uint32]$owner=0
    $h=[FreshPointer]::WindowFromPoint($point)
    [FreshPointer]::GetWindowThreadProcessId($h,[ref]$owner) | Out-Null
    if ($owner -ne $qa.Id) { throw 'Point is not QA-owned' }
    [FreshPointer]::SetCursorPos($point.X,$point.Y) | Out-Null
    [FreshPointer]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
    [FreshPointer]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    "CLICK $label pid=$owner hwnd=$h x=$($point.X) y=$($point.Y)" | Add-Content "$root\evidence\menu-actions.log"
    return
  }
  throw "Menu item not visible: $label"
}
try {
  $plus = New-Object FreshPointer+POINT
  $plus.X=475; $plus.Y=20
  [FreshPointer]::ClientToScreen([IntPtr]3213230,[ref]$plus) | Out-Null
  [uint32]$plusOwner=0
  $plusWindow=[FreshPointer]::WindowFromPoint($plus)
  [FreshPointer]::GetWindowThreadProcessId($plusWindow,[ref]$plusOwner) | Out-Null
  $plusProcess=Get-CimInstance Win32_Process -Filter "ProcessId=$plusOwner"
  $ownedWebView=$plusProcess.ParentProcessId -eq $qa.Id -and
    $plusProcess.Name -eq 'msedgewebview2.exe' -and
    $plusProcess.CommandLine.Contains("$root\webview2")
  if ($plusOwner -ne $qa.Id -and -not $ownedWebView) { throw "Plus point belongs to $plusOwner, not QA" }
  [FreshPointer]::SetCursorPos($plus.X,$plus.Y) | Out-Null
  [FreshPointer]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
  [FreshPointer]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
  "CLICK New tab pid=$plusOwner x=$($plus.X) y=$($plus.Y)" | Add-Content "$root\evidence\menu-actions.log"
  Click-Menu 'New Terminal Profile'
  & "$root\capture-fresh.ps1"
  Click-Menu 'Command Prompt'
  Write-Output 'CMD_MENU_SELECTED'
} catch {
  $_ | Out-String | Set-Content "$root\evidence\menu-error.txt"
  exit 1
}
