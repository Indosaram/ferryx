$root = 'C:\Users\sook\ferryx-herdr-q4-windows-01a097f8'
$ownedId = [int]((Get-Content "$root\monitor.log") -replace 'BUILD_PID=','')
$p = Get-Process -Id $ownedId -ErrorAction SilentlyContinue
if ($p) {
  if (-not $p.WaitForExit(240000)) { Write-Output 'BUILD_STILL_RUNNING' }
}
Get-Content "$root\build.log" -Tail 50
Get-Content "$root\build-exit.log" -ErrorAction SilentlyContinue
