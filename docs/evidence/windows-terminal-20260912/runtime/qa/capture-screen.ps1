# Full-screen capture for st_01a0958a QA evidence. Runs inside the interactive session
# (launched via scheduled task). Saves a PNG plus process/window inventory.
param([string]$Tag = 'shot')
$ErrorActionPreference = 'Stop'
$qaRoot = 'C:\Users\sook\ferryx-qa-rt-st01a0958a'
$evidence = Join-Path $qaRoot 'evidence'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$b = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
$path = Join-Path $evidence ("screen-{0}-{1}.png" -f $Tag, (Get-Date -Format 'HHmmss'))
$bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
Write-Host ("SCREEN_SAVED " + $path)
# Window inventory of QA ferryx only (path-filtered, installed app excluded)
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WinEnum {
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lp);
  public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lp);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr hWnd);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
$qaPids = (Get-Process -Name ferryx -ErrorAction SilentlyContinue | Where-Object { $_.Path -like 'C:\Users\sook\ferryx-qa-rt-st01a0958a\*' }).Id
Write-Host ("QA_FERRYX_PIDS " + ($qaPids -join ','))
$found = New-Object System.Collections.ArrayList
$cb = [WinEnum+EnumWindowsProc]{
  param($h, $l)
  [uint32]$wpid = 0
  [WinEnum]::GetWindowThreadProcessId($h, [ref]$wpid) | Out-Null
  if ($qaPids -contains [int]$wpid) {
    $t = New-Object System.Text.StringBuilder 256
    $c = New-Object System.Text.StringBuilder 256
    [WinEnum]::GetWindowText($h, $t, 256) | Out-Null
    [WinEnum]::GetClassName($h, $c, 256) | Out-Null
    $r = New-Object WinEnum+RECT
    [WinEnum]::GetWindowRect($h, [ref]$r) | Out-Null
    $parent = [WinEnum]::GetParent($h)
    $found.Add([pscustomobject]@{ Hwnd = $h; Parent = $parent; Class = $c.ToString(); Title = $t.ToString(); Visible = [WinEnum]::IsWindowVisible($h); L = $r.L; T = $r.T; R = $r.R; B = $r.B }) | Out-Null
  }
  return $true
}
[WinEnum]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
$found | Format-Table Hwnd, Parent, Class, Visible, L, T, R, B, Title -AutoSize | Out-String -Width 200 | ForEach-Object { Write-Host $_ }
$found | ConvertTo-Json | Set-Content (Join-Path $evidence ("hwnd-{0}.json" -f $Tag))
Write-Host 'WINDOW_DUMP_DONE'
