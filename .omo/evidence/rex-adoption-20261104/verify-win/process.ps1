Get-CimInstance Win32_Process | Where-Object { $_.Name -match 'cargo|rustc|link|ferryx_lib|build-script' } | Select-Object ProcessId,ParentProcessId,Name,CommandLine | Format-List
Get-Content C:\Users\sook\ferryx-wt\rex-verify-win-20261104\.omo\evidence\rex-adoption-20261104\verify-win\lib-test-corrected.log -Tail 40
