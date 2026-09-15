$ErrorActionPreference = 'Stop'
$t = Get-ScheduledTask -TaskName FerryxShot_st01a0958a
foreach ($pair in @(
  @{ Task='FerryxExpand_st01a0958a'; Script='expand-menu.ps1' },
  @{ Task='FerryxSelect_st01a0958a'; Script='select-cmd.ps1' }
)) {
  $a = New-ScheduledTaskAction -Execute 'C:\Program Files\PowerShell\7\pwsh.exe' -Argument "-NoProfile -File C:\Users\sook\ferryx-qa-rt-st01a0958a\qa\$($pair.Script)"
  Register-ScheduledTask -TaskName $pair.Task -Action $a -Principal $t.Principal -Settings $t.Settings -Force | Out-Null
}
Start-ScheduledTask -TaskName FerryxExpand_st01a0958a
