#!/bin/bash
set -eu
repo=/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8
stage=$(mktemp -d /tmp/a10-saturated.XXXXXX)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage"/{home,runtime,data,sessions,config,cache,state,tmp,agents}
printf 'owned_root=%s\n' "$stage"
cd "$stage"
set +e
env -i RUST_BACKTRACE=1 PATH=/Users/indo/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin HOME="$stage/home" SHELL=/bin/sh FERRYX_RUNTIME_DIR="$stage/runtime" FERRYX_DATA_DIR="$stage/data" FERRYX_SESSION_DIR="$stage/sessions" XDG_CONFIG_HOME="$stage/config" XDG_DATA_HOME="$stage/data" XDG_CACHE_HOME="$stage/cache" XDG_STATE_HOME="$stage/state" XDG_RUNTIME_DIR="$stage/runtime" TMPDIR="$stage/tmp" TMP="$stage/tmp" TEMP="$stage/tmp" CODEX_HOME="$stage/agents/codex" CLAUDE_CONFIG_DIR="$stage/agents/claude" CARGO_HOME=/Users/indo/.cargo RUSTUP_HOME=/Users/indo/.rustup CARGO_TARGET_DIR="$repo/src-tauri/target" CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 RUSTC_WRAPPER= GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL="$stage/home/.gitconfig" cargo test --manifest-path "$repo/src-tauri/Cargo.toml" "$@"
code=$?
printf 'exit=%s owned_root_removed_by_trap=%s\n' "$code" "$stage"
exit "$code"
