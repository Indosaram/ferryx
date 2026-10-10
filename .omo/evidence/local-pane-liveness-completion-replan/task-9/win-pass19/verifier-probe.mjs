// VERIFIER PROBE (pass 18, staged copy only - never the candidate tree).
//
// Records, at the pane step: the app's UIA tree, the isolated daemon's session inventory,
// and the barrier receipts - each stamped with the interpreter AND version that produced
// it. Pass 17's trap 14 cost a measurement because a probe result did not say which
// interpreter ran it (ProcessStartInfo.ArgumentList does not exist under Windows
// PowerShell 5.1), so every result now carries that stamp.
import { spawnSync } from 'node:child_process';
import { appendFileSync, existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

// Resolve the interpreter ONCE, preferring pwsh 7, and capture its version so no result
// can be read without knowing what produced it.
function resolveInterpreter() {
  for (const candidate of ['pwsh', 'powershell']) {
    try {
      const v = spawnSync(candidate, ['-NoProfile', '-Command', '$PSVersionTable.PSVersion.ToString()'], {
        encoding: 'utf8', timeout: 20_000, windowsHide: true,
      });
      if (v.error || v.status !== 0) continue;
      const version = String(v.stdout ?? '').trim();
      if (version) return { interpreter: candidate, version, kind: candidate === 'pwsh' ? 'PowerShell 7+' : 'Windows PowerShell 5.1' };
    } catch { /* try the next candidate */ }
  }
  return { interpreter: null, version: null, kind: null };
}
const INTERPRETER = resolveInterpreter();

function runPs(script, timeoutMs) {
  if (!INTERPRETER.interpreter) return { error: 'no powershell interpreter available' };
  const res = spawnSync(INTERPRETER.interpreter, ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', script], {
    encoding: 'utf8', timeout: timeoutMs, maxBuffer: 32 * 1024 * 1024, windowsHide: true,
  });
  if (res.error) return { error: String(res.error.message) };
  const text = String(res.stdout ?? '').trim();
  if (!text) return { error: 'no stdout', stderr: String(res.stderr ?? '').slice(0, 400) };
  return { text };
}

const TREE_SCRIPT = (pid) => String.raw`
$ErrorActionPreference='Continue'
$targetPid = ${pid}
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
  $e = [ordered]@{ hwnd=$w.hwnd; title=$w.title; className=$w.className; total=0; named=0; exactNewTerminal=0; nativeInput=$false; attachFailure=$false; tabItems=@(); errorText=@(); names=@() }
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
            if ($n -ceq 'Native terminal input') { $e.nativeInput = $true }
            if ($n -ceq 'Failed to attach native terminal') { $e.attachFailure = $true }
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
  const res = runPs(TREE_SCRIPT(pid), 90_000);
  if (res.error) return res;
  try { return JSON.parse(res.text); } catch { return { error: 'unparseable', raw: res.text.slice(0, 400) }; }
}

// The daemon's own control wire, bounded at every blocking step. Newline discipline is
// '\\n' (trap 2) and the connect is bounded (trap 3).
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
  $iar = $c.BeginConnect('127.0.0.1', ${port}, $null, $null)
  if (-not $iar.AsyncWaitHandle.WaitOne(1500)) { $c.Close(); Write-Output '{"error":"connect timeout"}'; exit 0 }
  $c.EndConnect($iar)
  $st = $c.GetStream(); $st.ReadTimeout = 2000; $st.WriteTimeout = 2000
  $w = New-Object System.IO.StreamWriter($st); $w.NewLine = [char]10; $w.AutoFlush = $true
  $rd = New-Object System.IO.StreamReader($st)
  $w.WriteLine('{"type":"handshake","version":5,"token":"${token}"}')
  $null = $rd.ReadLine()
  $w.WriteLine('{"type":"listSessions"}')
  $line = $rd.ReadLine()
  $c.Close()
  Write-Output $line
} catch { Write-Output ('{"error":"' + $_.Exception.Message + '"}') }
`;
  const res = runPs(ps, 30_000);
  if (res.error) return res;
  try {
    const parsed = JSON.parse(res.text);
    return parsed.sessions ? { sessions: parsed.sessions, count: parsed.sessions.length } : parsed;
  } catch { return { error: 'unparseable', raw: res.text.slice(0, 300) }; }
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
    // Every result says what produced it (trap 14: an interpreter/version mismatch
    // once made a harness bug look like a host hang).
    interpreter: INTERPRETER.interpreter,
    interpreterVersion: INTERPRETER.version,
    interpreterKind: INTERPRETER.kind,
    tree: dumpTree(pid),
    inventory: listSessions(isolationRoot),
    receipts: receiptState(isolationRoot),
  };
  try { appendFileSync(join(evidenceDir, 'verifier-probe.jsonl'), JSON.stringify(record) + '\n'); } catch { /* diagnostic only */ }
  return record;
}
