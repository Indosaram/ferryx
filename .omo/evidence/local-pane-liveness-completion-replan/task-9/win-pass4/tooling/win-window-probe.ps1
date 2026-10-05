$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$bin = Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$t9 = Join-Path $base 'task9-91d447e1'
$probe = Join-Path $t9 'window-probe'
if (Test-Path $probe) { Remove-Item -Recurse -Force $probe }
foreach ($d in @('barriers','data','runtime','home')) { New-Item -ItemType Directory -Force -Path (Join-Path $probe $d) | Out-Null }
$env:FERRYX_DATA_DIR = Join-Path $probe 'data'
$env:FERRYX_RUNTIME_DIR = Join-Path $probe 'runtime'
$env:FERRYX_QA_BARRIER_DIR = Join-Path $probe 'barriers'
$env:FERRYX_QA_RUN_ID = 'qa-run-windowprobe'
$env:FERRYX_QA_OPERATION_ID = 'qa-op-windowprobe'
$env:FERRYX_QA_FIXTURE_KINDS = 'source'
Write-Output ("HOST_LOAD=" + (Get-CimInstance Win32_Processor | Select-Object -ExpandProperty LoadPercentage))
Write-Output ("INTERACTIVE_SESSION=" + [System.Environment]::UserInteractive)
Write-Output ("SESSION_ID=" + (Get-Process -Id $PID).SessionId)
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$p = Start-Process -FilePath $bin -PassThru -WorkingDirectory $root
Write-Output "PROBE_PID=$($p.Id)"
# Poll MainWindowHandle for up to 20 s: does an owned window EVER appear?
$firstNonZero = $null
$deadline = (Get-Date).AddSeconds(20)
while ((Get-Date) -lt $deadline) {
  $p.Refresh()
  $h = $p.MainWindowHandle
  if ($h -ne 0) { $firstNonZero = $sw.ElapsedMilliseconds; break }
  Start-Sleep -Milliseconds 250
}
Write-Output ("MAINWINDOWHANDLE_FIRST_NONZERO_MS=" + $(if ($null -eq $firstNonZero) { 'NEVER_WITHIN_20000' } else { $firstNonZero }))
$p.Refresh()
Write-Output ("FINAL_MAINWINDOWHANDLE=" + $p.MainWindowHandle)
Write-Output ("HAS_EXITED=" + $p.HasExited)
Write-Output "=== any window owned by ANY process of this image (incl. children) ==="
$mine = Get-CimInstance Win32_Process -Filter "Name='ferryx.exe'" | Where-Object { $_.ExecutablePath -like '*source-21dea3c0*' }
foreach ($q in $mine) {
  $proc = Get-Process -Id $q.ProcessId -ErrorAction SilentlyContinue
  if ($proc) { Write-Output ("  PID=" + $q.ProcessId + " PPID=" + $q.ParentProcessId + " MainWindowHandle=" + $proc.MainWindowHandle + " Title='" + $proc.MainWindowTitle + "'") }
}
Write-Output "=== enumeration of top-level windows whose pid is ours (Win32) ==="
Add-Type @"
  using System;using System.Text;using System.Runtime.InteropServices;
  public class W { public delegate bool CB(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(CB cb, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n); }
"@
$pids = @($mine | ForEach-Object { [uint32]$_.ProcessId })
$found = New-Object System.Collections.Generic.List[string]
$cb = [W+CB]{ param($h,$l)
  $op = 0; [W]::GetWindowThreadProcessId($h, [ref]$op) | Out-Null
  if ($pids -contains $op) {
    $t = New-Object System.Text.StringBuilder 256; [W]::GetWindowTextW($h,$t,256) | Out-Null
    $c = New-Object System.Text.StringBuilder 256; [W]::GetClassNameW($h,$c,256) | Out-Null
    $found.Add("    hwnd=$h pid=$op visible=" + [W]::IsWindowVisible($h) + " class='" + $c.ToString() + "' title='" + $t.ToString() + "'")
  }
  return $true }
[W]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
foreach ($f in $found) { Write-Output $f }
Write-Output ("OWNED_TOPLEVEL_WINDOWS=" + $found.Count)
$mine | ForEach-Object { taskkill /T /F /PID $_.ProcessId 2>&1 | Out-String | Write-Output }
Write-Output "WINDOW_PROBE_DONE"
