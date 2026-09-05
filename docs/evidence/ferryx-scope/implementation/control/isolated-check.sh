#!/bin/bash
# Uses the lead-built library and existing warm dependency artifacts; no new cache.
set -eu
deps=src-tauri/target/debug/deps
args=()
for name in tokio serde serde_json parking_lot futures_util axum base64 reqwest tempfile portable_pty; do
  artifact=$(find "$deps" -maxdepth 1 -name "lib${name}-*.rlib" -print -quit)
  args+=(--extern "$name=$artifact")
done
rustc --edition=2021 --test src-tauri/tests/scoped_control.rs -L "dependency=$deps" --extern "ferryx_lib=$deps/libferryx_lib.rlib" "${args[@]}" -o src-tauri/target/debug/scoped-control-isolated
src-tauri/target/debug/scoped-control-isolated --nocapture
