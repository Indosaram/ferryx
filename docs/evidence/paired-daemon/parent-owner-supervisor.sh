#!/usr/bin/env bash
# Private supervisor for tests/machine_owner_handover.rs, which refuses to run
# unless HOME sits under a directory named a10-owner-* and every runtime/data/
# session/XDG/TMPDIR variable resolves below that same root. This keeps the
# user's real HOME, canonical daemon and PTYs untouched.
set -u
root="$(mktemp -d /tmp/a10-owner-parent.XXXXXX)"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export HOME="$root/home"
export FERRYX_RUNTIME_DIR="$root/runtime"
export FERRYX_DATA_DIR="$root/data"
export FERRYX_SESSION_DIR="$root/sessions"
export XDG_CONFIG_HOME="$root/config"
export XDG_DATA_HOME="$root/share"
export XDG_CACHE_HOME="$root/cache"
export TMPDIR="$root/tmp"
mkdir -p "$HOME" "$FERRYX_RUNTIME_DIR" "$FERRYX_DATA_DIR" "$FERRYX_SESSION_DIR" \
         "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$TMPDIR"
chmod 700 "$root" "$HOME" "$FERRYX_RUNTIME_DIR" "$FERRYX_DATA_DIR" "$FERRYX_SESSION_DIR" \
          "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$TMPDIR"
echo "supervisor_root=$root HOME=$HOME"
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features \
  --test machine_owner_handover
code=$?
rm -rf "$root"
echo "supervisor_root_removed=$? test_exit=$code"
exit "$code"
