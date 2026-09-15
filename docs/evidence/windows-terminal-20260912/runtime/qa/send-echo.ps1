# send-echo.ps1 v2 - types 'echo FERRYX_WIN_START_OK' into the QA Ferryx terminal ONLY.
# Focus strategy (verified before ANY typing; never types while another app is foreground):
#   1. AppActivate(QA pid) -> verify GetForegroundWindow == QA main hwnd
#   2. SW_MINIMIZE + SW_RESTORE + SetForegroundWindow -> verify
#   3. Last resort: single ALT tap (releases the foreground lock) + SetForegroundWindow -> verify
# Then: if the focused child is not the native terminal (FerryxNativeTerm), SendInput-click
# the CENTER of the FerryxNativeTerm rect (point lies inside the already-foreground QA
# window) and re-verify focus. Only then SendKeys. Full trace + before/after shots saved.
param([string]$EchoLine = 'echo FERRYX_WIN_START_OK')
$ErrorActionPreference = 'Continue'
$qaRoot = 'C:\Users\sook\ferryx-qa-rt-st01a0958a'
$evidence = Join-Path $qaRoot 'evidence'
$tag = Get-Date -Format 'HHmmss'

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class QaWin {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lp);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lp);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr hWnd, EnumProc cb, IntPtr lp);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, int dx, int dy, uint dwData, UIntPtr dwExtraInfo);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)]
  public struct GUITHREADINFO {
    public uint cbSize; public uint flags; public IntPtr hwndActive; public IntPtr hwndFocus;
    public IntPtr hwndCapture; public IntPtr hwndMenuOwner; public IntPtr hwndMoveSize;
    public IntPtr hwndCaret; public RECT rcCaret;
  }
  [DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint idThread, ref GUITHREADINFO info);
}
"@
[QaWin]::SetProcessDPIAware() | Out-Null

function Save-Shot([string]$path) {
  $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
  $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
}
function Wait-Foreground([IntPtr]$hwnd) {
  for ($i = 0; $i -lt 12; $i++) {
    if ([QaWin]::GetForegroundWindow() -eq $hwnd) { return $true }
    Start-Sleep -Milliseconds 150
  }
  return $false
}

$trace = New-Object System.Collections.ArrayList
$qaPids = @(Get-Process -Name ferryx -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -like "$qaRoot\*" } | ForEach-Object { $_.Id })
$trace.Add("QA_PIDS=" + ($qaPids -join ',')) | Out-Null
if ($qaPids.Count -eq 0) { Write-Host 'ECHO_ABORT no-qa-process'; exit 3 }

$tops = New-Object System.Collections.ArrayList
$cb = [QaWin+EnumProc]{
  param($h, $l)
  [uint32]$wpid = 0
  [QaWin]::GetWindowThreadProcessId($h, [ref]$wpid) | Out-Null
  if ($qaPids -contains [int]$wpid -and [QaWin]::IsWindowVisible($h) -and [QaWin]::GetParent($h) -eq [IntPtr]::Zero) {
    $t = New-Object System.Text.StringBuilder 256
    [QaWin]::GetWindowText($h, $t, 256) | Out-Null
    if ($t.ToString().Length -gt 0) {
      $r = New-Object QaWin+RECT
      [QaWin]::GetWindowRect($h, [ref]$r) | Out-Null
      $area = [Math]::Abs(($r.R - $r.L) * ($r.B - $r.T))
      $tops.Add([pscustomobject]@{ Hwnd = $h; Pid = [int]$wpid; Title = $t.ToString(); Area = $area }) | Out-Null
    }
  }
  return $true
}
[QaWin]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
if ($tops.Count -eq 0) { Write-Host 'ECHO_ABORT no-visible-qa-window'; exit 3 }
$main = $tops | Sort-Object Area -Descending | Select-Object -First 1
$trace.Add("MAIN hwnd=$($main.Hwnd) pid=$($main.Pid) title=$($main.Title)") | Out-Null

# locate the native terminal child
$termHwnd = [IntPtr]::Zero; $termRect = $null
$kids = New-Object System.Collections.ArrayList
$kcb = [QaWin+EnumProc]{
  param($h, $l)
  $c = New-Object System.Text.StringBuilder 256
  [QaWin]::GetClassName($h, $c, 256) | Out-Null
  if ($c.ToString() -eq 'FerryxNativeTerm' -and [QaWin]::IsWindowVisible($h)) {
    $r2 = New-Object QaWin+RECT
    [QaWin]::GetWindowRect($h, [ref]$r2) | Out-Null
    $script:termHwnd = $h; $script:termRect = $r2
  }
  return $true
}
[QaWin]::EnumChildWindows($main.Hwnd, $kcb, [IntPtr]::Zero) | Out-Null
$trace.Add("TERM hwnd=$termHwnd rect=$($termRect.L),$($termRect.T),$($termRect.R),$($termRect.B)") | Out-Null

Save-Shot (Join-Path $evidence ("echo-before-{0}.png" -f $tag))

# --- focus acquisition (layered, verified; no keys are sent to non-QA surfaces) ---
$ok = $false
$myTid = [QaWin]::GetCurrentThreadId()
$fgHwnd = [QaWin]::GetForegroundWindow()
[uint32]$fgTid = 0
if ($fgHwnd -ne [IntPtr]::Zero) { [QaWin]::GetWindowThreadProcessId($fgHwnd, [ref]$fgTid) | Out-Null }
if ($fgTid -ne 0 -and $fgTid -ne $myTid) {
  [QaWin]::AttachThreadInput($myTid, $fgTid, $true) | Out-Null
  [QaWin]::SetWindowPos($main.Hwnd, [IntPtr]::Zero, 0, 0, 0, 0, 0x0053) | Out-Null  # HWND_TOP, SWP_NOMOVE|NOSIZE|SHOWWINDOW
  [QaWin]::SetForegroundWindow($main.Hwnd) | Out-Null
  [QaWin]::AttachThreadInput($myTid, $fgTid, $false) | Out-Null
  $ok = Wait-Foreground $main.Hwnd
  $trace.Add("AttachThreadInput ok=$ok (fgTid=$fgTid)") | Out-Null
}
if (-not $ok) {
  try { if ((New-Object -ComObject WScript.Shell).AppActivate([int]$main.Pid)) { $ok = Wait-Foreground $main.Hwnd } } catch {}
  $trace.Add("AppActivate ok=$ok") | Out-Null
}
if (-not $ok) {
  [QaWin]::ShowWindow($main.Hwnd, 6) | Out-Null   # SW_MINIMIZE
  Start-Sleep -Milliseconds 400
  [QaWin]::ShowWindow($main.Hwnd, 9) | Out-Null   # SW_RESTORE
  [QaWin]::SetForegroundWindow($main.Hwnd) | Out-Null
  $ok = Wait-Foreground $main.Hwnd
  $trace.Add("MinRestore ok=$ok") | Out-Null
}
if (-not $ok) {
  [QaWin]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)   # ALT down
  [QaWin]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)   # ALT up
  [QaWin]::SetForegroundWindow($main.Hwnd) | Out-Null
  $ok = Wait-Foreground $main.Hwnd
  $trace.Add("AltUnlock ok=$ok") | Out-Null
}

if (-not $ok) {
  $trace.Add('ABORT focus-unobtainable - NO KEYS SENT') | Out-Null
  $trace | ForEach-Object { Write-Host ("TRACE " + $_) }
  [pscustomobject]@{ time = (Get-Date -Format 'o'); mode = 'aborted-focus'; focusVerified = $false; trace = $trace } |
    ConvertTo-Json | Set-Content (Join-Path $evidence ("echo-result-{0}.json" -f $tag))
  exit 4
}

# --- ensure the terminal pane has keyboard focus ---
$fgThread = 0; [uint32]$mtid = 0
[QaWin]::GetWindowThreadProcessId($main.Hwnd, [ref]$mtid) | Out-Null
$gi = New-Object QaWin+GUITHREADINFO
$gi.cbSize = [System.Runtime.InteropServices.Marshal]::SizeOf([type][QaWin+GUITHREADINFO])
$focusHwnd = [IntPtr]::Zero
if ([QaWin]::GetGUIThreadInfo($mtid, [ref]$gi)) { $focusHwnd = $gi.hwndFocus }
$trace.Add("FOCUS hwnd=$focusHwnd (term=$termHwnd)") | Out-Null
if ($termHwnd -ne [IntPtr]::Zero -and $focusHwnd -ne $termHwnd) {
  # click the center of the native terminal rect (inside the foreground QA window)
  $cx = [int](($termRect.L + $termRect.R) / 2); $cy = [int](($termRect.T + $termRect.B) / 2)
  [QaWin]::SetCursorPos($cx, $cy) | Out-Null
  [QaWin]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)  # LEFTDOWN
  [QaWin]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)  # LEFTUP
  Start-Sleep -Milliseconds 400
  $gi2 = New-Object QaWin+GUITHREADINFO
  $gi2.cbSize = [System.Runtime.InteropServices.Marshal]::SizeOf([type][QaWin+GUITHREADINFO])
  if ([QaWin]::GetGUIThreadInfo($mtid, [ref]$gi2)) { $focusHwnd = $gi2.hwndFocus }
  $trace.Add("CLICK term-center ($cx,$cy) focus-now=$focusHwnd") | Out-Null
}
$mode = if ($focusHwnd -eq $termHwnd) { 'sendkeys-terminal-focused' } else { 'sendkeys-app-foreground-focus=' + $focusHwnd }

[System.Windows.Forms.SendKeys]::SendWait($EchoLine + '{ENTER}')
Start-Sleep -Milliseconds 1200   # single render settle
Save-Shot (Join-Path $evidence ("echo-after-{0}.png" -f $tag))

[pscustomobject]@{
  time = (Get-Date -Format 'o'); mode = $mode; echoLine = $EchoLine
  mainHwnd = $main.Hwnd; mainPid = $main.Pid; termHwnd = $termHwnd
  focusVerified = $ok
  trace = $trace
} | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $evidence ("echo-result-{0}.json" -f $tag))
Write-Host ("ECHO_DONE mode=" + $mode)
