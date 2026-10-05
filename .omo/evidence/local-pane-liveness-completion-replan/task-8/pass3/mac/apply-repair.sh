#!/bin/sh
# Apply ONLY the two-line surface_host.rs repair to the already-staged pass-3 tree.
# Provenance: tree = git archive 21dea3c0 (UI bytes unchanged, ui/dist retained) + this patch.
set -eu
host=$1
case "$host" in
mac) base=/Users/I552267/ferryx-pane-completion ;;
linux) base=/home/indo/ferryx-pane-completion ;;
esac
root=$base/source-21dea3c0
out=$base/task8-21dea3c0
before=$(if [ "$host" = mac ]; then shasum -a 256 "$root/src-tauri/src/native_terminal/surface_host.rs" | cut -d' ' -f1; else sha256sum "$root/src-tauri/src/native_terminal/surface_host.rs" | cut -d' ' -f1; fi)
echo "BEFORE_SHA=$before"
test "$before" = "6e4928ce757aeef8da626ec33a20573d28976b1be430fa180e7bfc8a76e31cc9" || { echo "UNEXPECTED_PRE_SHA"; exit 2; }
cd "$root"
git apply --verbose "$out/surface_host-repair.patch"
after=$(if [ "$host" = mac ]; then shasum -a 256 "$root/src-tauri/src/native_terminal/surface_host.rs" | cut -d' ' -f1; else sha256sum "$root/src-tauri/src/native_terminal/surface_host.rs" | cut -d' ' -f1; fi)
echo "AFTER_SHA=$after"
test "$after" = "9039b136990166a7edb3d264a0249848d69ee888379e6107c216a6cdb2fe696a" || { echo "PATCH_SHA_MISMATCH"; exit 3; }
echo "DELTA_APPLIED_OK host=$host"
