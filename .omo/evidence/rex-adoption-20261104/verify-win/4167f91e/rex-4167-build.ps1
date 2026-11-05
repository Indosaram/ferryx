$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$r='C:\Users\sook\ferryx-wt\rex-verify-win-20261104'
$l='C:\Users\sook\ferryx-wt\.evidence\rex-verify-win-4167'
New-Item -ItemType Directory -Force $l | Out-Null
try {
 Set-Location $r
 if(@(Get-Process cargo,rustc,zig -ErrorAction SilentlyContinue).Count){throw 'foreign build present; no launch'}
 git rev-parse HEAD | Out-File "$l\head.txt"
 git -C src-tauri/vendor/ghostty rev-parse HEAD | Out-File "$l\native.txt"
 & 'C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\Common7\Tools\Launch-VsDevShell.ps1' -Arch amd64 -HostArch amd64 -SkipAutomaticLocation *> "$l\devshell.log"
 $env:CARGO_TARGET_DIR='C:\Users\sook\ferryx-wt\.target'
 $env:CARGO_INCREMENTAL='0'
 $env:CMAKE_POLICY_VERSION_MINIMUM='3.5'
 $env:LIBCLANG_PATH='C:\Users\sook\ferryx-wt\.llvm\clang+llvm-23.1.4-x86_64-pc-windows-msvc\bin'
 bun scripts/fetch-ffmpeg.mjs *> "$l\ffmpeg.log"
 if($LASTEXITCODE){throw "FFmpeg exit=$LASTEXITCODE"}
 $env:FFMPEG_DIR="$r\src-tauri\resources\ffmpeg\sdk"
 $env:PATH="$env:FFMPEG_DIR\bin;$env:PATH"
 $ErrorActionPreference='Continue'
 bun install --cwd ui --backend copy --frozen-lockfile *> "$l\ui-install.log"
 $c=$LASTEXITCODE; "EXIT=$c" | Out-File "$l\ui-install-exit.txt"
 if($c){throw "UI install failed: $c"}
 bun run --cwd ui build *> "$l\ui-build.log"
 $c=$LASTEXITCODE; "EXIT=$c" | Out-File "$l\ui-build-exit.txt"
 if($c){throw "UI build failed: $c"}
 "BEGIN=$(Get-Date -Format o)" | Out-File "$l\cli-final-exit.txt"
 cargo build --manifest-path src-tauri/Cargo.toml --locked --profile windows-dev --bin ferryx-cli -vv *> "$l\cli-final.log"
 $c=$LASTEXITCODE
 $ErrorActionPreference='Stop'
 "EXIT=$c END=$(Get-Date -Format o)" | Add-Content "$l\cli-final-exit.txt"
 if($c){throw "windows-dev CLI rebuild failed: $c"}
 Get-FileHash "$env:CARGO_TARGET_DIR\windows-dev\ferryx-cli.exe" | Format-List | Out-File "$l\binary.txt"
 'SUCCESS' | Out-File "$l\done.txt"
} catch { $_ | Out-String | Out-File "$l\failure.txt"; 'FAILURE' | Out-File "$l\done.txt"; exit 1 }
