$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-03e0d6b'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win\03e0d6b"
Get-CimInstance Win32_Process | Where-Object Name -match 'ferryx_lib|cargo|rustc' | Select-Object ProcessId,ParentProcessId,ExecutablePath,CommandLine | Format-List | Out-File "$logs\stall-processes.log"
Get-Content "$logs\repaired.log" -Tail 80 | Out-File "$logs\stall-current-test.log"
$runner=Get-CimInstance Win32_Process | Where-Object { $_.Name -like 'ferryx_lib*' -and $_.ProcessId -eq 17444 }
foreach($p in $runner){
 "ATTACH_RECORDED_RUNNER=$($p.ProcessId)" | Add-Content "$logs\stall-stack.log"
 & 'C:\Program Files (x86)\Windows Kits\10\Debuggers\x64\cdb.exe' -pv -p $p.ProcessId -c '~* kb;qd' *> "$logs\stall-stack-$($p.ProcessId).log"
 "EXIT=$LASTEXITCODE" | Add-Content "$logs\stall-stack-$($p.ProcessId).log"
}
