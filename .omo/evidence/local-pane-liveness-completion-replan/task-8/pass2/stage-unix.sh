#!/bin/sh
set -eu
host=$1
case "$host" in
mac) base=/Users/I552267/ferryx-pane-completion; ghost=/Users/I552267/ferryx-ghostty ;;
linux) base=/home/indo/ferryx-pane-completion; ghost=$base/ghostty-172baa87 ;;
esac
export PATH="$HOME/.cargo/bin:$HOME/.bun/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
printf 'PROBE_OK\n'; hostname; whoami; rustc --version; cargo --version; bun --version; node --version
root=$base/source-172baa87
out=$base/task8-172baa87
test ! -e "$root"
mkdir -p "$root" "$out"
tar -xf "$base/pass2-source.tar" -C "$root"
if [ "$host" = linux ]; then git clone /home/indo/ghostty.bundle "$ghost"; git -C "$ghost" checkout 6a508fd5e34c7e222c052a6d00bb3891ff3feace; fi
git -C "$ghost" rev-parse HEAD
rmdir "$root/src-tauri/vendor/ghostty"
ln -s "$ghost" "$root/src-tauri/vendor/ghostty"
if [ "$host" = mac ]; then shasum -a 256 "$root/src-tauri/Cargo.lock" "$root/ui/bun.lock"; else sha256sum "$root/src-tauri/Cargo.lock" "$root/ui/bun.lock"; fi
cd "$root"
bun install --cwd ui --frozen-lockfile
export CARGO_TARGET_DIR="$root/target"
node "$out/runner.mjs" "$root" "$out" "$host"

