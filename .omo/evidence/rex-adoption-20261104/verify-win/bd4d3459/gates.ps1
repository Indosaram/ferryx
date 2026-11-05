$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-bd4d3459'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win\bd4d3459"
New-Item -ItemType Directory -Force $logs | Out-Null
Set-Location $root
$env:CARGO_TARGET_DIR='C:\Users\sook\ferryx-wt\.target'
$env:CARGO_INCREMENTAL='0'
$env:CMAKE_POLICY_VERSION_MINIMUM='3.5'
$env:LIBCLANG_PATH='C:\Users\sook\ferryx-wt\.llvm\clang+llvm-23.1.4-x86_64-pc-windows-msvc\bin'
function Run($name,[scriptblock]$body){"BEGIN $name $(Get-Date -Format o)" | Add-Content "$logs\exits.log";& $body *> "$logs\$name.log";$code=$LASTEXITCODE;"EXIT=$code" | Add-Content "$logs\$name.log";"$name EXIT=$code $(Get-Date -Format o)" | Add-Content "$logs\exits.log"}
Run ffmpeg {bun scripts/fetch-ffmpeg.mjs}
$env:FFMPEG_DIR="$root\src-tauri\resources\ffmpeg\sdk";$env:PATH="$env:FFMPEG_DIR\bin;$env:PATH"
Run helper-debug {cargo build --manifest-path remote-helper/Cargo.toml --locked --target-dir remote-helper/target}
Run helper-release {cargo build --manifest-path remote-helper/Cargo.toml --locked --release --target x86_64-pc-windows-msvc --target-dir remote-helper/target}
New-Item -ItemType Directory -Force .omo/helper-receipts,.omo/helper-artifacts/x86_64-pc-windows-msvc | Out-Null
Run helper-receipt {bun scripts/helper-build-receipt.mjs x86_64-pc-windows-msvc remote-helper/target/x86_64-pc-windows-msvc/release/ferryx-remote-helper.exe .omo/helper-receipts/x86_64-pc-windows-msvc.json}
Copy-Item remote-helper/target/x86_64-pc-windows-msvc/release/ferryx-remote-helper.exe .omo/helper-artifacts/x86_64-pc-windows-msvc/
Run ui-install {bun install --cwd ui --backend copy --frozen-lockfile}
Run ui-build {bun run --cwd ui build}
Run original {cargo test --manifest-path src-tauri/Cargo.toml --locked --lib -- daemon::layout:: daemon::layout_migration:: daemon::handover::spawn_owner_tests:: daemon::agent_extension::p09_tests:: ipc::debug::pane_liveness_diagnostics_tests:: daemon::supervisor_peer::tests::verify_win_peer_11476_exited_ssh_ancestor_932_owner_and_pane_cases native_terminal::osc_effects::tests::osc_expectation_table}
Run added {cargo test --manifest-path src-tauri/Cargo.toml --locked --lib -- daemon::supervisor_peer:: native_terminal::osc_effects::}
Run all-targets {cargo check --manifest-path src-tauri/Cargo.toml --locked --all-targets}
Run headless {cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --locked --all-targets}
# Serial libtest output identifies the currently executing test on any stall. No skip/filter.
Run full-lib {cargo test --manifest-path src-tauri/Cargo.toml --locked --lib -- --test-threads=1 --nocapture}
'DONE' | Add-Content "$logs\exits.log"
