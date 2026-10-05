$ErrorActionPreference = 'Stop'
Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -and ($_.CommandLine.Contains('source-5464da0d') -or $_.CommandLine.Contains('task8-5464da0d\runner.mjs')) } | Select-Object ProcessId,ParentProcessId,Name,CommandLine | Format-List
Get-Item 'C:/Users/sook/ferryx-pane-completion/task8-5464da0d/logs/full-ui.log' | Select-Object Length,LastWriteTime
Get-Content 'C:/Users/sook/ferryx-pane-completion/task8-5464da0d/logs/full-ui.log' -Tail 15
