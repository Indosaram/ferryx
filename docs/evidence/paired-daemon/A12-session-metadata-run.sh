#!/bin/bash
set -u
cd /Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8 || exit 1
umask 077
root=$(mktemp -d /private/tmp/a12-metadata.XXXXXXXX) || exit 1
label=$1; shift
evidence="$PWD/docs/evidence/paired-daemon/A12-session-metadata-$label"
cleanup() {
  rm -rf "$root"
  printf 'private_root=%s removal_exit=%s\n' "$root" "$?" >> "$evidence.cleanup"
}
trap cleanup EXIT
mkdir -p "$root"/{home,runtime,data,sessions,xdg-config,xdg-cache,xdg-data,tmp}
env -i PATH=/Users/indo/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin HOME="$root/home" FERRYX_RUNTIME_DIR="$root/runtime" FERRYX_DATA_DIR="$root/data" FERRYX_SESSION_DIR="$root/sessions" FERRYX_AGENT_STATE_SOCKET="$root/runtime/agent.sock" XDG_CONFIG_HOME="$root/xdg-config" XDG_CACHE_HOME="$root/xdg-cache" XDG_DATA_HOME="$root/xdg-data" XDG_RUNTIME_DIR="$root/runtime" TMPDIR="$root/tmp" TMP="$root/tmp" TEMP="$root/tmp" CARGO_HOME=/Users/indo/.cargo RUSTUP_HOME=/Users/indo/.rustup CARGO_TARGET_DIR="$PWD/src-tauri/target" CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 RUSTC_WRAPPER= TERM=xterm-256color A12_PROVIDER_FIXTURE_SHELL=/opt/homebrew/bin/bash "$@" > "$evidence.log" 2>&1 &
pid=$!
printf 'owner=%s command=' "$pid" > "$evidence.monitor"; printf '%q ' "$@" >> "$evidence.monitor"; echo >> "$evidence.monitor"
ps -o pid,ppid,rss,etime,command -p "$pid" >> "$evidence.monitor"
wait "$pid"; result=$?
printf 'exit=%s owner_reaped=true\n' "$result" >> "$evidence.monitor"
echo "$result" > "$evidence.exit"
exit "$result"
