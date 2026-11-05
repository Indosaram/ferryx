$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-20261104'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win"
Set-Location $root
$env:CARGO_TARGET_DIR='C:\Users\sook\ferryx-wt\.target'
$env:CMAKE_POLICY_VERSION_MINIMUM='3.5'
$env:LIBCLANG_PATH='C:\Users\sook\ferryx-wt\.llvm\clang+llvm-23.1.4-x86_64-pc-windows-msvc\bin'
$env:FFMPEG_DIR="$root\src-tauri\resources\ffmpeg\sdk"
$env:PATH="$env:FFMPEG_DIR\bin;$env:PATH"
# Disable incremental reuse after concurrent baseline/candidate produced unresolved monomorph symbols.
$env:CARGO_INCREMENTAL='0'
cargo build --manifest-path src-tauri/Cargo.toml --locked --bin ferryx --bin ferryx-cli *> "$logs\debug-nonincremental.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\debug-nonincremental.log"
