#!/bin/bash
# Run only after the lead grants the exclusive shared Darwin Cargo slot.
set -euo pipefail
cd "$(dirname "$0")/../../.."
if [[ "${P23_CARGO_SLOT_GRANTED:-}" != "st_01a09a09" ]]; then
  echo 'BLOCKED: lead-issued exclusive Cargo slot required (P23_CARGO_SLOT_GRANTED=st_01a09a09)' >&2
  exit 64
fi
phase="${1:?expected red or green}"
case "$phase" in red|green) ;; *) exit 64 ;; esac
log="docs/evidence/windows-review-20260913/p23-${phase}.log"
{
  date -u
  rustc -Vv
  cargo -V
  shasum -a 256 src-tauri/src/remote/relay_server.rs src-tauri/src/remote/auth.rs
  cargo test --manifest-path src-tauri/Cargo.toml --lib remote::relay_server::tests::test_relay_key_store_transaction_is_cross_process -- --exact --nocapture --test-threads=1
} 2>&1 | tee "$log"
