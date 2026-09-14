$root='C:\Users\sook\ferryx-herdr-q4-windows-01a097f8'
$owner=@(Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'powershell.exe' -and $_.CommandLine -like '*\repaired-run.ps1*' })
foreach ($o in $owner) {
 $p=Get-Process -Id $o.ProcessId -ErrorAction SilentlyContinue
 if ($p -and -not $p.WaitForExit(480000)) { 'MONITOR_STILL_RUNNING' }
}
Get-Content "$root\repaired-exits.log" -ErrorAction SilentlyContinue
Get-Content "$root\repaired-build.log" -Tail 15 -ErrorAction SilentlyContinue
Get-Content "$root\repaired-clipboard.log" -Tail 70 -ErrorAction SilentlyContinue
