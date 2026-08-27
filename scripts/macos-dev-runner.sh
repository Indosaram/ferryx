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

# `cp -f` truncates and rewrites the destination in place. When a previously launched Ferryx is still
# running from this path, mutating its executable's pages makes the kernel invalidate the code
# signature and SIGKILL it ("Code Signature Invalid", zero stack frames). Writing to a temp file and
# renaming is atomic: the running process keeps its original inode and survives the rebuild.
install_atomic() {
    local src="$1" dest="$2" tmp
    tmp="$(mktemp "${dest}.XXXXXX")"
    cp -f "$src" "$tmp"
    chmod --reference="$src" "$tmp" 2>/dev/null || chmod "$(stat -f '%OLp' "$src")" "$tmp"
    mv -f "$tmp" "$dest"
}

install_atomic "$TARGET_DIR/ferryx" "$MACOS_DIR/ferryx"
install_atomic "$TARGET_DIR/Contents/Info.plist" "$CONTENTS_DIR/Info.plist"
install_atomic "$SRC_TAURI_DIR/icons/icon.icns" "$RESOURCES_DIR/icon.icns"

# cargo's linker-signed ad-hoc signature seals the bare binary. Once it sits in a bundle beside
# Info.plist and Resources, codesign reports "code has no resources but signature indicates they must
# be present", so re-sign the assembled bundle to write Contents/_CodeSignature/CodeResources.
codesign --force --sign - "$APP_DIR" >/dev/null 2>&1 || true

if (( ${#app_args[@]} )); then
    exec "$MACOS_DIR/ferryx" "${app_args[@]}"
fi

exec "$MACOS_DIR/ferryx"
