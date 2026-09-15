$ErrorActionPreference = 'Stop'
foreach ($name in @('FerryxSelect_st01a0958a','FerryxExpand_st01a0958a','FerryxMenu_st01a0958a','FerryxShot_st01a0958a')) {
  $task = Get-ScheduledTask -TaskName $name
  $old = $task.Actions[0]
  $a = New-ScheduledTaskAction -Execute $old.Execute -Argument ("-WindowStyle Hidden " + $old.Arguments)
  Set-ScheduledTask -TaskName $name -Action $a | Out-Null
}
