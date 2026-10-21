# Read-only focus observer for the daemon-lifetime QA harness (lib/gui.mjs launchWindows).
# A per-run scheduled task (Ferryx\ferryx-qa-<runId>-obs-<tag>, run only while the user is logged
# on) starts it through
# conhost --headless so it runs in the interactive session without opening a window; the SSH agent
# runs in session 0 and sees no session-1 windows (measured, Q00-run-dialog-capability-probe.md).
# It only observes: SetWinEventHook (WINEVENT_OUTOFCONTEXT) for focus, foreground and value
# changes, class names, owning process, and whether one Edit's text equals the expected path. It
# never sends input, never changes focus and never starts a process. All GUI input stays with
# MahoRD. The harness decides identity from the records; this script does not.
# Files in <runDir>\logs (teardown copies them into the evidence):
#   observer-<tag>.ready.json    after every hook, the timer and the file watcher are registered
#   observer-<tag>.events.jsonl  one record per line, each ending in ,"seq":<n>}
#   observer-<tag>.err           any terminating error
# Control files the harness creates in logs (FileSystemWatcher -> thread message, no polling):
#   observer-<tag>.arm           start emitting focus/foreground records (acknowledged by "armed")
#   observer-<tag>.target-<hwnd> latch that Edit: from then on, each EVENT_OBJECT_VALUECHANGE of it
#                                (the system raises it for edit controls) reports text equality,
#                                and focus leaving its root window reports "lost"
#   observer-<tag>.stop          unhook and exit. A WM_TIMER at deadlineMs bounds the run.
param([Parameter(Mandatory = $true)][string]$Tag)
$ErrorActionPreference = 'Stop'
$logs = Join-Path $PSScriptRoot 'logs'
[System.IO.Directory]::CreateDirectory($logs) | Out-Null
try {
  $cfg = Get-Content -Raw -Path (Join-Path $PSScriptRoot "observer-$Tag.args.json") | ConvertFrom-Json
  Add-Type -TypeDefinition @"
using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
public static class FocusObserver {
  delegate void WinEventProc(IntPtr hook, uint ev, IntPtr hwnd, int idObject, int idChild, uint thread, uint time);
  [StructLayout(LayoutKind.Sequential)] struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam; public IntPtr lParam; public uint time; public int x; public int y; }
  [DllImport("user32.dll")] static extern IntPtr SetWinEventHook(uint min, uint max, IntPtr mod, WinEventProc proc, uint pid, uint tid, uint flags);
  [DllImport("user32.dll")] static extern bool UnhookWinEvent(IntPtr hook);
  [DllImport("user32.dll")] static extern int GetMessage(out MSG msg, IntPtr hwnd, uint min, uint max);
  [DllImport("user32.dll")] static extern bool PostThreadMessage(uint tid, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern UIntPtr SetTimer(IntPtr hwnd, UIntPtr id, uint ms, IntPtr fn);
  [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr hwnd, uint flags);
  [DllImport("user32.dll")] static extern IntPtr GetParent(IntPtr hwnd);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr hwnd, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr SendMessageTimeout(IntPtr hwnd, uint msg, IntPtr w, StringBuilder l, uint flags, uint timeout, out IntPtr result);
  [DllImport("user32.dll", EntryPoint = "SendMessageTimeoutW")] static extern IntPtr SendMessageTimeoutPtr(IntPtr hwnd, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  [DllImport("kernel32.dll")] static extern uint WTSGetActiveConsoleSessionId();
  const uint EVENT_SYSTEM_FOREGROUND = 0x0003, EVENT_OBJECT_FOCUS = 0x8005, EVENT_OBJECT_VALUECHANGE = 0x800E;
  const uint WM_TIMER = 0x0113, WM_APP_ARM = 0x8001, WM_APP_TARGET = 0x8002, WM_APP_STOP = 0x8003, WM_GETTEXT = 0x000D, WM_GETTEXTLENGTH = 0x000E;
  const uint GA_ROOT = 2, SMTO_ABORTIFHUNG = 2;
  static WinEventProc proc; // kept reachable while hooked
  static string events, expected;
  static IntPtr target = IntPtr.Zero, targetRoot = IntPtr.Zero;
  static bool armed, matched;
  static int seq;

  static string Cls(IntPtr h) { var s = new StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }
  static string ProcName(IntPtr h) { uint pid; GetWindowThreadProcessId(h, out pid); try { return Process.GetProcessById((int)pid).ProcessName + ".exe"; } catch { return ""; } }
  static string J(string v) { return "\"" + (v ?? "").Replace("\\", "\\\\").Replace("\"", "\\\"") + "\""; }
  static void Emit(string kind, string rest) { seq++; File.AppendAllText(events, "{\"kind\":" + J(kind) + (rest.Length > 0 ? "," + rest : "") + ",\"seq\":" + seq + "}" + Environment.NewLine); }
  // Identity fields only: handles, classes, owning process, foreground. No window or control text.
  static string Record(IntPtr focus) {
    IntPtr root = GetAncestor(focus, GA_ROOT);
    return "\"focusHwnd\":" + focus.ToInt64() + ",\"rootHwnd\":" + root.ToInt64() + ",\"processName\":" + J(ProcName(root)) + ",\"rootClass\":" + J(Cls(root))
      + ",\"focusedClass\":" + J(Cls(focus)) + ",\"parentClass\":" + J(Cls(GetParent(focus))) + ",\"foreground\":" + (GetForegroundWindow() == root ? "true" : "false");
  }
  // Emits whether the latched Edit's text equals expectedText, and its length; the text itself is
  // never written (before Ctrl+A it may hold the user's previous Run entry).
  static void CheckText() {
    if (target == IntPtr.Zero || matched) return;
    IntPtr len;
    if (SendMessageTimeoutPtr(target, WM_GETTEXTLENGTH, IntPtr.Zero, IntPtr.Zero, SMTO_ABORTIFHUNG, 1000, out len) == IntPtr.Zero) { Emit("text", "\"match\":false,\"error\":\"WM_GETTEXTLENGTH timed out\""); return; }
    var s = new StringBuilder(len.ToInt32() + 1);
    IntPtr got;
    if (SendMessageTimeout(target, WM_GETTEXT, new IntPtr(s.Capacity), s, SMTO_ABORTIFHUNG, 1000, out got) == IntPtr.Zero) { Emit("text", "\"match\":false,\"error\":\"WM_GETTEXT timed out\""); return; }
    matched = string.Equals(s.ToString(), expected, StringComparison.OrdinalIgnoreCase);
    Emit("text", "\"match\":" + (matched ? "true" : "false") + ",\"length\":" + s.Length);
  }
  static void OnEvent(IntPtr hook, uint ev, IntPtr hwnd, int idObject, int idChild, uint thread, uint time) {
    try {
      if (!armed || hwnd == IntPtr.Zero) return;
      if (ev == EVENT_OBJECT_VALUECHANGE) { if (target != IntPtr.Zero && (hwnd == target || hwnd == GetParent(target))) CheckText(); return; }
      if (ev == EVENT_SYSTEM_FOREGROUND) { Emit("foreground", Record(hwnd)); return; }
      Emit("focus", Record(hwnd));
      if (targetRoot != IntPtr.Zero && GetAncestor(hwnd, GA_ROOT) != targetRoot) Emit("lost", Record(hwnd));
    } catch (Exception e) { Emit("event-error", "\"error\":" + J(e.GetType().FullName)); }
  }
  public static string Run(string logs, string tag, string nonce, string expected_, uint deadlineMs) {
    events = Path.Combine(logs, "observer-" + tag + ".events.jsonl");
    expected = expected_;
    uint tid = GetCurrentThreadId();
    proc = new WinEventProc(OnEvent);
    IntPtr[] hooks = { SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, IntPtr.Zero, proc, 0, 0, 0),
      SetWinEventHook(EVENT_OBJECT_FOCUS, EVENT_OBJECT_FOCUS, IntPtr.Zero, proc, 0, 0, 0),
      SetWinEventHook(EVENT_OBJECT_VALUECHANGE, EVENT_OBJECT_VALUECHANGE, IntPtr.Zero, proc, 0, 0, 0) };
    foreach (var h in hooks) if (h == IntPtr.Zero) throw new Exception("SetWinEventHook returned NULL");
    string targetPrefix = "observer-" + tag + ".target-";
    var watcher = new FileSystemWatcher(logs);
    watcher.Created += (s, e) => {
      long hwnd;
      if (e.Name == "observer-" + tag + ".arm") PostThreadMessage(tid, WM_APP_ARM, IntPtr.Zero, IntPtr.Zero);
      else if (e.Name.StartsWith(targetPrefix) && long.TryParse(e.Name.Substring(targetPrefix.Length), out hwnd)) PostThreadMessage(tid, WM_APP_TARGET, new IntPtr(hwnd), IntPtr.Zero);
      else if (e.Name == "observer-" + tag + ".stop") PostThreadMessage(tid, WM_APP_STOP, IntPtr.Zero, IntPtr.Zero);
    };
    watcher.EnableRaisingEvents = true;
    if (SetTimer(IntPtr.Zero, UIntPtr.Zero, deadlineMs, IntPtr.Zero) == UIntPtr.Zero) throw new Exception("SetTimer failed");
    var me = Process.GetCurrentProcess();
    string ready = Path.Combine(logs, "observer-" + tag + ".ready.json");
    File.WriteAllText(ready + ".tmp", "{\"pid\":" + me.Id + ",\"sessionId\":" + me.SessionId + ",\"activeConsoleSessionId\":" + WTSGetActiveConsoleSessionId()
      + ",\"nonce\":" + J(nonce) + ",\"hooks\":" + hooks.Length + ",\"start\":\"" + me.StartTime.ToFileTimeUtc() + "\"}");
    File.Move(ready + ".tmp", ready);
    string reason = "deadline";
    MSG msg;
    while (GetMessage(out msg, IntPtr.Zero, 0, 0) > 0) {
      if (msg.message == WM_TIMER) break;
      if (msg.message == WM_APP_STOP) { reason = "stop"; break; }
      if (msg.message == WM_APP_ARM && !armed) { armed = true; Emit("armed", "\"pid\":" + me.Id); }
      if (msg.message == WM_APP_TARGET && target == IntPtr.Zero) {
        target = msg.wParam;
        targetRoot = GetAncestor(target, GA_ROOT);
        Emit("latched", Record(target));
      }
    }
    watcher.Dispose();
    foreach (var h in hooks) UnhookWinEvent(h);
    Emit("exit", "\"reason\":" + J(reason));
    return reason;
  }
}
"@
  [void][FocusObserver]::Run($logs, $Tag, [string]$cfg.nonce, [string]$cfg.expectedText, [uint32]$cfg.deadlineMs)
} catch {
  [System.IO.File]::WriteAllText((Join-Path $logs "observer-$Tag.err"), ($_ | Out-String) + $_.ScriptStackTrace)
  exit 1
}
