param(
  [switch]$CompileOnly,
  [string]$QaRoot = 'C:\Users\sook\ferryx-qa-dag-st01a099f8',
  [int]$GuiPid,
  [long]$CreatedUnixMs,
  [long]$Hwnd,
  [ValidateSet('wheel','key','drag','capture')][string]$Action = 'capture',
  [int]$X, [int]$Y, [int]$EndX, [int]$EndY,
  [ValidateSet(-120,120)][int]$Delta = 120,
  [ValidateRange(1,255)][int]$VirtualKey = 87,
  [switch]$Control,
  [string]$Receipt
)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class DagInput {
  [StructLayout(LayoutKind.Sequential)] public struct Point { public int X,Y; }
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int L,T,R,B; }
  [StructLayout(LayoutKind.Sequential)] public struct Mouse { public int dx,dy; public uint data,flags,time; public UIntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] public struct Key { public ushort vk,scan; public uint flags,time; public UIntPtr extra; }
  [StructLayout(LayoutKind.Explicit)] public struct Union { [FieldOffset(0)] public Mouse mouse; [FieldOffset(0)] public Key key; }
  [StructLayout(LayoutKind.Sequential)] public struct Input { public uint type; public Union value; }
  [DllImport("user32.dll",SetLastError=true)] public static extern uint SendInput(uint n, Input[] inputs, int size);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
  public static void MouseEvent(uint flags,int data) {
    Input i=new Input(); i.type=0; i.value.mouse.flags=flags; i.value.mouse.data=unchecked((uint)data); Send(i);
  }
  public static void KeyEvent(ushort vk,bool up) {
    Input i=new Input(); i.type=1; i.value.key.vk=vk; i.value.key.flags=up?2u:0u; Send(i);
  }
  static void Send(Input i) {
    if(SendInput(1,new Input[]{i},Marshal.SizeOf(typeof(Input)))!=1)
      throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(),"SendInput rejected");
  }
}
'@
if($CompileOnly) {
  if([Runtime.InteropServices.Marshal]::SizeOf([type][DagInput+Input]) -ne 40){throw 'Expected x64 INPUT size 40'}
  'DAG_INPUT_COMPILE_OK'; exit 0
}
if((Get-Process -Id $PID).SessionId -eq 0){throw 'Interactive desktop required; Session0 input forbidden'}
if($QaRoot -ne 'C:\Users\sook\ferryx-qa-dag-st01a099f8'){throw 'Not the allocated QA root'}
$p = Get-CimInstance Win32_Process -Filter "ProcessId=$GuiPid"
if(!$p -or $p.ExecutablePath -ne "$QaRoot\orca-lite\target\debug\ferryx.exe" -or
  ([DateTimeOffset]$p.CreationDate).ToUnixTimeMilliseconds() -ne $CreatedUnixMs -or
  $p.SessionId -ne (Get-Process -Id $PID).SessionId -or $p.CommandLine -match '--daemon') {throw 'GUI identity mismatch'}
$h = [IntPtr]$Hwnd
[uint32]$owner = 0
[DagInput]::GetWindowThreadProcessId($h,[ref]$owner) | Out-Null
if($owner -ne $GuiPid -or ![DagInput]::IsWindowVisible($h) -or [DagInput]::GetForegroundWindow() -ne $h){throw 'QA main HWND must already be visible and foreground'}
if(!$Receipt -or !(Test-Path "$QaRoot\evidence") -or
  [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($Receipt)) -ne "$QaRoot\evidence" -or
  (Test-Path $Receipt) -or (Test-Path "$Receipt.png")){throw 'Provide unused receipt path directly in owned evidence directory'}
[DagInput]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
function Check-Point([int]$px,[int]$py) {
  $pt=New-Object DagInput+Point; $pt.X=$px; $pt.Y=$py
  [uint32]$pointOwner=0
  [DagInput]::GetWindowThreadProcessId([DagInput]::WindowFromPoint($pt),[ref]$pointOwner) | Out-Null
  if($pointOwner -ne $GuiPid){throw 'Input point not owned by QA GUI'}
}
$errorText=$null
try {
  if($Action -in @('wheel','drag')) {
    Check-Point $X $Y
    if($Action -eq 'drag'){Check-Point $EndX $EndY}
    if(![DagInput]::SetCursorPos($X,$Y)){throw 'SetCursorPos failed'}
  }
  switch($Action) {
    wheel { [DagInput]::MouseEvent(0x0800,$Delta) }
    key {
      try {
        if($Control){[DagInput]::KeyEvent(17,$false)}
        [DagInput]::KeyEvent([ushort]$VirtualKey,$false)
      } finally {
        [DagInput]::KeyEvent([ushort]$VirtualKey,$true)
        if($Control){[DagInput]::KeyEvent(17,$true)}
      }
    }
    drag {
      try {
        [DagInput]::MouseEvent(2,0)
        if(![DagInput]::SetCursorPos($EndX,$EndY)){throw 'Drag movement failed'}
      } finally { [DagInput]::MouseEvent(4,0) }
    }
  }
} catch { $errorText=$_.Exception.ToString(); throw }
finally {
  [pscustomobject]@{utc=[DateTime]::UtcNow.ToString('o');pid=$GuiPid;createdUnixMs=$CreatedUnixMs;hwnd=$Hwnd;action=$Action;x=$X;y=$Y;endX=$EndX;endY=$EndY;delta=$Delta;virtualKey=$VirtualKey;control=[bool]$Control;dpi=[DagInput]::GetDpiForWindow($h);error=$errorText;acceptance='NOT_ASSERTED'} | ConvertTo-Json | Set-Content -LiteralPath $Receipt
}
# This immediate capture records dispatch only. Capture again after an exact render/PTY signal.
Add-Type -AssemblyName System.Drawing
$rect=New-Object DagInput+Rect
if(![DagInput]::GetWindowRect($h,[ref]$rect)){throw 'GetWindowRect failed'}
$bitmap=New-Object Drawing.Bitmap ($rect.R-$rect.L),($rect.B-$rect.T)
$graphics=[Drawing.Graphics]::FromImage($bitmap)
try { $graphics.CopyFromScreen($rect.L,$rect.T,0,0,$bitmap.Size); $bitmap.Save("$Receipt.png") }
finally { $graphics.Dispose(); $bitmap.Dispose() }
