$ErrorActionPreference='Continue'
Set-Location C:\Users\sook\ferryx-wt\rex-verify-win-bd4d3459
$env:CARGO_TARGET_DIR='C:\Users\sook\ferryx-wt\.target';$env:CARGO_INCREMENTAL='0';$env:CMAKE_POLICY_VERSION_MINIMUM='3.5'
$env:LIBCLANG_PATH='C:\Users\sook\ferryx-wt\.llvm\clang+llvm-23.1.4-x86_64-pc-windows-msvc\bin'
$log='.omo/evidence/rex-adoption-20261104/verify-win/bd4d3459/runtime-build.log'
cargo build --manifest-path src-tauri/Cargo.toml --locked --bin ferryx-cli *> $log
"EXIT=$LASTEXITCODE" | Add-Content $log
