param([Parameter(Mandatory=$true)][int]$TargetPid)
$ErrorActionPreference = 'Continue'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class ExpOleacc {
  [DllImport("oleacc.dll")]
  public static extern int AccessibleObjectFromWindow(IntPtr hwnd, uint dwObjectID, ref Guid riid, [MarshalAs(UnmanagedType.Interface)] out object ppvObject);
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
}
'@
$pids = @([uint32]$TargetPid)
$handles = New-Object System.Collections.ArrayList
$cb = [ExpOleacc+EnumProc]{
  param($h, $l)
  $op = 0
  [ExpOleacc]::GetWindowThreadProcessId($h, [ref]$op) | Out-Null
  if (($pids -contains $op) -and [ExpOleacc]::IsWindowVisible($h)) { $handles.Add($h) | Out-Null }
  return $true
}
[ExpOleacc]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
# OBJID_CLIENT = 0xFFFFFFFC ; IID_IAccessible = {618736E0-3C3D-11CF-810C-00AA00389B71}
$iid = [Guid]'618736E0-3C3D-11CF-810C-00AA00389B71'
$results = New-Object System.Collections.ArrayList
foreach ($h in $handles) {
  $obj = $null
  $hr = 0
  try { $hr = [ExpOleacc]::AccessibleObjectFromWindow($h, 0xFFFFFFFC, [ref]$iid, [ref]$obj) } catch { $hr = -9999 }
  $name = $null
  $role = $null
  $childCount = $null
  if ($obj -ne $null) {
    try { $name = $obj.accName(0) } catch { }
    try { $role = $obj.accRole(0) } catch { }
    try { $childCount = $obj.accChildCount } catch { }
  }
  $results.Add([ordered]@{ hwnd = $h.ToInt64(); hr = $hr; gotObject = ($obj -ne $null); name = $name; role = $role; childCount = $childCount }) | Out-Null
}
Write-Output ([ordered]@{ probe = 'oleacc-activate'; pid = $TargetPid; visibleWindows = $handles.Count; results = @($results) } | ConvertTo-Json -Compress -Depth 6)
