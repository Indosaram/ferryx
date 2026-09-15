$ErrorActionPreference='Stop'
$t=Get-ScheduledTask -TaskName FerryxFresh0912
foreach($pair in @(
  @{Name='FerryxFreshSelect0912';Script='select-fresh.ps1'},
  @{Name='FerryxFreshCapture0912';Script='capture-fresh.ps1'}
)){
  $a=New-ScheduledTaskAction -Execute 'C:\Program Files\PowerShell\7\pwsh.exe' -Argument "-WindowStyle Hidden -NoProfile -File C:\Users\sook\ferryx-qa-fresh-0912\$($pair.Script)"
  Register-ScheduledTask -TaskName $pair.Name -Action $a -Principal $t.Principal -Settings $t.Settings | Out-Null
}
