#!/bin/sh
set -eu
case "$1" in
  mac) base=/Users/I552267/ferryx-pane-completion ;;
  linux) base=/home/indo/ferryx-pane-completion ;;
  *) exit 64 ;;
esac
root="$base/source-5464da0d"
if [ -L "$root/src-tauri/vendor/ghostty" ]; then
  unlink "$root/src-tauri/vendor/ghostty"
fi
test ! -e "$root/src-tauri/vendor/ghostty"
rm -rf "$root"
test ! -e "$root"
rm -f "$base/task8-runner.mjs"
printf 'CLEANUP_OK sourceAbsent=true ghosttyLinkDetached=true\n'
