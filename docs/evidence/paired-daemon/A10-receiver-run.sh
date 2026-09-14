#!/bin/bash
set -u
cd /Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8
stage=$(mktemp -d /tmp/a10-receiver.XXXXXX)
mkdir -p "$stage"/{home,runtime,data,sessions,config,cache,state,tmp}
export HOME="$stage/home" FERRYX_RUNTIME_DIR="$stage/runtime" FERRYX_DATA_DIR="$stage/data" FERRYX_SESSION_DIR="$stage/sessions" XDG_CONFIG_HOME="$stage/config" XDG_DATA_HOME="$stage/data" XDG_CACHE_HOME="$stage/cache" XDG_STATE_HOME="$stage/state" XDG_RUNTIME_DIR="$stage/runtime" TMPDIR="$stage/tmp" TMP="$stage/tmp" TEMP="$stage/tmp" CARGO_HOME=/Users/indo/.cargo RUSTUP_HOME=/Users/indo/.rustup CARGO_TARGET_DIR="$PWD/src-tauri/target" CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 RUSTC_WRAPPER= GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL="$stage/home/.gitconfig"
label=$1; shift
printf 'stage=%s\ncommand=cargo %s\n' "$stage" "$*"
cargo "$@"
code=$?
printf 'exit=%s\n' "$code"
rm -rf "$stage"
exit "$code"
