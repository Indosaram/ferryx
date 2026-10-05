$ErrorActionPreference = 'Stop'
$root = 'C:\Users\sook\ferryx-pane-completion\source-21dea3c0'
$out  = 'C:\Users\sook\ferryx-pane-completion\task8-21dea3c0'
$before = (Get-FileHash "$root\src-tauri\src\native_terminal\surface_host.rs" -Algorithm SHA256).Hash
Write-Output "BEFORE_SHA=$before"
if ($before -ne '6E4928CE757AEEF8DA626EC33A20573D28976B1BE430FA180E7BFC8A76E31CC9') { Write-Output 'UNEXPECTED_PRE_SHA'; exit 2 }
Set-Location $root
git apply --verbose "$out\surface_host-repair.patch"
$native = $LASTEXITCODE
if ($native -ne 0) { exit $native }
$after = (Get-FileHash "$root\src-tauri\src\native_terminal\surface_host.rs" -Algorithm SHA256).Hash
Write-Output "AFTER_SHA=$after"
if ($after -ne '9039B136990166A7EDB3D264A0249848D69EE888379E6107C216A6CDB2FE696A') { Write-Output 'PATCH_SHA_MISMATCH'; exit 3 }
Write-Output "DELTA_APPLIED_OK host=windows"
