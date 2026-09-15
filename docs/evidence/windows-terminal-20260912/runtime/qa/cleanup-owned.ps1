$ErrorActionPreference = 'Stop'
$root = 'C:\Users\sook\ferryx-qa-rt-st01a0958a'
$expected = '"C:\Windows\System32\cmd.exe" /c "C:\Users\sook\ferryx-qa-rt-st01a0958a\qa\launch-isolated.cmd"'
$process = Get-CimInstance Win32_Process -Filter 'ProcessId=3972'
if ($process -and $process.CommandLine -ne $expected) { throw 'QA root PID was reused; cleanup refused' }
if ($process) {
  taskkill /T /F /PID 3972
  if ($LASTEXITCODE -ne 0) { throw 'QA process tree cleanup failed' }
}
foreach ($name in @(
  'FerryxQA_st01a0958a','FerryxCdp_st01a0958a','FerryxExpand_st01a0958a',
  'FerryxKey_st01a0958a','FerryxMenu_st01a0958a','FerryxPosition_st01a0958a',
  'FerryxSelect_st01a0958a','FerryxShot_st01a0958a'
)) {
  $task = Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
  if ($task) {
    if ($task.State -eq 'Running') { Stop-ScheduledTask -TaskName $name }
    Unregister-ScheduledTask -TaskName $name -Confirm:$false
    Write-Output "REMOVED_TASK $name"
  }
}
$remaining = @(Get-CimInstance Win32_Process | Where-Object {
  $_.ExecutablePath -like "$root\*" -or
  ($_.Name -eq 'msedgewebview2.exe' -and $_.CommandLine.Contains("$root\webview2"))
})
if ($remaining.Count) { $remaining | Select-Object ProcessId,Name; throw 'Owned processes remain' }
Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -in @(17288,1756,20196) } |
  Select-Object ProcessId,ExecutablePath,CreationDate | ConvertTo-Json -Compress
Write-Output 'OWNED_PROCESSES_ZERO'
# Remove only the QA junction itself, not its shared Ghostty target.
$junction = Join-Path $root 'orca-lite\src-tauri\vendor\ghostty'
if (Test-Path $junction) {
  $item = Get-Item $junction -Force
  if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
    cmd /c rmdir $junction
    if ($LASTEXITCODE -ne 0) { throw 'Could not remove QA junction' }
  }
}
Remove-Item -LiteralPath $root -Recurse -Force
Write-Output 'QA_ROOT_REMOVED'
