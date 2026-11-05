$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-03e0d6b'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win\03e0d6b"
New-Item -ItemType Directory -Force $logs | Out-Null
Set-Location $root
$env:CARGO_TARGET_DIR='C:\Users\sook\ferryx-wt\.target'
$env:CARGO_INCREMENTAL='0'
$env:CMAKE_POLICY_VERSION_MINIMUM='3.5'
$env:LIBCLANG_PATH='C:\Users\sook\ferryx-wt\.llvm\clang+llvm-23.1.4-x86_64-pc-windows-msvc\bin'
function Run($name,[scriptblock]$body){"HEAD=$(git rev-parse HEAD) BEGIN $name $(Get-Date -Format o)" | Add-Content "$logs\exits.log";& $body *> "$logs\$name.log";$code=$LASTEXITCODE;"EXIT=$code" | Add-Content "$logs\$name.log";"$name EXIT=$code $(Get-Date -Format o)" | Add-Content "$logs\exits.log"}
Run ffmpeg {bun scripts/fetch-ffmpeg.mjs}
$env:FFMPEG_DIR="$root\src-tauri\resources\ffmpeg\sdk";$env:PATH="$env:FFMPEG_DIR\bin;$env:PATH"
Run helper-debug {cargo build --manifest-path remote-helper/Cargo.toml --locked --target-dir remote-helper/target}
Run helper-release {cargo build --manifest-path remote-helper/Cargo.toml --locked --release --target x86_64-pc-windows-msvc --target-dir remote-helper/target}
New-Item -ItemType Directory -Force .omo/helper-receipts,.omo/helper-artifacts/x86_64-pc-windows-msvc | Out-Null
Run helper-receipt {bun scripts/helper-build-receipt.mjs x86_64-pc-windows-msvc remote-helper/target/x86_64-pc-windows-msvc/release/ferryx-remote-helper.exe .omo/helper-receipts/x86_64-pc-windows-msvc.json}
Copy-Item remote-helper/target/x86_64-pc-windows-msvc/release/ferryx-remote-helper.exe .omo/helper-artifacts/x86_64-pc-windows-msvc/
Run ui-install {bun install --cwd ui --backend copy --frozen-lockfile}
Run ui-build {bun run --cwd ui build}
Run cli-default {cargo build --manifest-path src-tauri/Cargo.toml --locked --bin ferryx-cli -vv}
Run repaired {cargo test --manifest-path src-tauri/Cargo.toml --locked --lib -- daemon::session_service::machine_tests::desktop_inventory_isolated_parent remote::reference_chat::route_tests::a_stage_for_an_ssh_remote_pane_is_refused_and_writes_nothing_locally}
Run platform-contract {cargo test --manifest-path src-tauri/Cargo.toml --locked --test platform_build_contract}
Run headless {cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --locked --all-targets}
Run full-lib {cargo test --manifest-path src-tauri/Cargo.toml --locked --lib -- --test-threads=1 --nocapture}
'DONE' | Add-Content "$logs\exits.log"
