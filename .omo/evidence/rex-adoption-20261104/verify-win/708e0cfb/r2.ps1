$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-d8313422b'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win\708e0cfb"
New-Item -ItemType Directory -Force $logs | Out-Null
Set-Location $root
$env:CARGO_TARGET_DIR='C:\Users\sook\ferryx-wt\.target'
$env:CARGO_INCREMENTAL='0'
$env:CMAKE_POLICY_VERSION_MINIMUM='3.5'
$env:LIBCLANG_PATH='C:\Users\sook\ferryx-wt\.llvm\clang+llvm-23.1.4-x86_64-pc-windows-msvc\bin'
$env:FFMPEG_DIR="$root\src-tauri\resources\ffmpeg\sdk"
$env:PATH="$env:FFMPEG_DIR\bin;$env:PATH"
git rev-parse HEAD | Out-File "$logs\revision.log"
cargo test --manifest-path src-tauri/Cargo.toml --locked --lib -- daemon::plugins *> "$logs\plugin-tests.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\plugin-tests.log"
cargo check --manifest-path src-tauri/Cargo.toml --locked --all-targets *> "$logs\compile.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\compile.log"
'DONE' | Out-File "$logs\done.log"
