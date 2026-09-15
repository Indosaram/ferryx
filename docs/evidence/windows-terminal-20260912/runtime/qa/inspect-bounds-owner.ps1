$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class BoundsOwner {
 [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
 [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@
[BoundsOwner]::SetProcessDPIAware() | Out-Null
$root=[System.Windows.Automation.AutomationElement]::RootElement
$condition=New-Object System.Windows.Automation.PropertyCondition(
 [System.Windows.Automation.AutomationElement]::NameProperty,'Failed to update native terminal bounds')
$rows=@($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition) | ForEach-Object {
 $e=$_
 $r=$e.Current.BoundingRectangle
 $process=Get-CimInstance Win32_Process -Filter "ProcessId=$($e.Current.ProcessId)"
 [pscustomobject]@{name=$e.Current.Name;pid=$e.Current.ProcessId;rect=@($r.X,$r.Y,$r.Width,$r.Height);path=$process.ExecutablePath;parent=$process.ParentProcessId;command=$process.CommandLine}
})
$point=New-Object BoundsOwner+POINT
$point.X=2950;$point.Y=1215
$h=[BoundsOwner]::WindowFromPoint($point)
[uint32]$owner=0
[BoundsOwner]::GetWindowThreadProcessId($h,[ref]$owner) | Out-Null
$process=Get-CimInstance Win32_Process -Filter "ProcessId=$owner"
[pscustomobject]@{matches=$rows;point=@($point.X,$point.Y);pointOwner=$process;hwnd=$h.ToInt64()} |
 ConvertTo-Json -Depth 5 | Set-Content 'C:\Users\sook\ferryx-qa-fresh-0912\evidence\bounds-owner.json'
