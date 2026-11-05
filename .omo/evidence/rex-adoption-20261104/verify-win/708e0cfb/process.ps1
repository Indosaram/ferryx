Get-CimInstance Win32_Process | Where-Object { $_.Name -match 'cargo|rustc|ferryx_lib|link.exe' } | Select-Object ProcessId,ParentProcessId,Name,CommandLine | Format-List
