$root='C:\Users\sook\ferryx-herdr-q4-windows-01a097f8'
$owner=@(Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -like '*\repaired-suites.ps1*' })
foreach ($o in $owner) {
 $p=Get-Process -Id $o.ProcessId -ErrorAction SilentlyContinue
 if ($p -and -not $p.WaitForExit(600000)) { 'MONITOR_STILL_RUNNING' }
}
Get-Content "$root\repaired-exits.log"
foreach ($name in @('remote','integration','worktree')) { Get-Content "$root\repaired-$name.log" -Tail 75 -ErrorAction SilentlyContinue }
