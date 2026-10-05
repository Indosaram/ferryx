#!/bin/sh
set -eu
host=$1
shift
case "$host" in
mac) base=/Users/I552267/ferryx-pane-completion ;;
linux) base=/home/indo/ferryx-pane-completion ;;
esac
export PATH="$HOME/.cargo/bin:$HOME/.bun/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
root=$base/base-pass2-d82b35e4
out=$base/task8-172baa87/base-ab
test ! -e "$root"
mkdir -p "$root" "$out"
tar -xf "$base/base-pass2.tar" -C "$root"
cd "$root"
bun install --cwd ui --frozen-lockfile
node "$base/task8-172baa87/ab-runner.mjs" "$root" "$out" "$host" "$@"

