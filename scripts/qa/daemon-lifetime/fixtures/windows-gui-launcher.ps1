# Windows GUI launcher for the daemon-lifetime QA harness (Q03). Started by MahoRD (Win+R) so the
# GUI runs on the interactive desktop. It creates a job object, starts the QA GUI suspended-free
# with the run's env, assigns it to the job, publishes gui.json, then waits for the harness to ask
# for TerminateJobObject (Q03) through terminate.request in the run dir. It never touches any
# process other than the one it started.
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [Parameter(Mandatory = $true)][string]$RunDir,
  [Parameter(Mandatory = $true)][string]$EnvFile,
  [Parameter(Mandatory = $true)][string]$Tag
)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class QaJob {
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr CreateJobObject(IntPtr a, string n);
  [DllImport("kernel32.dll")] public static extern bool AssignProcessToJobObject(IntPtr j, IntPtr p);
  [DllImport("kernel32.dll")] public static extern bool TerminateJobObject(IntPtr j, uint c);
  [DllImport("kernel32.dll")] public static extern IntPtr OpenProcess(uint a, bool i, int pid);
}
"@
$vars = Get-Content -Raw -Path $EnvFile | ConvertFrom-Json
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $Exe
$psi.UseShellExecute = $false
foreach ($p in $vars.PSObject.Properties) { $psi.EnvironmentVariables[$p.Name] = [string]$p.Value }
$job = [QaJob]::CreateJobObject([IntPtr]::Zero, "ferryx-qa-gui-$Tag")
$proc = [System.Diagnostics.Process]::Start($psi)
$handle = [QaJob]::OpenProcess(0x1F0FFF, $false, $proc.Id)
$assigned = [QaJob]::AssignProcessToJobObject($job, $handle)
$out = Join-Path $RunDir "gui-$Tag.json"
[ordered]@{ pid = $proc.Id; path = $Exe; start = $proc.StartTime.ToFileTimeUtc(); jobAssigned = $assigned; launcherPid = $PID } | ConvertTo-Json -Compress | Set-Content -Path "$out.tmp"
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
