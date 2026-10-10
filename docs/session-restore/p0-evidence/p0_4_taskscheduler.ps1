$ErrorActionPreference = 'Stop'
$userSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$userName = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
$root = Join-Path $env:USERPROFILE 'ferryx-p0\svc'
New-Item -ItemType Directory -Force -Path $root | Out-Null
$hostScript = @'
param([string]$Id, [string]$Root)
$dir = Join-Path $Root $Id
$me = Get-CimInstance Win32_Process -Filter "ProcessId=$PID"
$parent = Get-CimInstance Win32_Process -Filter "ProcessId=$($me.ParentProcessId)" -ErrorAction SilentlyContinue
"$PID $($me.CreationDate.ToFileTimeUtc()) $($me.ParentProcessId) $($parent.Name)" | Set-Content (Join-Path $dir 'pid')
while ($true) {
  if (Test-Path (Join-Path $dir 'retire')) {
    Add-Content (Join-Path $dir 'events') 'retired-exit'
    Unregister-ScheduledTask -TaskName "Ferryx-P0-Host-$Id" -TaskPath '\FerryxP0\' -Confirm:$false
    exit 0
  }
  Start-Sleep -Milliseconds 200
}
'@
$ids = 'a1','b2'
foreach ($id in $ids) {
  $d = Join-Path $root $id
  New-Item -ItemType Directory -Force -Path $d | Out-Null
  Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $d 'pid'),(Join-Path $d 'events'),(Join-Path $d 'retire')
  Set-Content -Path (Join-Path $d 'ferryx-p0-host.ps1') -Value $hostScript
}
function Register-Instance($id) {
  $script = Join-Path (Join-Path $root $id) 'ferryx-p0-host.ps1'
  $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$script`" -Id $id -Root `"$root`""
  $trigger = New-ScheduledTaskTrigger -AtLogOn -User $userName
  $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
  $principal = New-ScheduledTaskPrincipal -UserId $userSid -LogonType Interactive -RunLevel Limited
  Register-ScheduledTask -TaskName "Ferryx-P0-Host-$id" -TaskPath '\FerryxP0\' -Action $action -Trigger $trigger -Settings $settings -Principal $principal -Force | Out-Null
  Start-ScheduledTask -TaskName "Ferryx-P0-Host-$id" -TaskPath '\FerryxP0\'
}
function Read-Pid($id) {
  $f = Join-Path (Join-Path $root $id) 'pid'
  for ($i = 0; $i -lt 50 -and -not (Test-Path $f); $i++) { Start-Sleep -Milliseconds 200 }
  (Get-Content $f).Split(' ')
}
try {
  "STEP start a1"; Register-Instance 'a1'; $a1 = Read-Pid 'a1'
  "a1 pid=$($a1[0]) parent=$($a1[3])"
  "STEP start b2 while a1 runs"; Register-Instance 'b2'; $b2 = Read-Pid 'b2'
  "b2 pid=$($b2[0]) parent=$($b2[3])"
  "STEP restart b2 only"
  Stop-ScheduledTask -TaskName 'Ferryx-P0-Host-b2' -TaskPath '\FerryxP0\'
  Remove-Item -Force (Join-Path (Join-Path $root 'b2') 'pid')
  Start-ScheduledTask -TaskName 'Ferryx-P0-Host-b2' -TaskPath '\FerryxP0\'
  $b2n = Read-Pid 'b2'
  $a1alive = [bool](Get-Process -Id ([int]$a1[0]) -ErrorAction SilentlyContinue)
  "a1_unchanged_after_b2_restart=$a1alive b2_new_pid=$($b2n[0])"
  "STEP launcher process exits (this ssh session's child) - check a1 not in its job"
  $p = Start-Process -FilePath powershell.exe -ArgumentList '-NoProfile','-Command','exit 0' -PassThru -WindowStyle Hidden; $p.WaitForExit()
  "a1_alive_after_unrelated_parent_exit=$([bool](Get-Process -Id ([int]$a1[0]) -ErrorAction SilentlyContinue))"
  "STEP retire a1 (self-exit + self-unregister)"
  New-Item -ItemType File -Force -Path (Join-Path (Join-Path $root 'a1') 'retire') | Out-Null
  Start-Sleep -Seconds 3
  $t = Get-ScheduledTask -TaskName 'Ferryx-P0-Host-a1' -TaskPath '\FerryxP0\' -ErrorAction SilentlyContinue
  "a1_alive=$([bool](Get-Process -Id ([int]$a1[0]) -ErrorAction SilentlyContinue)) a1_task_registered=$([bool]$t) a1_events=$(Get-Content (Join-Path (Join-Path $root 'a1') 'events') -ErrorAction SilentlyContinue)"
  "b2_alive=$([bool](Get-Process -Id ([int]$b2n[0]) -ErrorAction SilentlyContinue))"
} finally {
  "STEP cleanup"
  foreach ($id in $ids) {
    $f = Join-Path (Join-Path $root $id) 'pid'
    if (Test-Path $f) { $p = [int](Get-Content $f).Split(' ')[0]; $proc = Get-CimInstance Win32_Process -Filter "ProcessId=$p" -ErrorAction SilentlyContinue; if ($proc -and $proc.CommandLine -like "*ferryx-p0\svc\$id\ferryx-p0-host.ps1*") { Stop-Process -Id $p -Force } }
    Unregister-ScheduledTask -TaskName "Ferryx-P0-Host-$id" -TaskPath '\FerryxP0\' -Confirm:$false -ErrorAction SilentlyContinue
  }
  $left = Get-ScheduledTask -TaskPath '\FerryxP0\' -ErrorAction SilentlyContinue
  "cleanup tasks_left=$(@($left).Count)"
}
