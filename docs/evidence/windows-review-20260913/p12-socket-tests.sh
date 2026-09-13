#!/bin/sh
# Lead-owned execution gate: only run after granting the exclusive Darwin Cargo slot.
set -eu
if [ "${1-}" != "--lead-issued-exclusive-slot" ]; then
  printf '%s\n' 'P12 staged only. Requires lead-issued exclusive Cargo slot; no tests executed.' >&2
  exit 2
fi
cd /Users/indo/code/project/orca-lite
# Both tests own only ephemeral sockets and in-memory browser state.
# Run both even when the first reports the intended authorization failure.
status=0
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::browser_cli::tests::p12_tcp_rejects_unauthenticated_commands -- --exact --nocapture || status=$?
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::browser_cli::tests::p12_tcp_rejects_forged_credential -- --exact --nocapture || status=$?
exit "$status"
