$ErrorActionPreference = 'Stop'
$root = 'C:\Users\sook\ferryx-qa-fresh-0912'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class FreshWindow {
  public delegate bool EnumProc(IntPtr h,IntPtr p);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb,IntPtr p);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
}
"@
$pids = @(Get-Process ferryx -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -eq "$root\orca-lite\src-tauri\target\debug\ferryx.exe" } |
  ForEach-Object { $_.Id })
$rows = [Collections.Generic.List[object]]::new()
$callback = [FreshWindow+EnumProc]{
  param($h,$unused)
  [uint32]$owner = 0
  [FreshWindow]::GetWindowThreadProcessId($h,[ref]$owner) | Out-Null
  if ($pids -contains $owner -and [FreshWindow]::IsWindowVisible($h)) {
    $rect = New-Object FreshWindow+RECT
    [FreshWindow]::GetWindowRect($h,[ref]$rect) | Out-Null
    $rows.Add([pscustomobject]@{ pid=$owner; hwnd=$h.ToInt64(); rect=$rect })
  }
  return $true
}
[FreshWindow]::EnumWindows($callback,[IntPtr]::Zero) | Out-Null
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$rows | ConvertTo-Json -Depth 4 | Set-Content "$root\evidence\hwnd-$stamp.json"
$bounds = [Windows.Forms.SystemInformation]::VirtualScreen
$bitmap = New-Object Drawing.Bitmap $bounds.Width,$bounds.Height
$graphics = [Drawing.Graphics]::FromImage($bitmap)
try {
  $graphics.CopyFromScreen($bounds.Left,$bounds.Top,0,0,$bitmap.Size)
  $bitmap.Save("$root\evidence\screen-$stamp.png",[Drawing.Imaging.ImageFormat]::Png)
} finally {
  $graphics.Dispose()
  $bitmap.Dispose()
}
Write-Output "CAPTURED $stamp"
