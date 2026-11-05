$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-20261104'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win"
Get-Process -Id 26164 -ErrorAction SilentlyContinue | Stop-Process
'Focused baseline runner PID=26164 stopped: route test lacks progress beyond bounded wait. EXIT=TIMEOUT' | Add-Content "$logs\baseline-timeout.log"
# QA daemon was attached to its SSH job and exited on SSH teardown. Check only recorded QA pid.
$pidFile="$logs\qa-daemon.pid"
if(Test-Path $pidFile){$qaPid=[int](Get-Content $pidFile);$p=Get-CimInstance Win32_Process -Filter "ProcessId=$qaPid";if($p -and $p.ExecutablePath -like "$root\.qa\*"){Stop-Process -Id $qaPid;"QA_PID=$qaPid stopped"}else{"QA_PID=$qaPid already gone"}}
Get-ChildItem \\.\pipe\ | Where-Object Name -match 'ferryx-.*-(daemon|control|agent-state)-dev' | Select-Object Name
Get-CimInstance Win32_Process | Where-Object Name -match '^ferryx' | Select-Object ProcessId,ExecutablePath,CommandLine | Format-List
Set-Location $root
git status --short
