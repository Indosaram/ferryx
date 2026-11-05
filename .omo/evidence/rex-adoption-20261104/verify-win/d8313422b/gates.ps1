$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-d8313422b'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win\d8313422b"
New-Item -ItemType Directory -Force $logs | Out-Null
Set-Location $root
$env:CARGO_TARGET_DIR='C:\Users\sook\ferryx-wt\.target'
$env:CMAKE_POLICY_VERSION_MINIMUM='3.5'
$env:LIBCLANG_PATH='C:\Users\sook\ferryx-wt\.llvm\clang+llvm-23.1.4-x86_64-pc-windows-msvc\bin'
$env:CARGO_INCREMENTAL='0'
function Gate($name,[scriptblock]$body){
 "BEGIN $name $(Get-Date -Format o)" | Add-Content "$logs\exits.log"
 & $body *> "$logs\$name.log"
 $code=$LASTEXITCODE;"EXIT=$code" | Add-Content "$logs\$name.log"
 "$name EXIT=$code $(Get-Date -Format o)" | Add-Content "$logs\exits.log"
 if($code -ne 0){throw "Gate $name failed: $code"}
}
Gate source {git submodule status}
Gate ffmpeg {bun scripts/fetch-ffmpeg.mjs}
$env:FFMPEG_DIR="$root\src-tauri\resources\ffmpeg\sdk"
$env:PATH="$env:FFMPEG_DIR\bin;$env:PATH"
Gate helper-debug {cargo build --manifest-path remote-helper/Cargo.toml --locked --target-dir remote-helper/target}
Gate helper-release {cargo build --manifest-path remote-helper/Cargo.toml --locked --release --target x86_64-pc-windows-msvc --target-dir remote-helper/target}
New-Item -ItemType Directory -Force .omo/helper-receipts,.omo/helper-artifacts/x86_64-pc-windows-msvc | Out-Null
Gate helper-receipt {bun scripts/helper-build-receipt.mjs x86_64-pc-windows-msvc remote-helper/target/x86_64-pc-windows-msvc/release/ferryx-remote-helper.exe .omo/helper-receipts/x86_64-pc-windows-msvc.json}
Copy-Item remote-helper/target/x86_64-pc-windows-msvc/release/ferryx-remote-helper.exe .omo/helper-artifacts/x86_64-pc-windows-msvc/
Gate ui-install {bun install --cwd ui --backend copy --frozen-lockfile}
Gate ui-build {bun run --cwd ui build}
Gate debug {cargo build --manifest-path src-tauri/Cargo.toml --locked --bin ferryx --bin ferryx-cli}
Gate lib-test {cargo test --manifest-path src-tauri/Cargo.toml --lib --locked}
Gate headless {cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --locked --bin ferryx-cli --bin ferryx-relay --bin ferryx-account}
'DONE' | Add-Content "$logs\exits.log"
