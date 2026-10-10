#!/bin/sh
set -eu
case "$1" in
mac) base=/Users/I552267/ferryx-pane-completion ;;
linux) base=/home/indo/ferryx-pane-completion ;;
esac
root=$base/source-172baa87
active=$(ps -eo pid,args | grep -E '[n]ode .*task8-172baa87/(runner|ab-runner|candidate-assertion-runner).mjs|[c]argo .*source-172baa87|[n]ode .*base-pass2-d82b35e4' || true)
test -z "$active" || { printf '%s\n' "$active"; exit 1; }
if [ -L "$root/src-tauri/vendor/ghostty" ]; then unlink "$root/src-tauri/vendor/ghostty"; fi
test ! -e "$root/src-tauri/vendor/ghostty"
rm -rf "$root"
rm -f "$base/pass2-source.tar"
rm -rf "$base/base-pass2-d82b35e4"
rm -f "$base/base-pass2.tar"
if [ "$1" = linux ]; then rm -rf "$base/ghostty-172baa87"; fi
test ! -e "$root"
test ! -e "$base/base-pass2-d82b35e4"
test ! -e "$base/base-pass2.tar"
printf 'CLEANUP_OK sourceAbsent=true baseAbsent=true ghosttyLinkDetached=true archivesAbsent=true\n'

