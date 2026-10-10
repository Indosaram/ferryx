// VERIFIER PROBE (pass 13, staged copy only - never the candidate tree).
//
// The runner now drains the app's stdio always-on (562144b3), but the two things
// pass 12b left unmeasured are still not in any artifact: the app's UIA tree at
// the moment of the pane step, and the isolated daemon's session inventory during
// the run. This module records both, plus the barrier receipts, into one JSONL in
// the run's own evidence dir, so a blocked run still carries the measurement.
import { spawnSync } from 'node:child_process';
import { appendFileSync, existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

const PS_TREE = String.raw`
$ErrorActionPreference='Continue'
$targetPid = __PID__
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public class VProbeWin {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder t, int c);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder t, int c);
}
"@
$wins = New-Object System.Collections.ArrayList
$cb = [VProbeWin+EnumProc]{ param($h, $l)
  $owner = [uint32]0
  [VProbeWin]::GetWindowThreadProcessId($h, [ref]$owner) | Out-Null
  if ($owner -eq [uint32]$targetPid -and [VProbeWin]::IsWindowVisible($h)) {
    $t = New-Object System.Text.StringBuilder 256; [VProbeWin]::GetWindowTextW($h, $t, 256) | Out-Null
    $c = New-Object System.Text.StringBuilder 256; [VProbeWin]::GetClassNameW($h, $c, 256) | Out-Null
    $wins.Add([ordered]@{ hwnd=$h.ToInt64(); title=$t.ToString(); className=$c.ToString() }) | Out-Null
  }
  return $true }
[VProbeWin]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
$out = New-Object System.Collections.ArrayList
foreach ($w in $wins) {
  $e = [ordered]@{ hwnd=$w.hwnd; title=$w.title; className=$w.className; total=0; named=0; exactNewTerminal=0; tabItems=@(); errorText=@(); names=@() }
  try {
    $r = [System.Windows.Automation.AutomationElement]::FromHandle([System.IntPtr]::new([int64]$w.hwnd))
    if ($r -ne $null) {
      $all = $r.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
      $e.total = $all.Count
      $names = New-Object System.Collections.ArrayList; $tabs = New-Object System.Collections.ArrayList; $errs = New-Object System.Collections.ArrayList
      for ($i = 0; $i -lt $all.Count; $i++) {
        try {
          $n = [string]$all.Item($i).Current.Name
          $ct = [string]$all.Item($i).Current.ControlType.ProgrammaticName
          if ($n.Length -gt 0) {
            $e.named = $e.named + 1
            if ($n -ceq 'New Terminal') { $e.exactNewTerminal = $e.exactNewTerminal + 1 }
            if ($names.Count -lt 300) { $names.Add($ct + '|' + $n) | Out-Null }
          }
          if ($ct -eq 'ControlType.TabItem') { $tabs.Add($n) | Out-Null }
          if ($n -match '(?i)fail|error|retry|unable|cannot|denied|refus|not found|invalid|attach|unavailable') { $errs.Add($ct + '|' + $n) | Out-Null }
        } catch { }
      }
      $e.names = @($names); $e.tabItems = @($tabs); $e.errorText = @($errs)
    }
  } catch { $e.error = $_.Exception.Message }
  $out.Add($e) | Out-Null
}
Write-Output (([ordered]@{ windows = @($out) }) | ConvertTo-Json -Compress -Depth 8)
`;

function dumpTree(pid) {
  const script = PS_TREE.replace('__PID__', String(pid));
  const res = spawnSync('powershell', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', script], {
    encoding: 'utf8', timeout: 90_000, maxBuffer: 32 * 1024 * 1024, windowsHide: true,
  });
  if (res.error) return { error: String(res.error.message) };
  const text = String(res.stdout ?? '').trim();
  if (!text) return { error: 'no stdout', stderr: String(res.stderr ?? '').slice(0, 400) };
  try { return JSON.parse(text); } catch (error) { return { error: 'unparseable', raw: text.slice(0, 400) }; }
}

// The daemon's own control wire, bounded at every blocking step.
function listSessions(isolationRoot) {
  const runtime = join(isolationRoot, 'runtime');
  const portFile = join(runtime, 'daemon.port');
  const tokenFile = join(runtime, 'daemon.token');
  if (!existsSync(portFile) || !existsSync(tokenFile)) return { error: 'no runtime files' };
  const port = Number(readFileSync(portFile, 'utf8').trim());
  const token = readFileSync(tokenFile, 'utf8').trim();
  if (!Number.isFinite(port)) return { error: 'bad port' };
  const ps = String.raw`
$ErrorActionPreference='Continue'
try {
  $c = New-Object System.Net.Sockets.TcpClient
  $c.ReceiveTimeout = 2000; $c.SendTimeout = 2000
  $iar = $c.BeginConnect('127.0.0.1', __PORT__, $null, $null)
  if (-not $iar.AsyncWaitHandle.WaitOne(1500)) { $c.Close(); Write-Output '{"error":"connect timeout"}'; exit 0 }
  $c.EndConnect($iar)
  $st = $c.GetStream(); $st.ReadTimeout = 2000; $st.WriteTimeout = 2000
  $w = New-Object System.IO.StreamWriter($st); $w.NewLine = [char]10; $w.AutoFlush = $true
  $rd = New-Object System.IO.StreamReader($st)
  $w.WriteLine('{"type":"handshake","version":5,"token":"__TOKEN__"}')
  $null = $rd.ReadLine()
  $w.WriteLine('{"type":"listSessions"}')
  $line = $rd.ReadLine()
  $c.Close()
  Write-Output $line
} catch { Write-Output ('{"error":"' + $_.Exception.Message + '"}') }
`.replace('__PORT__', String(port)).replace('__TOKEN__', token);
  const res = spawnSync('powershell', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', ps], {
    encoding: 'utf8', timeout: 30_000, maxBuffer: 4 * 1024 * 1024, windowsHide: true,
  });
  const text = String(res.stdout ?? '').trim();
  if (!text) return { error: 'no stdout', stderr: String(res.stderr ?? '').slice(0, 300) };
  try {
    const parsed = JSON.parse(text);
    return parsed.sessions ? { sessions: parsed.sessions, count: parsed.sessions.length } : parsed;
  } catch { return { error: 'unparseable', raw: text.slice(0, 300) }; }
}

function receiptState(isolationRoot) {
  const dir = join(isolationRoot, 'barriers');
  if (!existsSync(dir)) return [];
  return readdirSync(dir).map(name => {
    const path = join(dir, name);
    let bytes = 0; let lines = 0;
    try { bytes = statSync(path).size; lines = readFileSync(path, 'utf8').split('\n').filter(Boolean).length; } catch { /* report what we have */ }
    return { name, bytes, lines };
  });
}

export function probePane({ pid, isolationRoot, evidenceDir, label, session }) {
  const record = {
    label, session: session ?? null, atMs: Date.now(),
    tree: dumpTree(pid),
    inventory: listSessions(isolationRoot),
    receipts: receiptState(isolationRoot),
  };
  try { appendFileSync(join(evidenceDir, 'verifier-probe.jsonl'), JSON.stringify(record) + '\n'); } catch { /* diagnostic only */ }
  return record;
}
