$ErrorActionPreference='Stop'
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class FreshResize {
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int height,uint f);
}
"@
$h=[IntPtr]3213230
[uint32]$owner=0
[FreshResize]::GetWindowThreadProcessId($h,[ref]$owner) | Out-Null
if ((Get-Process -Id $owner).Path -ne 'C:\Users\sook\ferryx-qa-fresh-0912\orca-lite\src-tauri\target\debug\ferryx.exe') { throw 'Not QA window' }
if (-not [FreshResize]::SetWindowPos($h,[IntPtr]::Zero,20,60,1200,900,0x0014)) { throw 'Resize failed' }
"RESIZED pid=$owner hwnd=$h 1200x900" | Set-Content 'C:\Users\sook\ferryx-qa-fresh-0912\evidence\resize-action.log'
