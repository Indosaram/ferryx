#!/bin/sh
# Push the frozen repaired files into the staged tree and verify sha256.
set -eu
host=$1
case "$host" in
mac) base=/Users/I552267/ferryx-pane-completion ;;
linux) base=/home/indo/ferryx-pane-completion ;;
esac
root=$base/source-21dea3c0
stage=/tmp/pass3-delta-$host
cd $root
for f in src-tauri/src/native_terminal/surface_host.rs src-tauri/src/daemon/client.rs src-tauri/src/daemon/handover.rs src-tauri/tests/daemon_handover_contract.rs src-tauri/tests/daemon_persistence_contract.rs src-tauri/tests/zero_config_gen4_audit.rs; do
  cp "$stage/$f" "$root/$f"
done
if [ "$host" = mac ]; then
  shasum -a 256 src-tauri/src/native_terminal/surface_host.rs src-tauri/src/daemon/client.rs src-tauri/src/daemon/handover.rs src-tauri/tests/daemon_handover_contract.rs src-tauri/tests/daemon_persistence_contract.rs src-tauri/tests/zero_config_gen4_audit.rs
else
  sha256sum src-tauri/src/native_terminal/surface_host.rs src-tauri/src/daemon/client.rs src-tauri/src/daemon/handover.rs src-tauri/tests/daemon_handover_contract.rs src-tauri/tests/daemon_persistence_contract.rs src-tauri/tests/zero_config_gen4_audit.rs
fi
printf 'DELTA_PUSHED_OK host=%s\n' "$host"
