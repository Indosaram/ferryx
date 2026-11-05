$ErrorActionPreference='Continue'
$root='C:\Users\sook\ferryx-wt\rex-verify-win-d8313422b'
$logs="$root\.omo\evidence\rex-adoption-20261104\verify-win\d8313422b"
Set-Location $root
New-Item -ItemType Directory -Force .qa/wasm | Out-Null
& C:\Users\sook\ferryx-wt\.zig-0.16.0\zig.exe cc -fno-sanitize=all -target wasm32-freestanding -nostdlib '-Wl,--no-entry' '-Wl,--export-memory' -o .qa/wasm/clock.wasm examples/plugins/clock/clock.c *> "$logs\clock-compile.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\clock-compile.log"
Get-FileHash .qa/wasm/clock.wasm -Algorithm SHA256 | Format-List | Out-File "$logs\clock-digest.log"
