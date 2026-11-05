$ErrorActionPreference='Stop'
Get-ChildItem C:\Users\sook\ferryx-wt\rex-buildgreen-logs
Get-ChildItem C:\Users\sook\ferryx-wt\.tmp-out
Get-CimInstance Win32_Process | Where-Object { $_.Name -match 'ferryx|cargo|rustc' } | Select-Object ProcessId,Name,ExecutablePath,CommandLine | Format-List
Get-ChildItem \\.\pipe\ | Where-Object Name -match 'ferryx'
git -C C:\Users\sook\ferryx-wt\rex-verify-win-20261104 submodule status
