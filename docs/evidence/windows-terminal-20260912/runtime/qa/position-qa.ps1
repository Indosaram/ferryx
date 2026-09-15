param([int]$Width = 1000, [int]$Height = 760)
$ErrorActionPreference = 'Stop'
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class QaPosition {
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hgt, uint flags);
}
"@
$hwnd = [IntPtr]15140440
[uint32]$owner = 0
[QaPosition]::GetWindowThreadProcessId($hwnd, [ref]$owner) | Out-Null
$process = Get-Process -Id $owner
if ($process.Path -ne 'C:\Users\sook\ferryx-qa-rt-st01a0958a\orca-lite\src-tauri\target\debug\ferryx.exe') {
  throw "Window is not owned by the QA executable"
}
if (-not [QaPosition]::SetWindowPos($hwnd, [IntPtr]::Zero, 10, 50, $Width, $Height, 0x0014)) {
  throw "SetWindowPos failed"
}
Write-Output "QA_WINDOW_RESIZED pid=$owner hwnd=$hwnd size=${Width}x${Height}"
