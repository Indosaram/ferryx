$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-20261104'
Set-Location $root
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win"
$env:CARGO_TARGET_DIR='C:\Users\sook\ferryx-wt\.target'
$env:CMAKE_POLICY_VERSION_MINIMUM='3.5'
$env:LIBCLANG_PATH='C:\Users\sook\ferryx-wt\.llvm\clang+llvm-23.1.4-x86_64-pc-windows-msvc\bin'
$env:FFMPEG_DIR="$root\src-tauri\resources\ffmpeg\sdk"
$env:PATH="$env:FFMPEG_DIR\bin;$env:PATH"
function Run($name,[scriptblock]$action) {
 "BEGIN $name $(Get-Date -Format o)" | Add-Content "$logs\exits-corrected.log"
 & $action *> "$logs\$name.log"
 "EXIT=$LASTEXITCODE" | Add-Content "$logs\$name.log"
 "$name EXIT=$LASTEXITCODE $(Get-Date -Format o)" | Add-Content "$logs\exits-corrected.log"
}
Run lib-test-corrected { cargo test --manifest-path src-tauri/Cargo.toml --lib --locked }
Run headless-corrected { cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --locked --bin ferryx-cli --bin ferryx-relay --bin ferryx-account }
Run debug-corrected { cargo build --manifest-path src-tauri/Cargo.toml --locked --bin ferryx --bin ferryx-cli }
Run msix-test { powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-build-msix.ps1 }
'DONE' | Add-Content "$logs\exits-corrected.log"
