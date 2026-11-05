$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-bd4d3459'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win\bd4d3459"
Get-CimInstance Win32_Process | Where-Object Name -match 'ferryx_lib|cargo|rustc' | Select-Object ProcessId,ParentProcessId,ExecutablePath,CommandLine | Format-List | Out-File "$logs\stall-processes.log"
Get-Content "$logs\full-lib.log" -Tail 80 | Out-File "$logs\stall-current-test.log"
$runner=Get-CimInstance Win32_Process | Where-Object { $_.Name -like 'ferryx_lib*' -and $_.CommandLine -match 'test-threads=1' }
foreach($p in $runner){
 "ATTACH_RECORDED_RUNNER=$($p.ProcessId)" | Add-Content "$logs\stall-stack.log"
 & 'C:\Program Files (x86)\Windows Kits\10\Debuggers\x64\cdb.exe' -pv -p $p.ProcessId -c '~* kb;qd' *> "$logs\stall-stack-$($p.ProcessId).log"
 "EXIT=$LASTEXITCODE" | Add-Content "$logs\stall-stack-$($p.ProcessId).log"
}
