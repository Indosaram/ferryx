$ErrorActionPreference='Continue'
Get-CimInstance Win32_Process | Where-Object { $_.Name -match 'maho-host|ferryx|explorer' } | Select-Object ProcessId,ParentProcessId,SessionId,Name,ExecutablePath,CommandLine | Format-List
quser
Get-NetTCPConnection -LocalPort 19730 -ErrorAction SilentlyContinue | Select-Object LocalAddress,State,OwningProcess
Get-ChildItem 'C:\Users\sook\tools' -Filter '*mahord*' -Directory
