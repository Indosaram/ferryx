param(
  [string]$Tag = 'capture',
  [int]$TimeoutSec = 90
)
$ErrorActionPreference = 'Continue'
$base = 'C:\Users\sook\ferryx-pane-completion'
$root = Join-Path $base 'source-21dea3c0'
$bin  = Join-Path $root 'src-tauri\target\debug\ferryx.exe'
$bun  = 'C:\Users\sook\.bun\bin\bun.exe'
$iso  = Join-Path $base ('cap12\' + $Tag)
$outPath = Join-Path $base ('capture12-' + $Tag + '.json')
$log = Join-Path $iso 'logs'
foreach ($d in @($iso, $log, (Join-Path $iso 'data'), (Join-Path $iso 'runtime'), (Join-Path $iso 'session'), (Join-Path $iso 'home'), (Join-Path $iso 'barriers'))) { New-Item -ItemType Directory -Force -Path $d | Out-Null }

Remove-Item $outPath -Force -ErrorAction SilentlyContinue
$res = [ordered]@{ tag=$Tag; startedAt=(Get-Date).ToString('o'); stagedRoot=$root; iso=$iso }
$procs = New-Object System.Collections.ArrayList

function Save { $res | ConvertTo-Json -Depth 12 | Set-Content $outPath -Encoding UTF8 }
$script:probeErr = ''
function ListSessions([string]$rt) {
  try {
    $pf = Join-Path $rt 'daemon.port'; $tf = Join-Path $rt 'daemon.token'
    if (-not (Test-Path $pf)) { $script:probeErr = 'no port file'; return $null }
    if (-not (Test-Path $tf)) { $script:probeErr = 'no token file'; return $null }
    $port = [int]((Get-Content $pf -Raw).Trim()); $token = (Get-Content $tf -Raw).Trim()
    $c = New-Object System.Net.Sockets.TcpClient
    $c.ReceiveTimeout = 1500; $c.SendTimeout = 1500
    $iar = $c.BeginConnect('127.0.0.1', $port, $null, $null)
    if (-not $iar.AsyncWaitHandle.WaitOne(1200)) { $c.Close(); return $null }
    $c.EndConnect($iar)
    $st = $c.GetStream(); $st.ReadTimeout = 1500; $st.WriteTimeout = 1500
    $w = New-Object System.IO.StreamWriter($st); $w.NewLine = [char]10; $w.AutoFlush = $true
    $rd = New-Object System.IO.StreamReader($st)
    $w.WriteLine('{"type":"handshake","version":5,"token":"' + $token + '"}')
    $null = $rd.ReadLine()
    $w.WriteLine('{"type":"listSessions"}')
    $line = $rd.ReadLine()
    $c.Close()
    if (-not $line) { $script:probeErr = 'no response line'; return $null }
    return @(($line | ConvertFrom-Json).sessions)
  } catch { $script:probeErr = $_.Exception.Message; return $null }
}

# ---- 1. frontend ----
$srvLog = Join-Path $iso 'serve-dist.log'
# serve-dist.mjs joins the request path onto argv[2], so it must receive the DIST dir
# (the harness passes ui/dist). Passing ui/ serves Vite's dev shell -> blank page.
$srv = Start-Process -FilePath $bun -ArgumentList @((Join-Path $base 'serve-dist.mjs'), (Join-Path $root 'ui\dist')) -PassThru -WindowStyle Hidden -RedirectStandardOutput $srvLog -RedirectStandardError (Join-Path $iso 'serve-dist.err')
$procs.Add(@{ pid=$srv.Id; role='serve-dist' }) | Out-Null
$res.serverPid = $srv.Id
$ready = $false
for ($i = 0; $i -lt 60; $i++) {
  Start-Sleep -Milliseconds 500
  try {
    $tc = New-Object System.Net.Sockets.TcpClient
    $iar = $tc.BeginConnect('127.0.0.1', 5173, $null, $null)
    if ($iar.AsyncWaitHandle.WaitOne(800)) { $tc.EndConnect($iar); $ready = $true; $tc.Close(); break }
    $tc.Close()
  } catch { }
}
$res.frontendReady = $ready
if (-not $ready) { $res.note = 'frontend not ready'; Save; Write-Output 'CAPTURE12_DONE'; exit 1 }

# ---- 2. app, with the session override so the empty state renders ----
$env:Path = $env:Path
$appEnv = @{
  FERRYX_DATA_DIR    = (Join-Path $iso 'data')
  FERRYX_RUNTIME_DIR = (Join-Path $iso 'runtime')
  FERRYX_SESSION_DIR = (Join-Path $iso 'session')
  FERRYX_QA_BARRIER_DIR = (Join-Path $iso 'barriers')
  HOME = (Join-Path $iso 'home')
  USERPROFILE = (Join-Path $iso 'home')
}
$old = @{}
foreach ($k in $appEnv.Keys) { $old[$k] = [Environment]::GetEnvironmentVariable($k); [Environment]::SetEnvironmentVariable($k, $appEnv[$k]) }
$appOut = Join-Path $iso 'app.stdout'; $appErr = Join-Path $iso 'app.stderr'
$app = Start-Process -FilePath $bin -PassThru -WindowStyle Hidden -WorkingDirectory $root -RedirectStandardOutput $appOut -RedirectStandardError $appErr
foreach ($k in $appEnv.Keys) { [Environment]::SetEnvironmentVariable($k, $old[$k]) }
$procs.Add(@{ pid=$app.Id; role='app' }) | Out-Null
$res.appPid = $app.Id

# wait for an owned window
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public class Cap12Win {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder t, int c);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder t, int c);
}
"@
function OwnedWindows([int]$targetPid) {
  $list = New-Object System.Collections.ArrayList
  $cb = [Cap12Win+EnumProc]{ param($h, $l)
    $owner = [uint32]0
    [Cap12Win]::GetWindowThreadProcessId($h, [ref]$owner) | Out-Null
    if ($owner -eq [uint32]$targetPid -and [Cap12Win]::IsWindowVisible($h)) {
      $t = New-Object System.Text.StringBuilder 256; [Cap12Win]::GetWindowTextW($h, $t, 256) | Out-Null
      $c = New-Object System.Text.StringBuilder 256; [Cap12Win]::GetClassNameW($h, $c, 256) | Out-Null
      $list.Add([ordered]@{ hwnd=$h.ToInt64(); title=$t.ToString(); className=$c.ToString() }) | Out-Null
    }
    return $true }
  [Cap12Win]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
  return $list
}
function DumpTree([int]$targetPid, [string]$label) {
  $wins = OwnedWindows $targetPid
  $out = New-Object System.Collections.ArrayList
  foreach ($w in $wins) {
    $entry = [ordered]@{ hwnd=$w.hwnd; title=$w.title; className=$w.className; total=0; named=0; exactNewTerminal=0; hasDocument=$false; tabItems=@(); errorText=@(); names=@() }
    try {
      $r = [System.Windows.Automation.AutomationElement]::FromHandle([System.IntPtr]::new([int64]$w.hwnd))
      if ($r -ne $null) {
        $all = $r.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
        $entry.total = $all.Count
        $names = New-Object System.Collections.ArrayList
        $tabs = New-Object System.Collections.ArrayList
        $errs = New-Object System.Collections.ArrayList
        for ($i = 0; $i -lt $all.Count; $i++) {
          try {
            $e = $all.Item($i)
            $n = [string]$e.Current.Name
            $ct = [string]$e.Current.ControlType.ProgrammaticName
            if ($ct -eq 'ControlType.Document') { $entry.hasDocument = $true }
            if ($n.Length -gt 0) {
              $entry.named = $entry.named + 1
              if ($n -ceq 'New Terminal') { $entry.exactNewTerminal = $entry.exactNewTerminal + 1 }
              if ($names.Count -lt 250) { $names.Add($ct + '|' + $n) | Out-Null }
            }
            if ($ct -eq 'ControlType.TabItem') { $tabs.Add($n) | Out-Null }
            if ($n -match '(?i)fail|error|retry|unable|cannot|denied|refus|not found|invalid') { $errs.Add($ct + '|' + $n) | Out-Null }
          } catch { }
        }
        $entry.names = @($names); $entry.tabItems = @($tabs); $entry.errorText = @($errs)
      }
    } catch { $entry.error = $_.Exception.Message }
    $out.Add($entry) | Out-Null
  }
  $res[$label] = @($out)
  Save
  return $out
}

$deadline = (Get-Date).AddSeconds($TimeoutSec)
$win = $null
while ((Get-Date) -lt $deadline) {
  $w = OwnedWindows $app.Id
  if (@($w).Count -gt 0) { $win = $w; break }
  Start-Sleep -Milliseconds 500
}
$res.ownedWindows = @($win)
Save
Start-Sleep -Seconds 14

# ---- 3. pre-click ----
$res.sessionsBefore = ListSessions (Join-Path $iso 'runtime')
DumpTree $app.Id 'treeBefore' | Out-Null

# ---- 4. invoke New Terminal on the webview window ----
$invoke = [ordered]@{ attempted=$false; attempts=0; seen=@() }
$searchSw = [System.Diagnostics.Stopwatch]::StartNew()
while ($searchSw.ElapsedMilliseconds -lt 30000) {
  $invoke.attempts = $invoke.attempts + 1
  $found = $false
  foreach ($w in (OwnedWindows $app.Id)) {
    try {
      $r = [System.Windows.Automation.AutomationElement]::FromHandle([System.IntPtr]::new([int64]$w.hwnd))
      if ($r -eq $null) { continue }
      # A cheap TrueCondition attach first: re-issuing it is what drives the lazy build.
      $null = $r.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
      $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, 'New Terminal')
      $items = $r.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)
      if ($items.Count -gt 0) {
        $el = $items.Item(0)
        $rect = $el.Current.BoundingRectangle
        $invoke = [ordered]@{ attempted=$true; attempts=$invoke.attempts; hwnd=$w.hwnd; count=$items.Count; enabled=[bool]$el.Current.IsEnabled; rect=("{0},{1},{2},{3}" -f $rect.Left,$rect.Top,$rect.Width,$rect.Height) }
        $p = $el.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
        $p.Invoke()
        $invoke.invoked = $true
        $found = $true
        break
      }
    } catch { $invoke.error = $_.Exception.Message }
  }
  if ($found) { break }
  Start-Sleep -Milliseconds 400
}
$invoke.elapsedMs = [int]$searchSw.ElapsedMilliseconds
$res.invoke = $invoke
$res.probeErrorBefore = $script:probeErr
Save

# ---- 5. post-click: sample inventory, then dump ----
$post = New-Object System.Collections.ArrayList
for ($i = 0; $i -lt 14; $i++) {
  Start-Sleep -Milliseconds 700
  $s = ListSessions (Join-Path $iso 'runtime')
  if ($null -ne $s) { $post.Add([ordered]@{ atMs = ($i*700); count=@($s).Count; sessions=@($s) }) | Out-Null }
}
$res.sessionsAfter = @($post)
DumpTree $app.Id 'treeAfter' | Out-Null

# ---- 6. logs ----
$res.appStdout = @(Get-Content $appOut -Tail 60 -ErrorAction SilentlyContinue)
$res.appStderr = @(Get-Content $appErr -Tail 60 -ErrorAction SilentlyContinue)
$dl = Join-Path $iso 'data\logs\daemon.log'
$res.daemonLogPath = $dl
$res.daemonLog = @(Get-Content $dl -Tail 120 -ErrorAction SilentlyContinue)
$res.daemonLogExists = (Test-Path $dl)
$res.receipts = @(Get-ChildItem (Join-Path $iso 'barriers') -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
Save

# ---- 7. teardown by exact recorded PID ----
foreach ($p in $procs) {
  $live = Get-CimInstance Win32_Process -Filter ("ProcessId=" + $p.pid) -ErrorAction SilentlyContinue
  if ($live) { taskkill /T /F /PID $p.pid 2>&1 | Out-Null }
}
$res.killedPids = @($procs)
Save
Write-Output ('CAPTURE12_DONE ' + $outPath)
