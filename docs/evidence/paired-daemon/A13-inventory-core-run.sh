#!/bin/bash
set -u
cd /Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8
umask 077
root=$(mktemp -d "$PWD/.a13-inventory-XXXXXXXX")
cleanup() {
  rm -rf "$root"
  printf 'removed=%s exists=' "$root" >> docs/evidence/paired-daemon/A13-inventory-core-cleanup.log
  if test -e "$root"; then echo yes; else echo no; fi >> docs/evidence/paired-daemon/A13-inventory-core-cleanup.log
}
trap cleanup EXIT
mkdir -p "$root"/{home,runtime,data,sessions,xdg-config,xdg-cache,xdg-data,tmp}
label=$1
shift
env -i PATH=/Users/indo/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin HOME="$root/home" FERRYX_RUNTIME_DIR="$root/runtime" FERRYX_DATA_DIR="$root/data" FERRYX_SESSION_DIR="$root/sessions" FERRYX_AGENT_STATE_SOCKET="$root/runtime/agent.sock" XDG_CONFIG_HOME="$root/xdg-config" XDG_CACHE_HOME="$root/xdg-cache" XDG_DATA_HOME="$root/xdg-data" XDG_RUNTIME_DIR="$root/runtime" TMPDIR="$root/tmp" TMP="$root/tmp" TEMP="$root/tmp" CARGO_HOME=/Users/indo/.cargo RUSTUP_HOME=/Users/indo/.rustup CARGO_TARGET_DIR="$PWD/src-tauri/target" CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 RUSTC_WRAPPER= "$@" > "docs/evidence/paired-daemon/A13-inventory-core-$label.log" 2>&1
result=$?
echo "$result" > "docs/evidence/paired-daemon/A13-inventory-core-$label.exit"
exit "$result"
