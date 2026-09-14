param([string]$Script='candidate-focused',[string]$Prefix='candidate-focused')
$root='C:\Users\sook\ferryx-herdr-q4-windows-01a097f8'
$owners=@(Get-CimInstance Win32_Process | Where-Object {$_.Name -eq 'powershell.exe' -and $_.CommandLine.Contains("\$Script.ps1")})
foreach($owner in $owners) {$p=Get-Process -Id $owner.ProcessId -ErrorAction SilentlyContinue; if($p -and -not $p.WaitForExit(600000)) {'STILL_RUNNING'}}
Get-Content "$root\$Prefix-exits.log"
Get-ChildItem "$root\$Prefix-*.log" | Where-Object {$_.Name -notmatch 'command|monitor|isolation|exits'} | ForEach-Object { $_.Name; Get-Content $_.FullName -Tail 22 }
