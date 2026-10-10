#!/bin/bash
set -u
cd /home/indo/ferryx-pane-completion/source-21dea3c0
pkill -f "cargo build --manifest-path src-tauri/Cargo.toml --features local-split-qa" 2>/dev/null || true
pkill -f "cargo test --manifest-path src-tauri/Cargo.toml" 2>/dev/null || true
sleep 2
cp /home/indo/ferryx-pane-completion/lib.rs.authoritative src-tauri/src/lib.rs
touch src-tauri/src/lib.rs
echo "LIB_HASH_RESTORED=$(sha256sum src-tauri/src/lib.rs | cut -d' ' -f1)"
echo "EXPECTED          =b32b3d3dbc274934c921cbcbc5d4cddc6418964edb9dafbf8e341c8dc8b93d57"
echo "HEAD:"; head -2 src-tauri/src/lib.rs
echo "recursion_limit_count=$(grep -c recursion_limit src-tauri/src/lib.rs)"
echo "=== remaining cargo ==="; ps -eo pid,etime,cmd | grep cargo | grep -v grep | head -3
echo "RESTORE_DONE"
