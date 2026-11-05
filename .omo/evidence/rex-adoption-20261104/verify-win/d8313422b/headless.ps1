$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-d8313422b'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win\d8313422b"
Set-Location $root
$env:CARGO_TARGET_DIR='C:\Users\sook\ferryx-wt\.target'
$env:CMAKE_POLICY_VERSION_MINIMUM='3.5'
$env:LIBCLANG_PATH='C:\Users\sook\ferryx-wt\.llvm\clang+llvm-23.1.4-x86_64-pc-windows-msvc\bin'
$env:FFMPEG_DIR="$root\src-tauri\resources\ffmpeg\sdk"
$env:CARGO_INCREMENTAL='0'
cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --locked --bin ferryx-cli --bin ferryx-relay --bin ferryx-account *> "$logs\headless.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\headless.log"
