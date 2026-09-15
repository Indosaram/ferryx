$ErrorActionPreference='Stop'
$root='C:\Users\sook\ferryx-qa-fresh-0912'
$expected='"C:\Windows\System32\cmd.exe" /c C:\Users\sook\launch-fresh.cmd'
$protected=@(Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -in @(17288,1756,20196) } |
 Select-Object ProcessId,CreationDate,ExecutablePath)
if ($protected.Count -ne 3) { throw 'User process baseline changed; inspect before cleanup' }
$process=Get-CimInstance Win32_Process -Filter 'ProcessId=4824'
if ($process -and $process.CommandLine -ne $expected) { throw 'QA root PID reused' }
if ($process) {
 taskkill /T /F /PID 4824
 if ($LASTEXITCODE -ne 0) { throw 'QA tree cleanup failed' }
}
foreach($name in @('FerryxFresh0912','FerryxFreshSelect0912','FerryxFreshCapture0912','FerryxFreshResize0912','FerryxFreshOwner0912','FerryxFreshInspect0912')) {
 $task=Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
 if ($task) {
  if ($task.State -eq 'Running') { Stop-ScheduledTask -TaskName $name }
  Unregister-ScheduledTask -TaskName $name -Confirm:$false
  "REMOVED_TASK $name"
 }
}
$remaining=@(Get-CimInstance Win32_Process | Where-Object {
 $_.ExecutablePath -like "$root\*" -or ($_.Name -eq 'msedgewebview2.exe' -and $_.CommandLine.Contains("$root\webview2"))
})
if ($remaining.Count) { $remaining | Select-Object ProcessId,Name; throw 'Owned processes remain' }
'FRESH_OWNED_PROCESSES_ZERO'
foreach($before in $protected) {
 $after=Get-CimInstance Win32_Process -Filter "ProcessId=$($before.ProcessId)"
 if (-not $after -or $after.CreationDate -ne $before.CreationDate -or $after.ExecutablePath -ne $before.ExecutablePath) { throw 'Protected user process changed' }
}
$protected | ConvertTo-Json -Compress
$junction=Join-Path $root 'orca-lite\src-tauri\vendor\ghostty'
if(Test-Path $junction) {
 $item=Get-Item $junction -Force
 if(-not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Expected QA Ghostty junction' }
 cmd /c rmdir $junction
 if($LASTEXITCODE -ne 0) { throw 'Junction cleanup failed' }
}
Remove-Item -LiteralPath $root -Recurse -Force
foreach($file in @('C:\Users\sook\ferryx-fresh-0912.bundle','C:\Users\sook\setup-fresh.ps1','C:\Users\sook\launch-fresh.cmd')) {
 if(Test-Path $file) { Remove-Item -LiteralPath $file -Force }
}
'FRESH_QA_ROOT_REMOVED'
