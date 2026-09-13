$ErrorActionPreference = 'Stop'
$root = 'C:\Users\sook\ferryx-herdr-q4-windows-01a097f8'
Start-Transcript -Path "$root\cleanup.log"
$manifest = Get-Content "$root\manifest.json" -Raw | ConvertFrom-Json
$count=0
foreach ($p in $manifest.PSObject.Properties) {
  if ((Get-FileHash (Join-Path "$root\source" $p.Name)).Hash.ToLower() -ne $p.Value.sha256) { throw "Changed source $($p.Name)" }
  $count++
}
"POST_BUILD_SOURCE_HASHES=$count"
$owned = @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($root,[StringComparison]::OrdinalIgnoreCase) })
"OWNED_EXECUTABLE_PROCESSES=$($owned.Count)"
if ($owned.Count) { $owned | Select-Object ProcessId,ExecutablePath; throw 'owned executable remains' }
Get-Content "$root\build-exit.log"
foreach ($name in @('runtime','data','sessions','home','config','cache','temp','appdata','localappdata')) {
  $p=Join-Path $root $name
  Remove-Item -LiteralPath $p -Recurse -Force
  "REMOVED=$p; ABSENT=$(-not (Test-Path -LiteralPath $p))"
}
'No test binaries, daemons, PTYs, HTTP/WS listeners or Git runtime fixtures were launched.'
'Retained: source, target, archive, bundle, manifests, scripts and command logs for coordinated repair review.'
Stop-Transcript
