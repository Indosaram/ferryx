$ErrorActionPreference='Continue';$ProgressPreference='SilentlyContinue';$l='C:\Users\sook\ferryx-wt\.evidence\rex-verify-win-4167';Set-Location C:\Users\sook\ferryx-wt\rex-verify-win-20261104
& 'C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\Common7\Tools\Launch-VsDevShell.ps1' -Arch amd64 -HostArch amd64 -SkipAutomaticLocation *> "$l\contract-devshell.log"
$env:CARGO_TARGET_DIR='C:\Users\sook\ferryx-wt\.target';$env:CARGO_INCREMENTAL='0';$env:CMAKE_POLICY_VERSION_MINIMUM='3.5';$env:LIBCLANG_PATH='C:\Users\sook\ferryx-wt\.llvm\clang+llvm-23.1.4-x86_64-pc-windows-msvc\bin';$env:FFMPEG_DIR="$PWD\src-tauri\resources\ffmpeg\sdk"
cargo test --manifest-path src-tauri/Cargo.toml --locked --profile windows-dev --test platform_build_contract *> "$l\platform-contract.log"
"EXIT=$LASTEXITCODE" | Out-File "$l\platform-contract-exit.txt"
