$ErrorActionPreference='Stop'
$root='C:\Users\sook\ferryx-herdr-q4-windows-01a097f8'
Start-Transcript -Path "$root\repaired-cleanup-final.log"
$manifest=Get-Content "$root\repaired-manifest.json" -Raw | ConvertFrom-Json
$n=0
foreach($p in $manifest.PSObject.Properties) {
 if((Get-FileHash (Join-Path "$root\source" $p.Name)).Hash.ToLower() -ne $p.Value.sha256) {throw "Changed input $($p.Name)"}; $n++
}
"REPAIRED_SOURCE_MATCH=$n"
$owned=@(Get-CimInstance Win32_Process | Where-Object { ($_.ExecutablePath -and $_.ExecutablePath.StartsWith($root,[StringComparison]::OrdinalIgnoreCase)) -or ($_.CommandLine -and $_.CommandLine.Contains($root) -and $_.Name -notin @('powershell.exe','sshd.exe','cmd.exe')) })
$owned | Select-Object ProcessId,ParentProcessId,Name,ExecutablePath,CommandLine
$owned=@($owned | Where-Object { -not ($_.Name -eq 'pwsh.exe' -and $_.CommandLine.Contains('\repaired-cleanup.ps1')) })
"OWNED_RUNTIME_PROCESSES=$($owned.Count)"
if($owned.Count) {throw 'Owned process remains; inspect before cleanup'}
Get-Content "$root\repaired-exits.log"
foreach($name in @('runtime','data','sessions','home','config','cache','temp','appdata','localappdata')) {
 $p=Join-Path $root $name
 Remove-Item -LiteralPath $p -Recurse -Force
 "REMOVED=$p; ABSENT=$(-not(Test-Path -LiteralPath $p))"
}
'Retained source/target/archive/bundle and tooling. No desktop or real clipboard action.'
Stop-Transcript
