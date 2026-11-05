$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-baseline-20261104'
$logs='C:\Users\sook\ferryx-wt\rex-verify-win-20261104\.omo\evidence\rex-adoption-20261104\verify-win'
Set-Location $root
$env:CARGO_TARGET_DIR='C:\Users\sook\ferryx-wt\.target'
$env:CMAKE_POLICY_VERSION_MINIMUM='3.5'
$env:LIBCLANG_PATH='C:\Users\sook\ferryx-wt\.llvm\clang+llvm-23.1.4-x86_64-pc-windows-msvc\bin'
$env:FFMPEG_DIR='C:\Users\sook\ferryx-wt\rex-verify-win-20261104\src-tauri\resources\ffmpeg\sdk'
$env:PATH="$env:FFMPEG_DIR\bin;$env:PATH"
git -c protocol.file.allow=always submodule update --init src-tauri/vendor/ghostty *> "$logs\baseline-setup.log"
# Use candidate-built static UI for Rust compile embedding only; baseline Rust is unchanged.
New-Item -ItemType Directory -Force ui\dist | Out-Null
Copy-Item C:\Users\sook\ferryx-wt\rex-verify-win-20261104\ui\dist\* ui\dist -Recurse -Force
cargo test --manifest-path src-tauri/Cargo.toml --lib --locked *> "$logs\baseline-lib-test.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\baseline-lib-test.log"
