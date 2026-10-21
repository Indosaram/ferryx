# Windows GUI launcher for the daemon-lifetime QA harness (Q03, Q06w). Started by MahoRD (Win+R) so
# the GUI runs on the interactive desktop. It creates a job object (with -NoBreakaway, one whose
# only limit is KILL_ON_JOB_CLOSE, so the job forbids breakaway: Q06w's detach guard), starts the
# QA GUI with the run's env and its stderr/stdout copied to logs\gui-<tag>.log, assigns it to the
# job, publishes gui.json, then waits for the harness to ask for TerminateJobObject (Q03) through
# terminate.request in the run dir. It never touches any process other than the one it started.
# Receipts for lib/gui.mjs launchWindows: its first statement writes logs\launcher-<tag>.started,
# and any terminating error writes logs\launcher-<tag>.err (the window is hidden, so nothing else
# would show it). teardown copies both into the evidence.
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [Parameter(Mandatory = $true)][string]$RunDir,
  [Parameter(Mandatory = $true)][string]$EnvFile,
  [Parameter(Mandatory = $true)][string]$Tag,
  [switch]$NoBreakaway
)
$ErrorActionPreference = 'Stop'
$receiptDir = Join-Path $RunDir 'logs'
[System.IO.Directory]::CreateDirectory($receiptDir) | Out-Null
[System.IO.File]::WriteAllText((Join-Path $receiptDir "launcher-$Tag.started"), "pid=$PID`r`n")
try {
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class QaJob {
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr CreateJobObject(IntPtr a, string n);
  [DllImport("kernel32.dll")] public static extern bool AssignProcessToJobObject(IntPtr j, IntPtr p);
  [DllImport("kernel32.dll")] public static extern bool TerminateJobObject(IntPtr j, uint c);
  [DllImport("kernel32.dll")] public static extern IntPtr OpenProcess(uint a, bool i, int pid);
  [DllImport("kernel32.dll")] public static extern bool SetInformationJobObject(IntPtr j, int c, IntPtr info, uint len);
  // JOBOBJECT_EXTENDED_LIMIT_INFORMATION (class 9) is 144 bytes on x64; LimitFlags is the u32 at
  // byte 16. KILL_ON_JOB_CLOSE (0x2000) alone sets neither BREAKAWAY_OK nor SILENT_BREAKAWAY_OK.
  public static bool ForbidBreakaway(IntPtr j) {
    IntPtr info = Marshal.AllocHGlobal(144);
    try {
      for (int i = 0; i < 144; i++) Marshal.WriteByte(info, i, 0);
      Marshal.WriteInt32(info, 16, 0x2000);
      return SetInformationJobObject(j, 9, info, 144);
    } finally { Marshal.FreeHGlobal(info); }
  }
  public static void Pipe(System.IO.StreamReader reader, string path) {
    var t = new System.Threading.Thread(() => {
      string line;
      while ((line = reader.ReadLine()) != null) { lock (typeof(QaJob)) { System.IO.File.AppendAllText(path, line + Environment.NewLine); } }
    });
    t.IsBackground = true;
    t.Start();
  }
}
"@
$vars = Get-Content -Raw -Path $EnvFile | ConvertFrom-Json
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $Exe
$psi.UseShellExecute = $false
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
foreach ($p in $vars.PSObject.Properties) { $psi.EnvironmentVariables[$p.Name] = [string]$p.Value }
$job = [QaJob]::CreateJobObject([IntPtr]::Zero, "ferryx-qa-gui-$Tag")
$noBreakaway = $false
if ($NoBreakaway) { $noBreakaway = [QaJob]::ForbidBreakaway($job) }
$proc = [System.Diagnostics.Process]::Start($psi)
$handle = [QaJob]::OpenProcess(0x1F0FFF, $false, $proc.Id)
$assigned = [QaJob]::AssignProcessToJobObject($job, $handle)
$logs = Join-Path $RunDir 'logs'
New-Item -ItemType Directory -Force -Path $logs | Out-Null
$log = Join-Path $logs "gui-$Tag.log"
[QaJob]::Pipe($proc.StandardError, $log)
[QaJob]::Pipe($proc.StandardOutput, $log)
$out = Join-Path $RunDir "gui-$Tag.json"
[ordered]@{ pid = $proc.Id; path = $Exe; start = $proc.StartTime.ToFileTimeUtc(); jobAssigned = $assigned; noBreakaway = $noBreakaway; launcherPid = $PID } | ConvertTo-Json -Compress | Set-Content -Path "$out.tmp"
Move-Item -Force "$out.tmp" $out
$request = Join-Path $RunDir "gui-$Tag.terminate.request"
$watcher = New-Object System.IO.FileSystemWatcher $RunDir, "gui-$Tag.terminate.request"
$watcher.EnableRaisingEvents = $true
while (-not $proc.HasExited) {
  if (Test-Path $request) {
    $ok = [QaJob]::TerminateJobObject($job, 1)
    Set-Content -Path (Join-Path $RunDir "gui-$Tag.terminated.json") -Value (@{ terminateJobObject = $ok } | ConvertTo-Json -Compress)
    break
  }
  $null = $watcher.WaitForChanged([System.IO.WatcherChangeTypes]::Created, 60000)
}
} catch {
  [System.IO.File]::WriteAllText((Join-Path $receiptDir "launcher-$Tag.err"), ($_ | Out-String) + $_.ScriptStackTrace)
  exit 1
}
