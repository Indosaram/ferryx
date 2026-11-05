$ErrorActionPreference='Continue'
Set-Location C:\Users\sook\ferryx-wt\rex-verify-win-20261104
$logs="$PWD\.omo\evidence\rex-adoption-20261104\verify-win"
git -c protocol.file.allow=always submodule update --init src-tauri/vendor/ghostty *> "$logs\submodule-repaired.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\submodule-repaired.log"
Remove-Item Env:FFMPEG_DIR -ErrorAction SilentlyContinue
bun scripts/fetch-ffmpeg.mjs *> "$logs\ffmpeg-repaired.log"
"EXIT=$LASTEXITCODE" | Add-Content "$logs\ffmpeg-repaired.log"
