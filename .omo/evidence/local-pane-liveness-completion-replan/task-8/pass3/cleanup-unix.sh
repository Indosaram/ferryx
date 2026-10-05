#!/bin/sh
set -eu
host=$1
case "$host" in
mac) base=/Users/I552267/ferryx-pane-completion ;;
linux) base=/home/indo/ferryx-pane-completion ;;
esac
root=$base/source-21dea3c0
out=$base/task8-21dea3c0
active=$(ps -eo pid,args | grep -E "[n]ode .*task8-21dea3c0/runner3.mjs|[c]argo .*source-21dea3c0|[b]un .*source-21dea3c0" || true)
test -z "$active" || { printf '%s\n' "$active"; exit 1; }
if [ -L "$root/src-tauri/vendor/ghostty" ]; then unlink "$root/src-tauri/vendor/ghostty"; fi
test ! -e "$root/src-tauri/vendor/ghostty"
rm -rf "$root"
rm -f "$base/pass3-source.tar.gz"
if [ "$1" = linux ]; then rm -rf "$base/ghostty-21dea3c0"; fi
test ! -e "$root"
printf 'CLEANUP_OK sourceAbsent=true ghosttyLinkDetached=true archiveAbsent=true\n'
