$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-20261104'
Set-Location $root
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win"
New-Item -ItemType Directory -Force $logs | Out-Null
$env:CARGO_TARGET_DIR='C:\Users\sook\ferryx-wt\.target'
$env:CMAKE_POLICY_VERSION_MINIMUM='3.5'
$env:LIBCLANG_PATH='C:\Users\sook\ferryx-wt\.llvm\clang+llvm-23.1.4-x86_64-pc-windows-msvc\bin'
$env:FFMPEG_DIR="$root\src-tauri\resources\ffmpeg\sdk"
function Run($name, [scriptblock]$action) {
  "BEGIN $name $(Get-Date -Format o)" | Add-Content "$logs\exits.log"
  & $action *> "$logs\$name.log"
  "EXIT=$LASTEXITCODE" | Add-Content "$logs\$name.log"
  "$name EXIT=$LASTEXITCODE $(Get-Date -Format o)" | Add-Content "$logs\exits.log"
}
Run metadata { cargo metadata --manifest-path src-tauri/Cargo.toml --locked --no-deps --format-version 1 }
Run submodule { git submodule update --init --depth 1 src-tauri/vendor/ghostty }
Run ffmpeg { bun scripts/fetch-ffmpeg.mjs }
Run ui-install { bun install --cwd ui --backend copy --frozen-lockfile }
Run ui-build { bun run --cwd ui build }
Run lib-test { cargo test --manifest-path src-tauri/Cargo.toml --lib --locked }
Run headless { cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --locked --bin ferryx-cli --bin ferryx-relay --bin ferryx-account }
Run debug { cargo build --manifest-path src-tauri/Cargo.toml --locked --bin ferryx --bin ferryx-cli }
Run release { cargo build --release --manifest-path src-tauri/Cargo.toml --locked }
'DONE' | Add-Content "$logs\exits.log"
