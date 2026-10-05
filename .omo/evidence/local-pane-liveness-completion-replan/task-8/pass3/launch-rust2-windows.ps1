$ErrorActionPreference = 'Continue'
$root = 'C:\Users\sook\ferryx-pane-completion\source-21dea3c0'
$out  = 'C:\Users\sook\ferryx-pane-completion\task8-21dea3c0'
$env:CARGO_TARGET_DIR = "$root\target"
Set-Location $root
node "$out\runner3-rust2.mjs" $root $out windows 2>&1 | Tee-Object -FilePath "$out\rust2.log" -Append
$native = $LASTEXITCODE
Write-Output "RUST2_LAUNCH_DONE native=$native"
exit $native
