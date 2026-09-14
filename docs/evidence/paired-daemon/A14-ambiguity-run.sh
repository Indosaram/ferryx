#!/bin/bash
set -u
cd /Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8
root="$PWD/.a14a"
export PATH="$PWD/.a14-ambiguity-tools/toolchain/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
export CARGO_HOME="$PWD/.a14-ambiguity-tools/cargo" CARGO_TARGET_DIR="$PWD/src-tauri/target"
export HOME="$root/home" TMPDIR="$root/tmp" TMP="$root/tmp" TEMP="$root/tmp"
export FERRYX_RUNTIME_DIR="$root/runtime" FERRYX_DATA_DIR="$root/data" FERRYX_SESSION_DIR="$root/sessions"
export XDG_CONFIG_HOME="$root/config" XDG_DATA_HOME="$root/data" XDG_CACHE_HOME="$root/cache" XDG_RUNTIME_DIR="$root/runtime"
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL="$root/home/.gitconfig" GIT_CEILING_DIRECTORIES="$root"
export CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 RUSTC_WRAPPER=
unset RUSTUP_HOME
printf '$'; printf ' %q' "$@"; printf '\n'
"$@"
status=$?
printf '\nEXIT_CODE=%s\n' "$status"
exit "$status"
