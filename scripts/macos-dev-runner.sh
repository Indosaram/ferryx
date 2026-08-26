#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" || "${1:-}" != "run" ]]; then
    exec cargo "$@"
fi

cargo_args=()
app_args=()
seen_dash_dash=false

for arg in "$@"; do
    if [[ "$seen_dash_dash" == true ]]; then
        app_args+=("$arg")
    elif [[ "$arg" == "--" ]]; then
        seen_dash_dash=true
    elif [[ "$arg" == "run" && ${#cargo_args[@]} -eq 0 ]]; then
        cargo_args+=("build")
    else
        cargo_args+=("$arg")
    fi
done

cargo "${cargo_args[@]}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ -d "$ROOT_DIR/src-tauri" ]]; then
    SRC_TAURI_DIR="$ROOT_DIR/src-tauri"
else
    SRC_TAURI_DIR="$ROOT_DIR"
fi

TARGET_DIR="${CARGO_TARGET_DIR:-$SRC_TAURI_DIR/target}/debug"
if [[ ! -d "$TARGET_DIR" && -d "$ROOT_DIR/target/debug" ]]; then
    TARGET_DIR="$ROOT_DIR/target/debug"
fi

APP_DIR="$TARGET_DIR/Ferryx.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"

mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"
cp -f "$TARGET_DIR/ferryx" "$MACOS_DIR/ferryx"
cp -f "$TARGET_DIR/Contents/Info.plist" "$CONTENTS_DIR/Info.plist"
cp -f "$SRC_TAURI_DIR/icons/icon.icns" "$RESOURCES_DIR/icon.icns"
if (( ${#app_args[@]} )); then
    exec "$MACOS_DIR/ferryx" "${app_args[@]}"
fi

exec "$MACOS_DIR/ferryx"
