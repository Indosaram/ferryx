# Harness preflight for lib/gui.mjs sweepExplorerDialogs, run once per run before the first Win+R.
# Runs as a per-run scheduled task (Ferryx\ferryx-qa-<runId>-sweep) in the interactive session, because
# the SSH agent's session 0 cannot see desktop windows. Steps:
#   1. list every visible top-level #32770 window: hwnd, owning process, and (explorer.exe only) title;
#      write logs\dialog-sweep.candidates.json
#   2. wait (bounded) for logs\dialog-sweep.close.json: the exact hwnds the harness selected
#   3. for each selected hwnd, re-verify it is still the same explorer.exe #32770 window with the same
#      title, post WM_CLOSE to that hwnd only, and wait (bounded) until it no longer exists
#   4. write logs\dialog-sweep.result.json
# No other window is touched; a hwnd that fails re-verification is reported, never closed.
# Report-only: candidates.json also records the foreground window and every other visible, titled
# top-level window (owning process, class, title) so a non-QA window that takes focus during a run
# (run-r6b Q05: RobloxStudioBeta.exe) is on record. Those windows are never closed or killed.
$ErrorActionPreference = 'Stop'
$logs = Join-Path $PSScriptRoot 'logs'
[System.IO.Directory]::CreateDirectory($logs) | Out-Null
$utf8 = New-Object System.Text.UTF8Encoding($false)
function Publish($name, $obj) {
  $path = Join-Path $logs $name
  [System.IO.File]::WriteAllText("$path.tmp", (ConvertTo-Json -InputObject $obj -Depth 6 -Compress), $utf8)
  Move-Item -Force "$path.tmp" $path
}
try {
  Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
public static class DialogSweep {
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("kernel32.dll")] public static extern uint WTSGetActiveConsoleSessionId();
  public static string Cls(IntPtr h) { var s = new StringBuilder(256); GetClassName(h, s, s.Capacity); return s.ToString(); }
  public static string Title(IntPtr h) { var s = new StringBuilder(512); GetWindowText(h, s, s.Capacity); return s.ToString(); }
  public static string Proc(IntPtr h) { uint pid; GetWindowThreadProcessId(h, out pid); try { return Process.GetProcessById((int)pid).ProcessName + ".exe"; } catch { return ""; } }
  public static List<IntPtr> Dialogs() {
    var l = new List<IntPtr>();
    EnumWindows((h, x) => { if (IsWindowVisible(h) && Cls(h) == "#32770") l.Add(h); return true; }, IntPtr.Zero);
    return l;
  }
  public static List<IntPtr> Titled() {
    var l = new List<IntPtr>();
    EnumWindows((h, x) => { if (IsWindowVisible(h) && Title(h).Length > 0) l.Add(h); return true; }, IntPtr.Zero);
    return l;
  }
  public static bool Close(IntPtr h) { return PostMessage(h, 0x0010, IntPtr.Zero, IntPtr.Zero); }
}
"@
  $me = [System.Diagnostics.Process]::GetCurrentProcess()
  $fg = [DialogSweep]::GetForegroundWindow()
  $seen = @{}
  $windows = @()
  foreach ($h in [DialogSweep]::Dialogs()) {
    $proc = [DialogSweep]::Proc($h)
    $title = $null
    if ($proc -ieq 'explorer.exe') { $title = [DialogSweep]::Title($h) }
    $seen[[int64]$h] = @{ proc = $proc; title = $title }
    $windows += [ordered]@{ hwnd = [int64]$h; processName = $proc; className = '#32770'; topLevel = $true; title = $title; foreground = ($h -eq $fg) }
  }
  $foreign = @()
  foreach ($h in [DialogSweep]::Titled()) {
    if ($seen.ContainsKey([int64]$h)) { continue }
    $foreign += [ordered]@{ hwnd = [int64]$h; processName = [DialogSweep]::Proc($h); className = [DialogSweep]::Cls($h); title = [DialogSweep]::Title($h); foreground = ($h -eq $fg) }
  }
  $fgInfo = if ($fg -ne [IntPtr]::Zero) { [ordered]@{ hwnd = [int64]$fg; processName = [DialogSweep]::Proc($fg); className = [DialogSweep]::Cls($fg); title = [DialogSweep]::Title($fg) } } else { $null }
  Publish 'dialog-sweep.candidates.json' ([ordered]@{ pid = $me.Id; sessionId = $me.SessionId; activeConsoleSessionId = [DialogSweep]::WTSGetActiveConsoleSessionId(); windows = @($windows); foreground = $fgInfo; visibleWindows = @($foreign) })
  $closeFile = Join-Path $logs 'dialog-sweep.close.json'
  $until = (Get-Date).AddSeconds(30)
  while (-not (Test-Path $closeFile) -and (Get-Date) -lt $until) { Start-Sleep -Milliseconds 250 }
  $results = @()
  if (Test-Path $closeFile) {
    $req = Get-Content -Raw -Path $closeFile | ConvertFrom-Json
    foreach ($n in @($req.hwnds)) {
      if ($null -eq $n) { continue }
      $h = [IntPtr][int64]$n
      $was = $seen[[int64]$n]
      $verified = ($null -ne $was) -and [DialogSweep]::IsWindow($h) -and ([DialogSweep]::Cls($h) -eq '#32770') -and ([DialogSweep]::Proc($h) -ieq 'explorer.exe') -and ([DialogSweep]::Title($h) -eq $was.title)
      $posted = $false
      $closed = $false
      if ($verified) {
        $posted = [DialogSweep]::Close($h)
        $deadline = (Get-Date).AddSeconds(3)
        while ([DialogSweep]::IsWindow($h) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100 }
        $closed = -not [DialogSweep]::IsWindow($h)
      }
      $results += [ordered]@{ hwnd = [int64]$n; title = $was.title; verified = $verified; posted = $posted; closed = $closed }
    }
    Publish 'dialog-sweep.result.json' ([ordered]@{ requested = $true; closed = @($results) })
  } else {
    Publish 'dialog-sweep.result.json' ([ordered]@{ requested = $false; closed = @() })
  }
} catch {
  [System.IO.File]::WriteAllText((Join-Path $logs 'dialog-sweep.err'), ($_ | Out-String) + $_.ScriptStackTrace, $utf8)
  exit 1
}

