$ErrorActionPreference = 'Stop'
$root = 'C:\Users\sook\ferryx-pane-completion\source-5464da0d'
$g = "$root\src-tauri\vendor\ghostty"
if ((Get-ChildItem $g -Force | Measure-Object).Count -ne 0) { throw 'Expected empty archive submodule directory' }
[System.IO.Directory]::Delete($g)
cmd.exe /d /c "mklink /J $g C:\Users\sook\task2-ghostty-6a508fd5"
$native = $LASTEXITCODE
if ($native -ne 0) { exit $native }
Set-Location $root
bun install --cwd ui --frozen-lockfile
$native = $LASTEXITCODE
Write-Output "STAGING_DONE windows native=$native"
exit $native
