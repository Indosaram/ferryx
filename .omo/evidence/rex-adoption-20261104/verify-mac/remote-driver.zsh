#!/bin/zsh
set -u
cd /Users/I552267/code/ferryx-wt/rex-verify-mac-20261104 || exit 1
export PATH=/Users/I552267/code/ferryx-wt/.zig-0.17.0:$PATH
export ZIG=/Users/I552267/code/ferryx-wt/.zig-0.17.0/zig
export CARGO_TARGET_DIR=/Users/I552267/code/ferryx-wt/.target
export CARGO_BUILD_JOBS=4 CMAKE_POLICY_VERSION_MINIMUM=3.5
L=/tmp/rex-verify-mac-20261104
mkdir -p $L
run() { local name=$1; shift; printf '%s\n' "$*" > $L/$name.cmd; "$@" > $L/$name.log 2>&1; local rc=$?; printf '%s EXIT=%s\n' $name $rc >> $L/exits.log; }
git rev-parse HEAD > $L/head.txt
run ui-install bun install --cwd ui --frozen-lockfile
run ui-build bun run --cwd ui build
run ffmpeg bun scripts/fetch-ffmpeg.mjs
run metadata cargo metadata --manifest-path src-tauri/Cargo.toml --locked --format-version 1 --no-deps
run full-lib cargo test --manifest-path src-tauri/Cargo.toml --lib --locked
run cli-build cargo build --manifest-path src-tauri/Cargo.toml --locked --bin ferryx-cli
run handover cargo test --manifest-path src-tauri/Cargo.toml --locked --test daemon_handover_transfer_contract
run hooks cargo test --manifest-path src-tauri/Cargo.toml --locked --no-default-features --test ferryx_cli_agent_hook
run release cargo build --release --manifest-path src-tauri/Cargo.toml --locked
printf 'DONE\n' >> $L/exits.log
